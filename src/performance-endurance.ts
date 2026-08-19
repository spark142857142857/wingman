import { invoke } from "@tauri-apps/api/core";
import type { Terminal } from "@xterm/xterm";

const durationMilliseconds = 30 * 60_000;
const settleMilliseconds = 10_000;
const readinessTimeoutMilliseconds = 30_000;
const outputCommand =
  "1..250 | ForEach-Object { [Console]::Out.WriteLine(('wingman-endurance-{0:D4}' -f $_)) }\r";
const followReadyMarker = "__WINGMAN_ENDURANCE_FOLLOW_READY__";

type EndurancePhase = "baseline" | "cycle" | "complete" | "failed";

export type EnduranceHost = {
  terminal: Terminal;
  activeSessionId: () => number;
  familiarEnabled: () => boolean;
  processInput: (data: string, clientSessionId: number) => Promise<void>;
  restartPowerShell: () => Promise<void>;
};

export class EnduranceProbe {
  private active = true;
  private outputTail = "";

  constructor(private readonly host: EnduranceHost) {}

  write(data: string) {
    if (this.active) this.outputTail = (this.outputTail + data).slice(-8192);
  }

  async run(initialSessionId: number) {
    let cycle = 0;
    try {
      await delay(settleMilliseconds);
      if (initialSessionId !== this.host.activeSessionId()) {
        throw new Error("initial endurance session changed during settle");
      }
      await this.mark(initialSessionId, "baseline", 0);
      await delay(5_000);
      const startedAt = performance.now();

      while (performance.now() - startedAt < durationMilliseconds) {
        let clientSessionId = this.host.activeSessionId();
        if (!this.host.familiarEnabled()) {
          await this.send(clientSessionId, "familiar on\r");
          await this.waitForEditor(clientSessionId, "familiar on");
        }

        await this.send(clientSessionId, outputCommand);
        await this.waitForEditor(clientSessionId, "bounded output");
        await this.send(clientSessionId, "clear\r");
        await this.waitForEditor(clientSessionId, "clear");

        this.outputTail = "";
        await this.send(clientSessionId, "tail -f wingman-endurance.txt\r");
        await this.waitForOutput(clientSessionId, followReadyMarker, "tail startup");
        await this.send(clientSessionId, "\u0003");
        await this.waitForEditor(clientSessionId, "tail cancellation");

        const columns = cycle % 2 === 0 ? 100 : 112;
        const rows = cycle % 2 === 0 ? 28 : 36;
        this.host.terminal.resize(columns, rows);
        await invoke("resize_shell", {
          clientSessionId,
          cols: columns,
          rows,
        });

        await this.host.restartPowerShell();
        clientSessionId = this.host.activeSessionId();
        await this.waitForEditor(clientSessionId, "session restart");
        cycle += 1;
        await this.mark(clientSessionId, "cycle", cycle);
        await delay(1_000);
      }

      await delay(settleMilliseconds);
      await this.waitForEditor(this.host.activeSessionId(), "final settle");
      await this.mark(this.host.activeSessionId(), "complete", cycle);
    } catch (error) {
      console.error("Endurance probe failed", error);
      try {
        await this.mark(this.host.activeSessionId(), "failed", cycle);
      } catch (markError) {
        console.error("Endurance failure marker failed", markError);
      }
    } finally {
      this.active = false;
    }
  }

  private async send(clientSessionId: number, data: string) {
    await this.host.processInput(data, clientSessionId);
    if (clientSessionId !== this.host.activeSessionId()) {
      throw new Error("endurance input crossed a session generation");
    }
  }

  private async waitForEditor(clientSessionId: number, stage: string) {
    const deadline = performance.now() + readinessTimeoutMilliseconds;
    while (
      clientSessionId === this.host.activeSessionId() &&
      performance.now() < deadline
    ) {
      const state = await invoke<{ accepted: boolean; editorReady: boolean }>(
        "poll_shell_readiness",
        { clientSessionId },
      );
      if (!state.accepted) {
        throw new Error(`endurance session became stale after ${stage}`);
      }
      if (state.editorReady) return;
      await delay(25);
    }
    throw new Error(`endurance editor readiness timed out after ${stage}`);
  }

  private async waitForOutput(
    clientSessionId: number,
    marker: string,
    stage: string,
  ) {
    const deadline = performance.now() + readinessTimeoutMilliseconds;
    while (
      clientSessionId === this.host.activeSessionId() &&
      performance.now() < deadline
    ) {
      if (this.outputTail.includes(marker)) return;
      await delay(25);
    }
    throw new Error(`endurance output timed out during ${stage}`);
  }

  private async mark(
    clientSessionId: number,
    phase: EndurancePhase,
    cycle: number,
  ) {
    const accepted = await invoke<boolean>("mark_performance_endurance", {
      clientSessionId,
      phase,
      cycle,
    });
    if (!accepted) throw new Error(`endurance marker ${phase} was rejected`);
  }
}

function delay(milliseconds: number) {
  return new Promise<void>((resolve) => window.setTimeout(resolve, milliseconds));
}
