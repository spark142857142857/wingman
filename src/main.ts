import "@xterm/xterm/css/xterm.css";
import "./styles.css";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type ShellKind = "powershell" | "cmd";

const termHost = document.getElementById("terminal")!;
const cwdEl = document.getElementById("cwd")!;
const shellLabel = document.getElementById("shellLabel")!;
const compatLabel = document.getElementById("compatLabel")!;
const compatToggle = document.getElementById("compatToggle") as HTMLInputElement;
const shellButtons = Array.from(document.querySelectorAll<HTMLButtonElement>("[data-shell]"));

const term = new Terminal({
  convertEol: true,
  cursorBlink: true,
  fontFamily: "Cascadia Code, Consolas, Courier New, monospace",
  fontSize: 13,
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
term.open(termHost);
fitAddon.fit();

let currentShell: ShellKind = "powershell";
let compat = true;
let inputBuffer = "";

function updateStatus(cwd?: string) {
  shellLabel.textContent = currentShell === "powershell" ? "PowerShell" : "cmd";
  compatLabel.textContent = compat ? "ON" : "OFF";
  compatLabel.className = compat ? "ok" : "off";
  if (cwd) cwdEl.textContent = cwd;
}

function setShellButtons() {
  for (const btn of shellButtons) {
    btn.classList.toggle("active", btn.dataset.shell === currentShell);
  }
}

async function refreshCwd() {
  try {
    const cwd = await invoke<string>("get_cwd");
    updateStatus(cwd);
  } catch {
    // ignore transient startup races
  }
}

function mapLinuxCommand(line: string, shell: ShellKind): string | null {
  const trimmed = line.trim();
  if (!trimmed) return null;

  const parts = trimmed.split(/\s+/);
  const cmd = parts[0];
  const args = parts.slice(1);
  const q = (s: string) => (/\s/.test(s) ? `"${s.replaceAll('"', '\\"')}"` : s);
  const joined = args.map(q).join(" ");

  if (shell === "powershell") {
    switch (cmd) {
      case "ls":
        return args.length ? `Get-ChildItem ${joined}` : "Get-ChildItem";
      case "ll":
        return args.length
          ? `Get-ChildItem ${joined} | Format-Table Mode, Length, LastWriteTime, Name`
          : "Get-ChildItem | Format-Table Mode, Length, LastWriteTime, Name";
      case "pwd":
        return "Get-Location";
      case "clear":
        return "Clear-Host";
      case "cat":
        return args.length ? `Get-Content ${joined}` : "Write-Error 'cat: missing file'";
      case "rm":
        return args.length ? `Remove-Item -Force ${joined}` : "Write-Error 'rm: missing target'";
      case "mv":
        return args.length >= 2
          ? `Move-Item ${q(args[0])} ${q(args[1])}`
          : "Write-Error 'mv: need source and dest'";
      case "cp":
        return args.length >= 2
          ? `Copy-Item ${q(args[0])} ${q(args[1])}`
          : "Write-Error 'cp: need source and dest'";
      default:
        return null;
    }
  }

  switch (cmd) {
    case "ls":
    case "ll":
      return args.length ? `dir ${joined}` : "dir";
    case "pwd":
      return "cd";
    case "clear":
      return "cls";
    case "cat":
      return args.length ? `type ${joined}` : "echo cat: missing file";
    case "rm":
      return args.length ? `del /f ${joined}` : "echo rm: missing target";
    case "mv":
      return args.length >= 2 ? `move ${q(args[0])} ${q(args[1])}` : "echo mv: need source and dest";
    case "cp":
      return args.length >= 2 ? `copy ${q(args[0])} ${q(args[1])}` : "echo cp: need source and dest";
    default:
      return null;
  }
}

async function startSession(shell: ShellKind) {
  currentShell = shell;
  setShellButtons();
  updateStatus();
  term.reset();
  inputBuffer = "";
  const { cols, rows } = term;
  await invoke("start_shell", { shell, cols, rows });
  await refreshCwd();
  term.focus();
}

await listen<string>("pty-output", (event) => {
  term.write(event.payload);
});

await listen<string>("cwd-changed", (event) => {
  updateStatus(event.payload);
});

term.onData(async (data) => {
  for (const ch of data) {
    if (ch === "\r") {
      const line = inputBuffer;
      inputBuffer = "";

      if (compat) {
        const mapped = mapLinuxCommand(line, currentShell);
        if (mapped !== null) {
          // Erase the raw Linux-style command the user typed, then send the mapped Windows command.
          const erase = "\u007f".repeat(line.length);
          await invoke("write_shell", { data: `${erase}${mapped}\r` });
          void refreshCwd();
          continue;
        }
      }

      await invoke("write_shell", { data: "\r" });
      void refreshCwd();
      continue;
    }

    if (ch === "\u007f") {
      inputBuffer = inputBuffer.slice(0, -1);
      await invoke("write_shell", { data: ch });
      continue;
    }

    if (ch === "\u0003") {
      inputBuffer = "";
      await invoke("write_shell", { data: ch });
      continue;
    }

    if (ch >= " " || ch === "\t") {
      inputBuffer += ch;
    }
    await invoke("write_shell", { data: ch });
  }
});

for (const btn of shellButtons) {
  btn.addEventListener("click", async () => {
    const shell = btn.dataset.shell as ShellKind;
    if (shell !== currentShell) {
      await startSession(shell);
    } else {
      term.focus();
    }
  });
}

compatToggle.addEventListener("change", () => {
  compat = compatToggle.checked;
  updateStatus();
  term.focus();
});

window.addEventListener("resize", () => {
  fitAddon.fit();
  const { cols, rows } = term;
  void invoke("resize_shell", { cols, rows });
});

document.addEventListener("keydown", (e) => {
  if (e.ctrlKey && e.shiftKey && e.key.toLowerCase() === "c") {
    e.preventDefault();
    const sel = term.getSelection();
    if (sel) void navigator.clipboard.writeText(sel);
  }
  if (e.ctrlKey && e.shiftKey && e.key.toLowerCase() === "v") {
    e.preventDefault();
    void navigator.clipboard.readText().then((text) => {
      if (text) void invoke("write_shell", { data: text });
    });
  }
});

updateStatus(await invoke<string>("get_cwd").catch(() => "D:\\"));
setShellButtons();
await startSession("powershell");

