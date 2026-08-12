import { invoke } from "@tauri-apps/api/core";
import { Terminal } from "@xterm/xterm";

const inputEchoToken = "__WINGMAN_INPUT_ECHO_PROBE__";
const inputEchoCommand = `# ${inputEchoToken}\r`;
const bulkStart = "__WINGMAN_BULK_START__\r\n";
const bulkEnd = "__WINGMAN_BULK_END__\r\n";
const bulkExpectedLength = 11_900_000;
const bulkExpectedHash = 0x48bac225;
const bulkCommand =
  "$p=([char]0xe9).ToString()*55;" +
  "$s='__WINGMAN_BULK_'+'START__';$e='__WINGMAN_BULK_'+'END__';" +
  '[Console]::Out.Write($s+"`r`n");' +
  'for($i=0;$i -lt 100000;$i++){[Console]::Out.Write(("{0:D6}:{1}`r`n" -f $i,$p))};' +
  '[Console]::Out.Write($e+"`r`n")\r';
const latencyEnd = "__WINGMAN_LATENCY_END__\r\n";
const latencyMarkerCarryLength =
  "__WINGMAN_LATENCY_READY_000__\r\n".length - 1;
const latencyCommand =
  "$p=([char]0xe9).ToString()*55;" +
  "$r='__WINGMAN_LATENCY_'+'READY_';$a='__WINGMAN_LATENCY_'+'ECHO_';" +
  "$z='__WINGMAN_LATENCY_'+'END__';" +
  'for($s=0;$s -lt 100;$s++){for($l=0;$l -lt 1000;$l++){' +
  '$i=$s*1000+$l;[Console]::Out.Write(("{0:D6}:{1}`r`n" -f $i,$p))};' +
  '[Console]::Out.Write((($r+"{0:D3}__`r`n") -f $s));' +
  'while(-not [Console]::KeyAvailable){Start-Sleep -Milliseconds 1};' +
  '[void][Console]::ReadKey($true);' +
  '[Console]::Out.Write((($a+"{0:D3}__`r`n") -f $s))};' +
  '[Console]::Out.Write($z+"`r`n")\r';

type ProbeKind = "input-echo" | "bulk-output" | "bulk-latency";
type BulkState = "waiting-start" | "body" | "complete" | "invalid";
type VtState = "ground" | "escape" | "csi" | "string" | "string-escape";
type LatencyObservation = {
  echoIndexes: number[];
  endObserved: boolean;
};

type ProbeAvailability = {
  accepted: boolean;
  enabled: boolean;
};

export class PerformanceProbe {
  private observed = false;
  private bulkState: BulkState = "waiting-start";
  private bulkCarry = "";
  private bodyLength = 0;
  private bodyHash = 0x811c9dc5;
  private vtState: VtState = "ground";
  private latencyCarry = "";
  private nextLatencyReady = 0;
  private nextLatencyEcho = 0;
  private readonly latencyStarts = new Map<number, number>();
  private readonly latencySamples: number[] = [];
  private latencyEndSeen = false;
  private latencyInvalid = false;

  private constructor(
    private readonly sessionId: number,
    private readonly kind: ProbeKind,
    private readonly terminal: Terminal,
  ) {}

  static async start(
    sessionId: number,
    terminal: Terminal,
    isCurrentSession: () => boolean,
  ): Promise<PerformanceProbe | null> {
    const latency = await queryAvailability(
      "performance_bulk_latency_probe",
      sessionId,
    );
    if (latency.accepted && latency.enabled && isCurrentSession()) {
      const probe = new PerformanceProbe(sessionId, "bulk-latency", terminal);
      terminal.input(latencyCommand, true);
      return probe;
    }

    const bulk = await queryAvailability(
      "performance_bulk_output_probe",
      sessionId,
    );
    if (bulk.accepted && bulk.enabled && isCurrentSession()) {
      const probe = new PerformanceProbe(sessionId, "bulk-output", terminal);
      terminal.input(bulkCommand, true);
      return probe;
    }

    const input = await queryAvailability(
      "performance_input_echo_probe",
      sessionId,
    );
    if (input.accepted && input.enabled && isCurrentSession()) {
      const probe = new PerformanceProbe(sessionId, "input-echo", terminal);
      terminal.input(inputEchoCommand, true);
      return probe;
    }
    return null;
  }

  write(sessionId: number, data: string): boolean {
    if (sessionId !== this.sessionId || this.observed) return false;
    if (this.kind === "bulk-latency") {
      const observation = this.observeLatencyOutput(data);
      this.terminal.write(data, () => this.checkRenderedLatency(observation));
      return true;
    }
    if (this.kind === "bulk-output") this.observeBulkOutput(data);
    this.terminal.write(data, () => this.checkRenderedCompletion());
    return true;
  }

  private checkRenderedCompletion() {
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        if (this.observed) return;
        const ready =
          this.kind === "input-echo"
            ? renderedTerminalContains(this.terminal, inputEchoToken)
            : this.bulkState === "complete" &&
              renderedTerminalContains(this.terminal, bulkEnd.trim());
        if (!ready) return;

        this.observed = true;
        const command =
          this.kind === "input-echo"
            ? "mark_performance_input_echo"
            : "mark_performance_bulk_output";
        void invoke(command, { clientSessionId: this.sessionId });
      });
    });
  }

  private observeBulkOutput(data: string) {
    if (this.bulkState === "complete" || this.bulkState === "invalid") return;

    let input = this.bulkCarry + data;
    this.bulkCarry = "";
    if (this.bulkState === "waiting-start") {
      const markerIndex = input.indexOf(bulkStart);
      if (markerIndex < 0) {
        this.bulkCarry = input.slice(-(bulkStart.length - 1));
        return;
      }
      input = input.slice(markerIndex + bulkStart.length);
      this.bulkState = "body";
    }

    const endIndex = input.indexOf(bulkEnd);
    if (endIndex >= 0) {
      this.hashBody(input.slice(0, endIndex));
      this.bulkCarry = "";
      this.bulkState =
        this.bodyLength === bulkExpectedLength &&
        this.bodyHash === bulkExpectedHash &&
        this.vtState === "ground"
          ? "complete"
          : "invalid";
      return;
    }

    const retainedLength = Math.min(input.length, bulkEnd.length - 1);
    const bodyEnd = input.length - retainedLength;
    this.hashBody(input.slice(0, bodyEnd));
    this.bulkCarry = input.slice(bodyEnd);
  }

  private observeLatencyOutput(data: string): LatencyObservation {
    const input = this.latencyCarry + data;
    const echoIndexes: number[] = [];
    let consumed = 0;
    const marker = /__WINGMAN_LATENCY_(READY|ECHO)_(\d{3})__\r\n/g;
    for (let match = marker.exec(input); match; match = marker.exec(input)) {
      consumed = marker.lastIndex;
      const index = Number(match[2]);
      if (match[1] === "READY") {
        if (
          index !== this.nextLatencyReady ||
          this.latencyStarts.has(index) ||
          index >= 100
        ) {
          this.latencyInvalid = true;
          continue;
        }
        this.nextLatencyReady += 1;
        this.latencyStarts.set(index, performance.now());
        this.terminal.input(".", true);
        continue;
      }
      if (
        index !== this.nextLatencyEcho ||
        !this.latencyStarts.has(index) ||
        index >= 100
      ) {
        this.latencyInvalid = true;
        continue;
      }
      this.nextLatencyEcho += 1;
      echoIndexes.push(index);
    }

    const endIndex = input.indexOf(latencyEnd);
    const endObserved = !this.latencyEndSeen && endIndex >= 0;
    if (endIndex >= 0) consumed = Math.max(consumed, endIndex + latencyEnd.length);
    if (endObserved) this.latencyEndSeen = true;
    this.latencyCarry = input.slice(
      Math.max(consumed, input.length - latencyMarkerCarryLength),
    );
    return { echoIndexes, endObserved };
  }

  private checkRenderedLatency(observation: LatencyObservation) {
    if (observation.echoIndexes.length === 0 && !observation.endObserved) return;
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        if (this.observed || this.latencyInvalid) return;
        for (const index of observation.echoIndexes) {
          const startedAt = this.latencyStarts.get(index);
          if (startedAt === undefined || index !== this.latencySamples.length) {
            this.latencyInvalid = true;
            return;
          }
          this.latencyStarts.delete(index);
          this.latencySamples.push(performance.now() - startedAt);
        }
        if (
          !this.latencyEndSeen ||
          this.nextLatencyReady !== 100 ||
          this.nextLatencyEcho !== 100 ||
          this.latencyStarts.size !== 0 ||
          this.latencySamples.length !== 100
        ) {
          return;
        }

        this.observed = true;
        void invoke("mark_performance_bulk_latency", {
          clientSessionId: this.sessionId,
          samplesMs: [...this.latencySamples],
        });
      });
    });
  }

  private hashBody(body: string) {
    for (let index = 0; index < body.length; index += 1) {
      const code = body.charCodeAt(index);
      if (this.vtState === "ground") {
        if (code === 0x1b) {
          this.vtState = "escape";
        } else {
          const codePoint = body.codePointAt(index)!;
          this.hashCodePoint(codePoint);
          if (codePoint > 0xffff) index += 1;
        }
        continue;
      }
      if (this.vtState === "escape") {
        if (body[index] === "[") {
          this.vtState = "csi";
        } else if (body[index] === "]" || body[index] === "P") {
          this.vtState = "string";
        } else {
          this.vtState = "ground";
        }
        continue;
      }
      if (this.vtState === "csi") {
        if (code >= 0x40 && code <= 0x7e) this.vtState = "ground";
        continue;
      }
      if (this.vtState === "string") {
        if (code === 0x07) {
          this.vtState = "ground";
        } else if (code === 0x1b) {
          this.vtState = "string-escape";
        }
        continue;
      }
      if (body[index] === "\\") {
        this.vtState = "ground";
      } else if (code !== 0x1b) {
        this.vtState = "string";
      }
    }
  }

  private hashCodePoint(codePoint: number) {
    if (codePoint <= 0x7f) {
      this.hashByte(codePoint);
    } else if (codePoint <= 0x7ff) {
      this.hashByte(0xc0 | (codePoint >> 6));
      this.hashByte(0x80 | (codePoint & 0x3f));
    } else if (codePoint <= 0xffff) {
      this.hashByte(0xe0 | (codePoint >> 12));
      this.hashByte(0x80 | ((codePoint >> 6) & 0x3f));
      this.hashByte(0x80 | (codePoint & 0x3f));
    } else {
      this.hashByte(0xf0 | (codePoint >> 18));
      this.hashByte(0x80 | ((codePoint >> 12) & 0x3f));
      this.hashByte(0x80 | ((codePoint >> 6) & 0x3f));
      this.hashByte(0x80 | (codePoint & 0x3f));
    }
  }

  private hashByte(byte: number) {
    this.bodyLength += 1;
    this.bodyHash = Math.imul(
      (this.bodyHash ^ byte) >>> 0,
      0x01000193,
    ) >>> 0;
  }
}

async function queryAvailability(command: string, clientSessionId: number) {
  return invoke<ProbeAvailability>(command, { clientSessionId }).catch(() => ({
    accepted: false,
    enabled: false,
  }));
}

function renderedTerminalContains(terminal: Terminal, text: string) {
  const buffer = terminal.buffer.active;
  const firstLine = Math.max(0, buffer.length - terminal.rows - 4);
  for (let index = firstLine; index < buffer.length; index += 1) {
    if (buffer.getLine(index)?.translateToString(true).includes(text)) return true;
  }
  return false;
}
