# Application CLI Launch Contract

Status: current P0 contract, implemented by the release candidate. Final release
acceptance remains governed by the release matrix.

## Executable names

```text
wingman.exe         user-facing terminal application
wingman-runner.exe  internal P0 sidecar; not a user command
```

The installed application is invokable as `wingman` from ordinary PowerShell
and `cmd` sessions. `PATHEXT` or a Windows app-execution alias permits omission
of the `.exe` suffix.

## P0 public grammar

```text
wingman [--shell powershell|cmd] [--] [PATH]
wingman --help
wingman --version
```

`--shell` occurs at most once and must precede the optional `--` and path.
There is at most one path. `--help` and `--version` must be the only argument;
short options, `--shell=value`, and additional operands are syntax exit `2`.

The optional starting directory follows the shared
[Windows path and filesystem contract](WINDOWS_PATH_CONTRACT.md). Drive-relative,
root-relative, device-namespace, ADS, wildcard, and ambiguous path forms are
rejected before any GUI process is started.

- With no path, the new Wingman window starts in the caller's current Windows
  filesystem directory.
- A relative path is resolved from the caller's current directory. An absolute
  path remains a native Windows path.
- P0 accepts an existing directory only. A missing path or file path prints a
  concise stderr diagnostic, opens no window, and exits `1`.
- Invalid syntax or an unsupported option exits `2`.
- With no `--shell`, use the saved shell preference; if none exists, use
  Windows PowerShell.
- A successful GUI handoff exits the launcher with `0` without waiting for the
  Wingman window to close.
- Each invocation requests a new Wingman window. Single-instance coordination,
  if added later, must preserve this visible behavior.

## P0 process topology

The target is one signed `wingman.exe` with two process roles, plus the separate
runner. This is a mandatory boundary-spike decision, not implementation approval.

```text
PowerShell/cmd
  -> wingman.exe public console launcher
       -> same wingman.exe in protected internal GUI role
            -> Wingman window, WebView/renderer, PTY, selected shell

Wingman P0 submission
  -> wingman-runner.exe one-shot sidecar
```

The public binary is a Windows console-subsystem application so `cmd` and
PowerShell wait for its launcher status. On the supported Windows matrix, the
packaged manifest uses the detached console-allocation policy so an Explorer or
shortcut launch does not create a stray console; this is part of the boundary
spike, not an assumption. The public invocation always enters launcher role. It parses and validates the
complete public grammar, snapshots the caller's filesystem cwd/environment and
access token, resolves the starting directory, and performs no GUI initialization
for `--help`, `--version`, or any failure.

For a valid window request, the launcher creates the same installed binary as a
new process in an internal GUI role. The GUI child has no console window, does
not inherit general launcher handles or stdio, uses the same unelevated/elevated
access token, and survives normal launcher exit. The exact Windows creation
flags must prove these properties in the boundary spike; the initial candidate
is `CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP` with an explicit inherited-handle
allowlist.

Internal GUI role is not a public CLI. Its command line contains only a fixed
internal marker and an inherited handoff-handle number. The handle carries a
bounded, versioned message containing an unpredictable nonce, parent identity,
resolved start directory, and selected shell. A direct or replayed internal
invocation without that live inherited handle exits `2` and opens no window.
Paths, environment values, and shell source are never serialized into the child
command line.

## Readiness and error propagation

The launcher and child complete a two-way handoff:

1. the child validates the internal message and revalidates the directory;
2. it creates the top-level window and starts the selected PTY shell;
3. it reports either bounded `Ready` or `Failed` over the handoff channel;
4. the launcher acknowledges `Ready`, then exits `0`; only after that
   acknowledgement may the GUI child own the session independently.

`Ready` means the window exists and the initial shell/PTY accepted ownership; it
does not wait for the user to close the window. A pre-readiness filesystem,
asset, WebView, PTY, or shell-start failure opens no usable window, prints one
bounded launcher stderr diagnostic, and exits `1`. Syntax is `2`. Ctrl+C while
waiting exits `130`. A 10-second handoff timeout is operational `1`; without a
launcher acknowledgement the child must terminate instead of becoming an
unreported orphan. After acknowledged readiness, later GUI failure cannot alter
the already returned launcher status.

The boundary spike must prove this exact behavior from `cmd` and Windows
PowerShell 5.1, with spaces/Korean/UNC paths, missing paths, invalid combinations,
missing assets, shell-start failure, timeout, Ctrl+C, ordinary and elevated
tokens, repeated launches, and direct internal-role abuse. If same-binary launch
cannot satisfy console-free child, reliable status, and independent lifetime,
the contract is reopened before adding a separately signed internal GUI binary;
public `wingman.exe` and `wingman-runner.exe` names do not change silently.

## Native pass-through inside Wingman

`wingman` is not a P0 compatibility command. Typing it inside a Wingman shell
passes through as a native executable invocation and opens another Wingman
window under this same contract.

## Installer registration

- An NSIS/MSI-style installation registers the application command in the
  current user's command search path without requiring machine-wide scope.
- A future MSIX/Store package uses a Windows App Execution Alias named
  `wingman.exe`.
- Uninstall removes only the registration created by that installation.
- The internal `wingman-runner.exe` is never registered as a general PATH
  command.

CLI path and option values are parsed as application arguments, never rebuilt
as shell source.

## Research basis

- Microsoft documents that console-subsystem applications block `cmd` and
  PowerShell, and that the Windows 11 24H2 detached console-allocation manifest
  policy can avoid allocating a console outside an existing session:
  [Console Allocation Policy](https://learn.microsoft.com/en-us/windows/console/console-allocation-policy).
- `CREATE_NO_WINDOW` runs a console child without a console, while
  `CREATE_NEW_PROCESS_GROUP` creates a separate process group and disables
  inherited Ctrl+C handling for that child:
  [Process Creation Flags](https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags).
- Microsoft recommends `STARTUPINFOEX` with
  `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` when only specific handles should be
  inherited: [Create processes](https://learn.microsoft.com/en-us/windows/win32/procthread/creating-processes).
