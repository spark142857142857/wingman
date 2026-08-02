export type ShellKind = "powershell" | "cmd";
export type CompatCommand = "on" | "off" | "status";
export type ShellTransition = "enter" | "leave" | "root-exit";

export class ShellState {
  private stack: ShellKind[] = ["powershell"];

  get current(): ShellKind {
    return this.stack.at(-1)!;
  }

  setCurrent(shell: ShellKind) {
    this.stack = [shell];
  }

  enter(shell: ShellKind) {
    this.stack.push(shell);
  }

  leave(): ShellKind | null {
    if (this.stack.length === 1) return null;
    this.stack.pop();
    return this.current;
  }
}

export function parseShellCommand(line: string): ShellKind | null {
  const command = line.trim().toLowerCase();
  if (command === "cmd" || command === "cmd.exe") return "cmd";
  if (["powershell", "powershell.exe", "pwsh", "pwsh.exe"].includes(command)) return "powershell";
  return null;
}

export function applyShellTransition(
  line: string,
  state: ShellState,
): ShellTransition | null {
  const shell = parseShellCommand(line);
  if (shell !== null) {
    state.enter(shell);
    return "enter";
  }

  if (line.trim().toLowerCase() !== "exit") return null;
  return state.leave() === null ? "root-exit" : "leave";
}

export function parseCompatCommand(line: string): CompatCommand | null {
  const match = line.trim().toLowerCase().match(/^(?:familiar|fam|compat)\s+(on|off|status)$/);
  return (match?.[1] as CompatCommand | undefined) ?? null;
}

export function familiarStatusCommand(shell: ShellKind, enabled: boolean): string {
  const status = `Familiar: ${enabled ? "ON" : "OFF"}`;
  return shell === "powershell" ? `Write-Output '${status}'` : `echo ${status}`;
}

export function mapCmdCompatCommand(line: string): string | null {
  const trimmed = line.trim();
  if (!trimmed) return null;

  const pipeline = splitCmdPipeline(trimmed);
  if (pipeline === null) return null;

  let changed = false;
  const mappedSegments = pipeline.map((segment, index) => {
    const { command, redirection } = splitCmdRedirection(segment);
    const hasInput = index > 0 || /^(?:0)?</.test(redirection);
    const mapped = mapCmdSimpleCompatCommand(command, hasInput);
    if (mapped === null) return segment.trim();
    changed = true;
    return redirection ? `${mapped} ${redirection}` : mapped;
  });

  return changed ? mappedSegments.join(" | ") : null;
}

function mapCmdSimpleCompatCommand(line: string, hasPipelineInput: boolean): string | null {
  const trimmed = line.trim();
  if (!trimmed) return null;

  const parts = tokenizeCommandLine(trimmed);
  if (parts.length === 0) return null;
  const command = parts[0].toLowerCase();
  const args = parts.slice(1);
  const error = (message: string) => `echo wingman ${command}: ${message}`;

  switch (command) {
    case "ls":
    case "ll": {
      const parsed = parseFlags(args, new Set(["a", "l", "h"]));
      if (parsed.error) return error(parsed.error);
      const switches = parsed.flags.has("a") ? ["/a"] : [];
      return ["dir", ...switches, ...parsed.values.map(quoteCmdArgument)].join(" ");
    }
    case "pwd":
      return args.length === 0 ? "cd" : error("pwd does not accept arguments");
    case "clear":
      return args.length === 0 ? "cls" : error("clear does not accept arguments");
    case "mkdir": {
      const parsed = parseFlags(args, new Set(["p"]));
      if (parsed.error) return error(parsed.error);
      return parsed.values.length > 0
        ? `mkdir ${parsed.values.map(quoteCmdArgument).join(" ")}`
        : error("missing directory");
    }
    case "touch": {
      if (args.length === 0) return error("missing file");
      if (args.some((argument) => argument.startsWith("-"))) return error("options are not supported");
      return args
        .map((path) => {
          const quoted = quoteCmdArgument(path);
          return `if exist ${quoted} (copy /b ${quoted} +,, >nul) else (type nul > ${quoted})`;
        })
        .join(" & ");
    }
    case "which":
      return args.length > 0
        ? `where ${args.map(quoteCmdArgument).join(" ")}`
        : error("missing command");
    case "grep": {
      const parsed = parseFlags(args, new Set(["i", "n", "v", "F"]));
      if (parsed.error) return error(parsed.error);
      if (parsed.values.length < 1 || (!hasPipelineInput && parsed.values.length < 2)) {
        return error(hasPipelineInput ? "missing pattern" : "need pattern and file");
      }
      const switches = [];
      if (parsed.flags.has("i")) switches.push("/i");
      if (parsed.flags.has("n")) switches.push("/n");
      if (parsed.flags.has("v")) switches.push("/v");
      if (parsed.flags.has("F")) switches.push("/l");
      const [pattern, ...paths] = parsed.values;
      return [
        "findstr",
        ...switches,
        `/c:${quoteCmdArgument(pattern)}`,
        ...paths.map(quoteCmdArgument),
      ].join(" ");
    }
    case "head": {
      const parsed = parseLineCountArguments(args, false);
      if (parsed.error) return error(parsed.error);
      if (!hasPipelineInput && parsed.paths.length === 0) return error("missing file or pipeline input");
      const input = parsed.paths.length > 0
        ? `Get-Content -LiteralPath @(${parsed.paths.map(quotePowerShellLiteral).join(",")})`
        : "$input";
      return `powershell.exe -NoLogo -NoProfile -Command "${input} | Select-Object -First ${parsed.count}"`;
    }
    case "tail": {
      const parsed = parseLineCountArguments(args, true);
      if (parsed.error) return error(parsed.error);
      if (parsed.follow && parsed.paths.length === 0) return error("-f requires a file");
      if (!hasPipelineInput && parsed.paths.length === 0) return error("missing file or pipeline input");
      if (parsed.follow) {
        return `powershell.exe -NoLogo -NoProfile -Command "Get-Content -LiteralPath @(${parsed.paths.map(quotePowerShellLiteral).join(",")}) -Tail ${parsed.count} -Wait"`;
      }
      const input = parsed.paths.length > 0
        ? `Get-Content -LiteralPath @(${parsed.paths.map(quotePowerShellLiteral).join(",")})`
        : "$input";
      return `powershell.exe -NoLogo -NoProfile -Command "${input} | Select-Object -Last ${parsed.count}"`;
    }
    case "sort": {
      const parsed = parseFlags(args, new Set(["r", "n", "u"]));
      if (parsed.error) return error(parsed.error);
      const input = parsed.values.length > 0
        ? `Get-Content -LiteralPath @(${parsed.values.map(quotePowerShellLiteral).join(",")})`
        : "$input";
      if (parsed.values.length === 0 && !hasPipelineInput) return "sort";
      if (!parsed.flags.has("n") && !parsed.flags.has("u")) {
        return ["sort", parsed.flags.has("r") ? "/r" : "", ...parsed.values.map(quoteCmdArgument)]
          .filter(Boolean)
          .join(" ");
      }
      const numeric = parsed.flags.has("n") ? " { [double]$_ }" : "";
      const descending = parsed.flags.has("r") ? " -Descending" : "";
      const unique = parsed.flags.has("u") ? " -Unique" : "";
      return `powershell.exe -NoLogo -NoProfile -Command "${input} | Sort-Object${numeric}${descending}${unique}"`;
    }
    case "wc": {
      const parsed = parseFlags(args, new Set(["l"]));
      if (parsed.error) return error(parsed.error);
      if (!parsed.flags.has("l")) return error("only -l is supported in cmd mode");
      if (!hasPipelineInput && parsed.values.length === 0) return error("missing file or pipeline input");
      return ["find", "/c", "/v", '""', ...parsed.values.map(quoteCmdArgument)].join(" ");
    }
    case "cat": {
      const parsed = parseFlags(args, new Set(["n"]));
      if (parsed.error) return error(parsed.error);
      if (parsed.values.length === 0) return error("missing file");
      return parsed.flags.has("n")
        ? `findstr /n "^" ${parsed.values.map(quoteCmdArgument).join(" ")}`
        : `type ${parsed.values.map(quoteCmdArgument).join(" ")}`;
    }
    case "rm": {
      const parsed = parseFlags(args, new Set(["r", "R", "f"]));
      if (parsed.error) return error(parsed.error);
      if (parsed.values.length === 0) return error("missing target");
      const recursive = parsed.flags.has("r") || parsed.flags.has("R");
      if (!recursive) return `del /f /q ${parsed.values.map(quoteCmdArgument).join(" ")}`;
      return parsed.values
        .map((path) => {
          const quoted = quoteCmdArgument(path);
          return `if exist ${quoted}\\NUL (rmdir /s /q ${quoted}) else (del /f /q ${quoted})`;
        })
        .join(" & ");
    }
    case "mv": {
      if (args.some((argument) => argument.startsWith("-"))) return error("options are not supported");
      return args.length === 2
        ? `move ${quoteCmdArgument(args[0])} ${quoteCmdArgument(args[1])}`
        : error("need source and destination");
    }
    case "cp": {
      const parsed = parseFlags(args, new Set(["r", "R", "f"]));
      if (parsed.error) return error(parsed.error);
      if (parsed.values.length !== 2) return error("need source and destination");
      const [source, destination] = parsed.values.map(quoteCmdArgument);
      return parsed.flags.has("r") || parsed.flags.has("R")
        ? `xcopy /e /i /y ${source} ${destination}`
        : `copy /y ${source} ${destination}`;
    }
    default:
      return null;
  }
}

function splitCmdPipeline(line: string): string[] | null {
  const segments: string[] = [];
  let current = "";
  let quoted = false;
  let escaped = false;

  for (let index = 0; index < line.length; index++) {
    const character = line[index];
    if (escaped) {
      current += character;
      escaped = false;
      continue;
    }
    if (character === "^") {
      current += character;
      escaped = true;
      continue;
    }
    if (character === '"') {
      quoted = !quoted;
      current += character;
      continue;
    }
    if (!quoted && (character === "&" || (character === "|" && line[index + 1] === "|"))) {
      return null;
    }
    if (!quoted && character === "|") {
      if (!current.trim()) return null;
      segments.push(current.trim());
      current = "";
      continue;
    }
    current += character;
  }

  if (quoted || escaped || !current.trim()) return null;
  segments.push(current.trim());
  return segments;
}

function splitCmdRedirection(segment: string) {
  let quoted = false;
  let escaped = false;
  for (let index = 0; index < segment.length; index++) {
    const character = segment[index];
    if (escaped) {
      escaped = false;
      continue;
    }
    if (character === "^") {
      escaped = true;
      continue;
    }
    if (character === '"') {
      quoted = !quoted;
      continue;
    }
    if (!quoted && (character === ">" || character === "<")) {
      let start = index;
      if (
        index > 0
        && /[0-9]/.test(segment[index - 1])
        && (index === 1 || /\s/.test(segment[index - 2]))
      ) {
        start--;
      }
      return {
        command: segment.slice(0, start).trim(),
        redirection: segment.slice(start).trim(),
      };
    }
  }
  return { command: segment.trim(), redirection: "" };
}

function parseLineCountArguments(args: string[], allowFollow: boolean) {
  let count = 10;
  let follow = false;
  const paths: string[] = [];

  for (let index = 0; index < args.length; index++) {
    const argument = args[index];
    if (argument === "-n") {
      const value = args[++index];
      if (value === undefined || !/^\d+$/.test(value)) {
        return { count, follow, paths, error: "-n requires a non-negative number" };
      }
      count = Number(value);
    } else if (/^-\d+$/.test(argument)) {
      count = Number(argument.slice(1));
    } else if (allowFollow && argument === "-f") {
      follow = true;
    } else if (argument.startsWith("-")) {
      return { count, follow, paths, error: `unsupported option ${argument}` };
    } else {
      paths.push(argument);
    }
  }

  return { count, follow, paths, error: null as string | null };
}

function tokenizeCommandLine(line: string): string[] {
  const values: string[] = [];
  const pattern = /"([^"]*)"|(\S+)/g;
  for (const match of line.matchAll(pattern)) {
    values.push(match[1] ?? match[2]);
  }
  return values;
}

function quoteCmdArgument(value: string): string {
  if (!/[\s&|<>^]/.test(value)) return value;
  return `"${value.replaceAll('"', '""')}"`;
}

function quotePowerShellLiteral(value: string): string {
  return `'${value.replaceAll("'", "''")}'`;
}

function parseFlags(args: string[], supported: Set<string>) {
  const flags = new Set<string>();
  const values: string[] = [];
  let options = true;

  for (const argument of args) {
    if (options && argument === "--") {
      options = false;
      continue;
    }
    if (options && argument.startsWith("-") && argument.length > 1) {
      for (const flag of argument.slice(1)) {
        if (!supported.has(flag)) return { flags, values, error: `unsupported option -${flag}` };
        flags.add(flag);
      }
      continue;
    }
    values.push(argument);
  }

  return { flags, values, error: null as string | null };
}
