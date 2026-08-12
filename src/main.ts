import "@xterm/xterm/css/xterm.css";
import "./styles.css";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { classifyTerminalPaste } from "./terminal-paste";
import { isPasteShortcut } from "./terminal-shortcuts";

type ShellKind = "powershell" | "cmd";

const termHost = document.getElementById("terminal")!;
const cwdEl = document.getElementById("cwd")!;
const shellLabel = document.getElementById("shellLabel")!;
const familiarLabel = document.getElementById("familiarLabel")!;

const preferences = {
  fontSize: "wingman.fontSize.v2",
};

function savedFontSize() {
  const saved = localStorage.getItem(preferences.fontSize);
  if (saved === null) return 17;
  const value = Number(saved);
  return Number.isFinite(value) ? Math.min(24, Math.max(12, value)) : 17;
}

const term = new Terminal({
  convertEol: true,
  cursorBlink: true,
  fontFamily: "Cascadia Mono, Consolas, monospace",
  fontSize: savedFontSize(),
  fontWeight: "500",
  fontWeightBold: "700",
  lineHeight: 1.25,
  theme: {
    background: "#00000000",
    foreground: "#dceaff",
    cursor: "#8ef0c5",
    selectionBackground: "#6ec1ff55",
    black: "#0b1622",
    red: "#ff7b72",
    green: "#8ef0c5",
    yellow: "#ffd37a",
    blue: "#6ec1ff",
    magenta: "#d2a8ff",
    cyan: "#76e4f7",
    white: "#e8f2ff"
  }
});

const fitAddon = new FitAddon();
term.loadAddon(fitAddon);
term.loadAddon(new WebLinksAddon());
term.open(termHost);
fitAddon.fit();

let compat = false;
let activeShell: ShellKind = "powershell";
let inputQueue = Promise.resolve();
let activeSessionId = 0;

const shellReadinessTimeoutMs = 30_000;
const performanceInputEchoToken = "__WINGMAN_INPUT_ECHO_PROBE__";
const performanceInputEchoCommand = `# ${performanceInputEchoToken}\r`;
const performanceBulkStart = "__WINGMAN_BULK_START__\r\n";
const performanceBulkEnd = "__WINGMAN_BULK_END__\r\n";
const performanceBulkExpectedLength = 11_900_000;
const performanceBulkExpectedHash = 0x48bac225;
const performanceBulkCommand =
  "$p=([char]0xe9).ToString()*55;" +
  "$s='__WINGMAN_BULK_'+'START__';$e='__WINGMAN_BULK_'+'END__';" +
  '[Console]::Out.Write($s+"`r`n");' +
  'for($i=0;$i -lt 100000;$i++){[Console]::Out.Write(("{0:D6}:{1}`r`n" -f $i,$p))};' +
  '[Console]::Out.Write($e+"`r`n")\r';

type PerformanceInputEchoProbe = {
  sessionId: number;
  observed: boolean;
};

type PerformanceBulkOutputProbe = {
  sessionId: number;
  state: "waiting-start" | "body" | "complete" | "invalid";
  carry: string;
  bodyLength: number;
  bodyHash: number;
  vtState: "ground" | "escape" | "csi" | "string" | "string-escape";
  observed: boolean;
};

let performanceInputEchoProbe: PerformanceInputEchoProbe | null = null;
let performanceBulkOutputProbe: PerformanceBulkOutputProbe | null = null;

function delay(milliseconds: number) {
  return new Promise<void>((resolve) => window.setTimeout(resolve, milliseconds));
}

async function observeEditorReadiness(clientSessionId: number) {
  const deadline = performance.now() + shellReadinessTimeoutMs;
  while (clientSessionId === activeSessionId && performance.now() < deadline) {
    const state = await invoke<{ accepted: boolean; editorReady: boolean }>(
      "poll_shell_readiness",
      { clientSessionId },
    ).catch(() => ({ accepted: false, editorReady: false }));
    if (!state.accepted || clientSessionId !== activeSessionId) return;
    if (state.editorReady) {
      const bulkProbe = await invoke<{ accepted: boolean; enabled: boolean }>(
        "performance_bulk_output_probe",
        { clientSessionId },
      ).catch(() => ({ accepted: false, enabled: false }));
      if (
        bulkProbe.accepted &&
        bulkProbe.enabled &&
        clientSessionId === activeSessionId
      ) {
        performanceBulkOutputProbe = {
          sessionId: clientSessionId,
          state: "waiting-start",
          carry: "",
          bodyLength: 0,
          bodyHash: 0x811c9dc5,
          vtState: "ground",
          observed: false,
        };
        term.input(performanceBulkCommand, true);
        return;
      }
      const probe = await invoke<{ accepted: boolean; enabled: boolean }>(
        "performance_input_echo_probe",
        { clientSessionId },
      ).catch(() => ({ accepted: false, enabled: false }));
      if (probe.accepted && probe.enabled && clientSessionId === activeSessionId) {
        performanceInputEchoProbe = {
          sessionId: clientSessionId,
          observed: false,
        };
        term.input(performanceInputEchoCommand, true);
      }
      return;
    }
    await delay(25);
  }
}

function updateStatus(cwd?: string) {
  shellLabel.textContent = activeShell === "powershell" ? "PowerShell" : "cmd";
  familiarLabel.textContent = compat ? "ON" : "PAUSED";
  familiarLabel.className = compat ? "ok" : "off";
  if (cwd) cwdEl.textContent = cwd;
}

async function refreshCwd() {
  try {
    const cwd = await invoke<string>("get_cwd");
    updateStatus(cwd);
  } catch {
    // ignore transient startup races
  }
}

async function startSession(shell: ShellKind) {
  const sessionId = ++activeSessionId;
  term.reset();
  const { cols, rows } = term;
  const session = await invoke<{ shell: string; cwd: string }>("start_shell", {
    shell,
    cols,
    rows,
    compat,
    clientSessionId: sessionId,
  });
  activeShell = shell;
  updateStatus(session.cwd);
  term.focus();
  if (shell === "powershell") void observeEditorReadiness(sessionId);
}

await listen<{ session_id: number; data: string }>("pty-output", (event) => {
  if (event.payload.session_id === activeSessionId) {
    const bulkProbe = performanceBulkOutputProbe;
    if (
      bulkProbe &&
      bulkProbe.sessionId === activeSessionId &&
      !bulkProbe.observed
    ) {
      observePerformanceBulkOutput(bulkProbe, event.payload.data);
      term.write(event.payload.data, () => {
        requestAnimationFrame(() => {
          requestAnimationFrame(() => {
            if (
              !bulkProbe.observed &&
              bulkProbe.sessionId === activeSessionId &&
              bulkProbe.state === "complete" &&
              renderedTerminalContains(performanceBulkEnd.trim())
            ) {
              bulkProbe.observed = true;
              void invoke("mark_performance_bulk_output", {
                clientSessionId: bulkProbe.sessionId,
              });
            }
          });
        });
      });
      return;
    }
    const probe = performanceInputEchoProbe;
    if (probe && probe.sessionId === activeSessionId && !probe.observed) {
      term.write(event.payload.data, () => {
        requestAnimationFrame(() => {
          requestAnimationFrame(() => {
            if (
              !probe.observed &&
              probe.sessionId === activeSessionId &&
              renderedTerminalContains(performanceInputEchoToken)
            ) {
              probe.observed = true;
              void invoke("mark_performance_input_echo", {
                clientSessionId: probe.sessionId,
              });
            }
          });
        });
      });
      return;
    }
    term.write(event.payload.data);
  }
});

function observePerformanceBulkOutput(
  probe: PerformanceBulkOutputProbe,
  data: string,
) {
  if (probe.state === "complete" || probe.state === "invalid") return;

  let input = probe.carry + data;
  probe.carry = "";
  if (probe.state === "waiting-start") {
    const markerIndex = input.indexOf(performanceBulkStart);
    if (markerIndex < 0) {
      probe.carry = input.slice(-(performanceBulkStart.length - 1));
      return;
    }
    input = input.slice(markerIndex + performanceBulkStart.length);
    probe.state = "body";
  }

  const endIndex = input.indexOf(performanceBulkEnd);
  if (endIndex >= 0) {
    hashPerformanceBulkBody(probe, input.slice(0, endIndex));
    probe.carry = "";
    probe.state =
      probe.bodyLength === performanceBulkExpectedLength &&
      probe.bodyHash === performanceBulkExpectedHash &&
      probe.vtState === "ground"
        ? "complete"
        : "invalid";
    return;
  }

  const retainedLength = Math.min(input.length, performanceBulkEnd.length - 1);
  const bodyEnd = input.length - retainedLength;
  hashPerformanceBulkBody(probe, input.slice(0, bodyEnd));
  probe.carry = input.slice(bodyEnd);
}

function hashPerformanceBulkBody(
  probe: PerformanceBulkOutputProbe,
  body: string,
) {
  for (let index = 0; index < body.length; index += 1) {
    const code = body.charCodeAt(index);
    if (probe.vtState === "ground") {
      if (code === 0x1b) {
        probe.vtState = "escape";
      } else {
        const codePoint = body.codePointAt(index)!;
        hashPerformanceBulkCodePoint(probe, codePoint);
        if (codePoint > 0xffff) index += 1;
      }
      continue;
    }
    if (probe.vtState === "escape") {
      if (body[index] === "[") {
        probe.vtState = "csi";
      } else if (body[index] === "]" || body[index] === "P") {
        probe.vtState = "string";
      } else {
        probe.vtState = "ground";
      }
      continue;
    }
    if (probe.vtState === "csi") {
      if (code >= 0x40 && code <= 0x7e) probe.vtState = "ground";
      continue;
    }
    if (probe.vtState === "string") {
      if (code === 0x07) {
        probe.vtState = "ground";
      } else if (code === 0x1b) {
        probe.vtState = "string-escape";
      }
      continue;
    }
    if (body[index] === "\\") {
      probe.vtState = "ground";
    } else if (code !== 0x1b) {
      probe.vtState = "string";
    }
  }
}

function hashPerformanceBulkCodePoint(
  probe: PerformanceBulkOutputProbe,
  codePoint: number,
) {
  if (codePoint <= 0x7f) {
    hashPerformanceBulkByte(probe, codePoint);
  } else if (codePoint <= 0x7ff) {
    hashPerformanceBulkByte(probe, 0xc0 | (codePoint >> 6));
    hashPerformanceBulkByte(probe, 0x80 | (codePoint & 0x3f));
  } else if (codePoint <= 0xffff) {
    hashPerformanceBulkByte(probe, 0xe0 | (codePoint >> 12));
    hashPerformanceBulkByte(probe, 0x80 | ((codePoint >> 6) & 0x3f));
    hashPerformanceBulkByte(probe, 0x80 | (codePoint & 0x3f));
  } else {
    hashPerformanceBulkByte(probe, 0xf0 | (codePoint >> 18));
    hashPerformanceBulkByte(probe, 0x80 | ((codePoint >> 12) & 0x3f));
    hashPerformanceBulkByte(probe, 0x80 | ((codePoint >> 6) & 0x3f));
    hashPerformanceBulkByte(probe, 0x80 | (codePoint & 0x3f));
  }
}

function hashPerformanceBulkByte(probe: PerformanceBulkOutputProbe, byte: number) {
  probe.bodyLength += 1;
  probe.bodyHash = Math.imul(
    (probe.bodyHash ^ byte) >>> 0,
    0x01000193,
  ) >>> 0;
}

function renderedTerminalContains(text: string) {
  const buffer = term.buffer.active;
  const firstLine = Math.max(0, buffer.length - term.rows - 4);
  for (let index = firstLine; index < buffer.length; index += 1) {
    if (buffer.getLine(index)?.translateToString(true).includes(text)) return true;
  }
  return false;
}

await listen<string>("cwd-changed", (event) => {
  updateStatus(event.payload);
});

async function processTerminalData(data: string, clientSessionId: number) {
  const result = await invoke<{ accepted: boolean; familiarEnabled: boolean }>(
    "handle_terminal_input",
    { clientSessionId, data },
  );
  if (!result.accepted || clientSessionId !== activeSessionId) return;
  if (compat !== result.familiarEnabled) {
    compat = result.familiarEnabled;
    updateStatus();
  }
  if (/[\r\n]/.test(data)) void refreshCwd();
}

function enqueueInputTask(task: () => Promise<void>) {
  inputQueue = inputQueue
    .then(task)
    .catch((error) => {
      console.error("Terminal input failed", error);
    });
}

function enqueueTerminalData(data: string) {
  const clientSessionId = activeSessionId;
  enqueueInputTask(() => processTerminalData(data, clientSessionId));
}

term.onData((data) => enqueueTerminalData(data));

let fitFrame: number | null = null;

function scheduleTerminalFit() {
  if (fitFrame !== null) cancelAnimationFrame(fitFrame);
  fitFrame = requestAnimationFrame(() => {
    fitFrame = requestAnimationFrame(() => {
      fitFrame = null;
      fitAddon.fit();
      const { cols, rows } = term;
      void invoke("resize_shell", { clientSessionId: activeSessionId, cols, rows });
    });
  });
}

function resizeFont(delta: number) {
  const currentSize = term.options.fontSize ?? 17;
  const next = Math.min(24, Math.max(12, currentSize + delta));
  if (next === currentSize) return;
  term.options.fontSize = next;
  localStorage.setItem(preferences.fontSize, String(next));
  scheduleTerminalFit();
  term.focus();
}

async function copySelection() {
  const selection = term.getSelection();
  if (selection) await navigator.clipboard.writeText(selection);
  term.focus();
}

async function pasteClipboard() {
  const clientSessionId = activeSessionId;
  const text = await navigator.clipboard.readText();
  if (!text || clientSessionId !== activeSessionId) {
    term.focus();
    return;
  }

  const paste = classifyTerminalPaste(text);
  if (paste.kind === "line-breaking") {
    const shouldSend = window.confirm(
      "여러 줄을 터미널에 그대로 보낼까요? 포함된 명령이 실행될 수 있습니다.",
    );
    if (shouldSend && clientSessionId === activeSessionId) {
      await invoke("write_native_paste", { clientSessionId, data: paste.data });
    }
  } else {
    enqueueTerminalData(paste.data);
  }
  term.focus();
}

term.attachCustomKeyEventHandler((event) => {
  if (event.type !== "keydown" || !isPasteShortcut(event)) return true;
  event.preventDefault();
  event.stopPropagation();
  void pasteClipboard();
  return false;
});

const terminalResizeObserver = new ResizeObserver(scheduleTerminalFit);
terminalResizeObserver.observe(termHost);

document.addEventListener("keydown", (e) => {
  if (e.ctrlKey && e.shiftKey && e.key.toLowerCase() === "c") {
    e.preventDefault();
    void copySelection();
  }
  if (e.ctrlKey && !e.shiftKey && (e.key === "+" || e.key === "=")) {
    e.preventDefault();
    resizeFont(1);
  }
  if (e.ctrlKey && !e.shiftKey && e.key === "-") {
    e.preventDefault();
    resizeFont(-1);
  }
  if (e.ctrlKey && e.shiftKey && e.key.toLowerCase() === "r") {
    e.preventDefault();
    enqueueInputTask(() => startSession(activeShell));
  }
});

updateStatus();
await startSession("powershell");
