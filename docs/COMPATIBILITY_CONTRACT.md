# Wingman Compatibility Contract (v0.1)

Status: accepted product direction for the MVP.

## Product role

> **Windows shell. Unix muscle memory.**

Wingman is a session-scoped Unix-command familiarity layer over native Windows
shells. It keeps PowerShell or `cmd` as the process, file-system, permission,
and execution environment; it does not provide a Linux distribution, Bash, or
a POSIX runtime.

The intended user already knows Unix command habits, but must work in a native
Windows environment and does not want a shell switch to interrupt that flow.

## Input contract

With Familiar mode enabled:

1. Supported Unix syntax is translated to an equivalent operation for the
   active shell.
2. Native PowerShell, `cmd`, and Windows management commands pass through
   unchanged.
3. Syntax outside the supported Unix subset is not guessed or partially
   translated. The active shell receives the unrecognised command, or Wingman
   returns an unsupported-syntax error when it has already claimed the command.

These rules apply only to a reliably mirrored single line at a validated shell
prompt. Completion, multiline paste, foreground interactive input, and unknown
editing follow the conservative fallback in the [terminal submission and
session contract](TERMINAL_SESSION_CONTRACT.md).

With Familiar mode disabled, all command input passes through without Wingman
interpretation. Session isolation and the confirmation for a paste containing
a line break still apply.

Names shared by Windows and Unix tools, such as `find` or `sort`, use the
Unix-compatible Wingman meaning only while Familiar mode is enabled. Turn the
mode off to use the original shell meaning.

### P0 shell availability

The current production cutover keeps Familiar default-paused. Windows
PowerShell 5.1 now has a separately reviewed OOB editor-readiness channel, while
PTY output has no readiness authority; `cmd.exe` remains native pass-through.
Typing `familiar on` enables the complete P0 Rust-runner command set listed
below. Every name has passed its typed catalog/runner tests, real broker and
packaged-sidecar path, and applicable traversal/listing/mutation release
resource gate. This does not broaden the documented syntax or enable any
prototype-only command. Familiar still starts paused, and the remaining
shell-transition, endurance, and release-hardening gates remain release work.

## Native shell state commands

Wingman does not translate `cd`, `chdir`, `pushd`, `popd`, or PowerShell's
`Set-Location`. Their common directory-navigation forms are already familiar,
and the native shell may provide useful extra behavior such as `cmd` drive
handling or PowerShell providers. They pass through unchanged in either mode.

Wingman's file-oriented commands use the active shell's resulting current
filesystem directory. If the active PowerShell location is not a filesystem
path, those commands fail clearly rather than inventing a Linux-like mapping.

## P0: required compatibility surface

| Workflow | Supported Unix syntax | Guarantee |
| --- | --- | --- |
| List and location | `ls`, `ll`, `ls -a`, `ls -l`, `pwd`, `clear` | List files, display the working directory, and clear the terminal. See [the navigation and creation contract](commands/NAVIGATION_CREATION.md). |
| Read files | `cat FILE`, `cat -n FILE` | Print text-file contents. See [the text stream contract](commands/TEXT_STREAM.md). |
| Create files and directories | `touch FILE`, `mkdir -p PATH` | Create files or nested directories. See [the navigation and creation contract](commands/NAVIGATION_CREATION.md). |
| Copy and move | `cp SOURCE DEST`, `cp -r SOURCE DEST`, `mv SOURCE DEST` | Operate on one explicit source and destination path under predictable overwrite rules. See [the copy and move contract](commands/COPY_MOVE.md). |
| Remove | `rm FILE`, `rm -r DIR`, `rm -rf DIR` | Permanently remove explicit Windows paths under Wingman's safety rules. See [the rm contract](commands/RM.md). |
| Find a command | `which NAME` | Locate an executable available to the active Windows environment. See [the navigation and creation contract](commands/NAVIGATION_CREATION.md). |
| Search text | `grep [-i,-n,-v,-F,-r] PATTERN FILE` | Search file or pipeline text. See [the grep contract](commands/GREP.md). |
| Work with lines | `head -n N`, `tail -n N`, `tail -f FILE`, `wc -l` | Process text lines from a file or supported pipeline. See [the text stream contract](commands/TEXT_STREAM.md). |
| Sort and deduplicate | `sort [-r,-n,-u]`, `uniq` | Perform basic text sorting and consecutive-line deduplication. See [the sort and uniq contract](commands/SORT_UNIQ.md). |
| Compose text workflows | `COMMAND | COMMAND [| COMMAND ...]`, `>`, `>>` | Connect supported P0 commands and save their output. |

### Constrained `find`

`find` is P0 because it is a common Unix workflow, but its grammar is narrow.
See [the find contract](commands/FIND.md) for its full rules:

```text
find PATH [-type f|d] [-name PATTERN|-iname PATTERN] [-mindepth N] [-maxdepth N] [-print]
```

No boolean expressions, permission predicates, `-exec`, or other side effects
are part of P0.

## P0 input grammar

Wingman deliberately recognises a small subset of one-line Unix command
syntax, not Bash as a programming language:

```text
line     = pipeline [redirect]
pipeline = command ("|" command)*
command  = P0-command argument*
redirect = (">" | ">>") output-path
```

Within that subset, P0 accepts whitespace-separated arguments, double-quoted
and single-quoted arguments, supported command options, `--` as the end of
options, pipelines of P0 commands, and one final `>` or `>>` redirection.
Backslashes in a path remain ordinary path characters, so Windows paths such
as `src\main.ts` are valid inputs.

P0 does not interpret shell glob expansion (for example `grep TODO *.txt`),
environment-variable expansion, command substitution, command chains such as
`&&` and `||`, `;`, input or error-stream redirection, or other Bash control
syntax. A wildcard is accepted when it is an explicit command argument, such
as `find -name "*.ts"`, rather than a shell-expanded file list.

## P1: evaluate after P0 is reliable

- `cut`, `tr`, and a limited `sed`
- `xargs`
- Advanced recursive `grep` filters such as include/exclude patterns and binary-file handling
- Additional `find` predicates such as `-size` and `-mtime`

These features require predictable behavior in every shell where Familiar
interpretation is enabled before they become a compatibility promise. Shell
availability is evaluated separately from command semantics.

## Explicitly out of scope

- Full Bash scripting syntax, command substitution, arrays, functions, or job
  control
- Linux permissions and ownership: `chmod`, `chown`, `umask`
- Linux process/signal behavior: `kill`, `nohup`, `jobs`, `fg`, `bg`
- Linux devices and special paths: `/dev/null`, sockets, FIFOs
- Linux package managers and a Linux distribution
- Symbolic-link compatibility: `ln -s`

This does not block native Windows administration. For example, `icacls`,
`taskkill`, `Stop-Process`, `Get-Acl`, and `Set-Acl` remain normal shell input
and execute with the privileges of the Wingman process.

## Safety and consistency rules

- Every P0 path and final redirection follows the shared
  [Windows path and filesystem contract](WINDOWS_PATH_CONTRACT.md).
- Every P0 text file, generated record, pipeline, and stdout sink follows the
  shared [text record and stream contract](TEXT_STREAM_MODEL.md).
- Never infer unsupported options or silently change their meaning.
- Do not claim Linux filesystem, access-control, signal, or mount behavior.
- Do not guarantee shell glob expansion or complex redirection beyond the P0
  subset.
- Prefer a predictable failure to an unsafe or surprising conversion.
- Windows PowerShell Familiar commands use the packaged adapter. `cmd` input
  remains native pass-through in P0; both shells must preserve native input
  exactly whenever interception is unavailable.

## MVP acceptance examples

The following must run predictably with Familiar mode enabled in Windows
PowerShell 5.1. In `cmd`, the same text must be passed unchanged to the native
shell:

```text
ls -a
pwd
cat README.md | grep Wingman | head -n 5
grep -in TODO src\main.ts
find src -type f -name "*.ts" | wc -l
mkdir -p temp\a\b
rm -rf temp
```
