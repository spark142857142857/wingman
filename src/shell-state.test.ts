import { mapCmdCompatCommand, parseCompatCommand, parseShellCommand, ShellState } from "./shell-state.ts";

function assertEqual(label: string, actual: unknown, expected: unknown) {
  if (actual !== expected) {
    throw new Error(`${label}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  }
}

const shells = new ShellState();
assertEqual("default shell", shells.current, "powershell");
assertEqual("cmd switch command", parseShellCommand("cmd"), "cmd");
shells.setCurrent("cmd");
assertEqual("active shell changes to cmd", shells.current, "cmd");
assertEqual("cmd compat ls", mapCmdCompatCommand("ls"), "dir");
assertEqual("cmd compat pwd", mapCmdCompatCommand("pwd"), "cd");
assertEqual("PowerShell switch command", parseShellCommand("powershell"), "powershell");
assertEqual("PowerShell executable switch command", parseShellCommand("pwsh.exe"), "powershell");
assertEqual("shell command with arguments passes through", parseShellCommand("cmd /d"), null);
assertEqual("ordinary exit is owned by active shell", parseShellCommand("exit"), null);

assertEqual("compat on", parseCompatCommand("compat on"), "on");
assertEqual("compat off", parseCompatCommand("compat off"), "off");
assertEqual("linux status alias", parseCompatCommand("linux status"), "status");
assertEqual("ordinary command", parseCompatCommand("echo compat on"), null);
assertEqual("PowerShell cmdlet passes through", mapCmdCompatCommand("Get-Process"), null);
assertEqual("cmd native dir passes through", mapCmdCompatCommand("dir /b"), null);
assertEqual("developer command passes through", mapCmdCompatCommand("git status"), null);
assertEqual("cmd compat cat", mapCmdCompatCommand("cat file.txt"), "type file.txt");
assertEqual("cmd compat grep", mapCmdCompatCommand("grep TODO app.txt"), "findstr /c:TODO app.txt");
assertEqual("cmd compat grep flags", mapCmdCompatCommand("grep -inv TODO app.txt"), "findstr /i /n /v /c:TODO app.txt");
assertEqual("cmd compat ls flags", mapCmdCompatCommand("ls -la"), "dir /a");
assertEqual("cmd compat mkdir parents", mapCmdCompatCommand("mkdir -p demo\\nested"), "mkdir demo\\nested");
assertEqual(
  "cmd compat recursive copy",
  mapCmdCompatCommand('cp -r "source dir" "target dir"'),
  'xcopy /e /i /y "source dir" "target dir"',
);
assertEqual(
  "cmd compat recursive remove",
  mapCmdCompatCommand("rm -rf demo"),
  "if exist demo\\NUL (rmdir /s /q demo) else (del /f /q demo)",
);
assertEqual(
  "unsupported option is explicit",
  mapCmdCompatCommand("ls -z"),
  "echo wingman ls: unsupported option -z",
);
assertEqual(
  "cmd compat text pipeline",
  mapCmdCompatCommand("cat app.txt | grep TODO | head -n 2"),
  'type app.txt | findstr /c:TODO | powershell.exe -NoLogo -NoProfile -Command "$input | Select-Object -First 2"',
);
assertEqual(
  "cmd compat pipeline and redirection",
  mapCmdCompatCommand('cat app.txt | grep TODO > "result file.txt"'),
  'type app.txt | findstr /c:TODO > "result file.txt"',
);
assertEqual(
  "cmd compat line count",
  mapCmdCompatCommand("cat app.txt | wc -l"),
  'type app.txt | find /c /v ""',
);
assertEqual(
  "cmd compat input redirection",
  mapCmdCompatCommand("grep TODO < app.txt"),
  "findstr /c:TODO < app.txt",
);
assertEqual(
  "cmd stderr redirection remains intact",
  mapCmdCompatCommand("cat missing.txt 2> errors.txt"),
  "type missing.txt 2> errors.txt",
);
assertEqual(
  "native conditional passes through",
  mapCmdCompatCommand("echo ok && dir"),
  null,
);

console.log("Shell state tests passed.");
