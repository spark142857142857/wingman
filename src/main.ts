import "@xterm/xterm/css/xterm.css";
import "./styles.css";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { classifyTerminalPaste } from "./terminal-paste";
import { blockedTerminalLinkHandler } from "./terminal-security";
import { isPasteShortcut } from "./terminal-shortcuts";
import { PerformanceProbe } from "./performance-probes";
import { TERMINAL_SCROLLBACK_ROWS } from "./terminal-config";

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
  scrollback: TERMINAL_SCROLLBACK_ROWS,
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
  },
  linkHandler: blockedTerminalLinkHandler,
});

const fitAddon = new FitAddon();
term.loadAddon(fitAddon);
term.open(termHost);
fitAddon.fit();

let compat = false;
let activeShell: ShellKind = "powershell";
let inputQueue = Promise.resolve();
let activeSessionId = 0;

const shellReadinessTimeoutMs = 30_000;
const enduranceDurationMs = 30 * 60_000;
const enduranceSettleMs = 10_000;
const enduranceOutputCommand =
  "1..250 | ForEach-Object { [Console]::Out.WriteLine(('wingman-endurance-{0:D4}' -f $_)) }\r";
let performanceProbe: PerformanceProbe | null = null;
let enduranceStarted = false;

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
      const endurance = await invoke<{ accepted: boolean; enabled: boolean }>(
        "performance_endurance_probe",
        { clientSessionId },
      ).catch(() => ({ accepted: false, enabled: false }));
      if (
        endurance.accepted &&
        endurance.enabled &&
        !enduranceStarted &&
        clientSessionId === activeSessionId
      ) {
        enduranceStarted = true;
        void runEnduranceProbe(clientSessionId);
        return;
      }
      performanceProbe = await PerformanceProbe.start(
        clientSessionId,
        term,
        () => clientSessionId === activeSessionId,
      );
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
  performanceProbe = null;
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

await listen<{ session_id: number; sequence: number; data: string }>("pty-output", (event) => {
  const acknowledge = () => {
    void invoke("acknowledge_pty_output", {
      clientSessionId: event.payload.session_id,
      sequence: event.payload.sequence,
    });
  };
  if (event.payload.session_id === activeSessionId) {
    if (performanceProbe?.write(activeSessionId, event.payload.data, acknowledge)) return;
    term.write(event.payload.data, acknowledge);
    return;
  }
  acknowledge();
});

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

async function waitForEditorReady(clientSessionId: number) {
  const deadline = performance.now() + shellReadinessTimeoutMs;
  while (clientSessionId === activeSessionId && performance.now() < deadline) {
    const state = await invoke<{ accepted: boolean; editorReady: boolean }>(
      "poll_shell_readiness",
      { clientSessionId },
    );
    if (!state.accepted) throw new Error("endurance session became stale");
    if (state.editorReady) return;
    await delay(25);
  }
  throw new Error("endurance editor readiness timed out");
}

async function markEndurance(
  clientSessionId: number,
  phase: "baseline" | "cycle" | "complete" | "failed",
  cycle: number,
) {
  const accepted = await invoke<boolean>("mark_performance_endurance", {
    clientSessionId,
    phase,
    cycle,
  });
  if (!accepted) throw new Error(`endurance marker ${phase} was rejected`);
}

async function sendEnduranceInput(clientSessionId: number, data: string) {
  await processTerminalData(data, clientSessionId);
  if (clientSessionId !== activeSessionId) {
    throw new Error("endurance input crossed a session generation");
  }
}

async function runEnduranceProbe(initialSessionId: number) {
  let cycle = 0;
  try {
    await delay(enduranceSettleMs);
    if (initialSessionId !== activeSessionId) {
      throw new Error("initial endurance session changed during settle");
    }
    await markEndurance(initialSessionId, "baseline", 0);
    await delay(5_000);
    const startedAt = performance.now();

    while (performance.now() - startedAt < enduranceDurationMs) {
      let clientSessionId = activeSessionId;
      if (!compat) {
        await sendEnduranceInput(clientSessionId, "familiar on\r");
        await waitForEditorReady(clientSessionId);
      }

      await sendEnduranceInput(clientSessionId, enduranceOutputCommand);
      await waitForEditorReady(clientSessionId);
      await sendEnduranceInput(clientSessionId, "clear\r");
      await waitForEditorReady(clientSessionId);

      await sendEnduranceInput(clientSessionId, "tail -f wingman-endurance.txt\r");
      await delay(350);
      await sendEnduranceInput(clientSessionId, "\u0003");
      await waitForEditorReady(clientSessionId);

      const columns = cycle % 2 === 0 ? 100 : 112;
      const rows = cycle % 2 === 0 ? 28 : 36;
      term.resize(columns, rows);
      await invoke("resize_shell", {
        clientSessionId,
        cols: columns,
        rows,
      });

      await startSession("powershell");
      clientSessionId = activeSessionId;
      await waitForEditorReady(clientSessionId);
      cycle += 1;
      await markEndurance(clientSessionId, "cycle", cycle);
      await delay(1_000);
    }

    await delay(enduranceSettleMs);
    await waitForEditorReady(activeSessionId);
    await markEndurance(activeSessionId, "complete", cycle);
  } catch (error) {
    console.error("Endurance probe failed", error);
    try {
      await markEndurance(activeSessionId, "failed", cycle);
    } catch (markError) {
      console.error("Endurance failure marker failed", markError);
    }
  }
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
