import "@xterm/xterm/css/xterm.css";
import "./styles.css";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { TerminalInputParser } from "./terminal-input";
import { isPasteShortcut } from "./terminal-shortcuts";
import {
  applyShellTransition,
  familiarStatusCommand,
  mapCmdCompatCommand,
  parseCompatCommand,
  ShellState,
  type ShellKind,
} from "./shell-state";

const termHost = document.getElementById("terminal")!;
const cwdEl = document.getElementById("cwd")!;
const shellLabel = document.getElementById("shellLabel")!;
const familiarLabel = document.getElementById("familiarLabel")!;

const preferences = {
  compat: "wingman.compat",
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

let compat = localStorage.getItem(preferences.compat) !== "false";
const inputParser = new TerminalInputParser();
const shellState = new ShellState();
let inputQueue = Promise.resolve();
let activeSessionId = 0;

function updateStatus(cwd?: string) {
  shellLabel.textContent = shellState.current === "powershell" ? "PowerShell" : "cmd";
  familiarLabel.textContent = compat ? "ON" : "OFF";
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
  inputParser.reset();
  const { cols, rows } = term;
  await invoke("start_shell", { shell, cols, rows, compat, clientSessionId: sessionId });
  shellState.setCurrent(shell);
  updateStatus();
  await refreshCwd();
  term.focus();
}

await listen<{ session_id: number; data: string }>("pty-output", (event) => {
  if (event.payload.session_id === activeSessionId) {
    term.write(event.payload.data);
  }
});

await listen<string>("cwd-changed", (event) => {
  updateStatus(event.payload);
});

async function processTerminalData(data: string) {
  const actions = inputParser.consume(data);
  for (const action of actions) {
    if (action.type === "submit") {
      const line = action.line;
      const shellTransition = action.reliable
        ? applyShellTransition(line, shellState)
        : null;
      const compatCommand = action.reliable ? parseCompatCommand(line) : null;

      if (shellTransition === "enter" || shellTransition === "leave") {
        await invoke("write_shell", { data: "\r" });
        inputParser.commitSubmittedLine(line);
        updateStatus();
        void refreshCwd();
        continue;
      }

      if (compatCommand !== null) {
        if (compatCommand !== "status") {
          compat = compatCommand === "on";
          localStorage.setItem(preferences.compat, String(compat));
          await invoke("set_compat", { enabled: compat });
          updateStatus();
        }

        const erase = "\u007f".repeat(line.length);
        const statusCommand = familiarStatusCommand(shellState.current, compat);
        await invoke("write_shell", {
          data: `${erase}${statusCommand}\r`,
        });
        inputParser.commitSubmittedLine(statusCommand);
        continue;
      }

      if (action.reliable && compat && shellState.current === "cmd") {
        const mapped = mapCmdCompatCommand(line);
        if (mapped !== null) {
          // Erase the raw Linux-style command the user typed, then send the mapped Windows command.
          const erase = "\u007f".repeat(line.length);
          await invoke("write_shell", { data: `${erase}${mapped}\r` });
          inputParser.commitSubmittedLine(mapped);
          void refreshCwd();
          continue;
        }
      }

      await invoke("write_shell", { data: "\r" });
      inputParser.commitSubmittedLine(action.reliable ? line : null);
      void refreshCwd();
      continue;
    }
    await invoke("write_shell", { data: action.data });
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
  enqueueInputTask(() => processTerminalData(data));
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
      void invoke("resize_shell", { cols, rows });
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
  const text = await navigator.clipboard.readText();
  if (text) enqueueTerminalData(text.replace(/\r\n|\n/g, "\r"));
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
    enqueueInputTask(() => startSession(shellState.current));
  }
});

updateStatus(await invoke<string>("get_cwd").catch(() => "D:\\"));
await startSession("powershell");
