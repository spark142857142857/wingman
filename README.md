# Wingman

[Korean](README.ko.md)

> **Historical prototype material — not the current P0 compatibility promise.**
> The explicitly labelled feature and mapping sections below describe the
> pre-cutover prototype and include behavior outside the target.
> See the [prototype/target boundary](docs/PROTOTYPE_TARGET_BOUNDARY.md) and
> [target compatibility contract](docs/COMPATIBILITY_CONTRACT.md).
>
> **Current release-candidate status (2026-08-20): Familiar starts `PAUSED`.**
> At a validated Windows PowerShell 5.1 prompt, `familiar on` enables the
> contracted P0 Rust-runner set: `pwd`, `clear`,
> `which`, `ls`/`ll`, `find`, `cat`, `head`, `tail`, `wc -l`, `grep`, `sort`,
> `uniq`, `mkdir`, `touch`, `cp`, `mv`, and `rm`, with only their documented
> pipelines and redirection. `cmd.exe` is a supported native terminal session
> but remains pass-through; it does not receive Familiar conversion. Final
> release acceptance and the external Windows matrix remain open.

Wingman is a lightweight native terminal candidate for Windows. It keeps the
real PowerShell or `cmd.exe` process and adds a deliberately small Unix-command
familiarity layer at validated PowerShell prompts—without providing WSL, Bash,
or a Linux runtime.

**Stack:** Tauri 2, Rust (`portable-pty`), Vite, TypeScript, and xterm.js.

## Why

Wingman reduces a few common sources of friction in Windows terminals:

- The context-switching cost of opening PowerShell and cmd separately.
- Missing Linux command muscle memory, such as `ls`, `pwd`, and `cat`.
- Having to infer the active shell, compatibility mode, and starting directory.

This MVP deliberately focuses on a fast, local terminal experience; it does not include AI features.

## Current P0 candidate

- Start a root session with Windows PowerShell 5.1 or native `cmd.exe`.
- Preserve native shell syntax, environment, current directory, permissions,
  foreground children, and external programs.
- Opt into the documented P0 Familiar grammar at a validated PowerShell prompt.
- Run supported P0 commands through a packaged Rust sidecar with bounded paths,
  streams, resources, cancellation, and session isolation.
- Confirm any paste containing a line break before sending its original bytes.
- Launch installed windows with `wingman [--shell powershell|cmd] [--] [PATH]`.

Current verification: [release test matrix](docs/RELEASE_TEST_MATRIX.md),
[manual release smoke](docs/RELEASE_SMOKE_TEST.md), and
[recorded performance baselines](docs/PERFORMANCE_BASELINES.md).

## Historical prototype feature snapshot

The following list is retained as migration evidence. It is not the current P0
support promise.

- Enter cmd from PowerShell in the same terminal session; `exit` returns to the parent PowerShell.
- Toggle Linux Familiar mode on or off.
- PowerShell Linux Familiar compatibility layer:
  - `grep`, `head`, `tail`, `find`, `sort`, `uniq`, and `wc`
  - `cut`, `tr`, common `sed` usage, and `xargs` with safe argument passing
  - `ls`, `ll`, `cat`, `touch`, `which`, `mkdir -p`, and `rm -rf`
  - Pipelines such as `cat file | grep text | head -n 10`
- cmd Linux Familiar mappings:
  - `ls`, `pwd`, `cat`, `grep`, `head`, `tail`, `sort`, `wc -l`, `rm`, `mv`, and `cp`
  - Text pipelines and `<`, `>`, and `>>` redirection, including `cat file | grep text | head -n 10`
- Status bar showing the shell, Familiar mode, and starting directory.
- Keyboard shortcuts:
  - `Ctrl+Shift+C` to copy
  - `Ctrl+V` or `Ctrl+Shift+V` to paste
  - `Ctrl+Shift+R` to start a new session
  - `Ctrl` + `+` / `-` to change font size
- Acrylic/glass demo UI with UTF-8 code-page configuration.

## Tech Stack

| Layer | Technology |
| --- | --- |
| Desktop shell | Tauri 2 |
| PTY backend | Rust + `portable-pty` |
| Frontend | Vite + TypeScript |
| Terminal UI | xterm.js + FitAddon |
| Shells | PowerShell, cmd |

## Project Structure

```text
wingman/
  src/
    main.ts          # terminal UI, session-tagged input, shortcuts
    styles.css       # glass UI
  src-tauri/
    src/lib.rs       # PTY/session, broker, launch, and Tauri boundary
    tauri.conf.json  # Tauri configuration
  docs/              # test plan and manual smoke-test guide
  index.html
  package.json
  README.md
```

## Local build requirements

- Windows 11 for the currently recorded local evidence; the final supported
  Windows matrix remains an external release gate
- Node.js 18+
- Rust (`rustc` and `cargo`)
- WebView2 (normally included with Windows)

## Setup

```powershell
git clone https://github.com/spark142857142857/wingman.git
cd wingman
npm install
```

## Run in Development

```powershell
npm run tauri dev
```

The app starts in PowerShell by default. Entering `cmd` opens an ordinary native
foreground child and `exit` returns to PowerShell. To start a `cmd.exe` root
session, use the public release CLI with `--shell cmd`. Familiar interception is
available only at a validated Windows PowerShell 5.1 prompt.

## Build

```powershell
npm run tauri build
```

Build artifacts are generated under `src-tauri/target/release/`.

## Verify

```powershell
npm run verify
```

`npm run verify` runs type checking, frontend input/security checks, Windows
layout and packaging checks, the complete non-ignored Rust contract suite, the
production frontend build, formatting, and warning-free Clippy.

- Current release matrix: [docs/RELEASE_TEST_MATRIX.md](docs/RELEASE_TEST_MATRIX.md)
- Current manual app gate: [docs/RELEASE_SMOKE_TEST.md](docs/RELEASE_SMOKE_TEST.md)
- Historical prototype matrix: [docs/TEST_MATRIX.md](docs/TEST_MATRIX.md)
- Historical prototype smoke: [docs/MANUAL_SMOKE_TEST.md](docs/MANUAL_SMOKE_TEST.md)

## Historical prototype mapping

This table records the old shell-specific prototype. In the current P0
candidate, only the documented PowerShell adapter may intercept Familiar input;
`cmd.exe` remains native pass-through.

| Input | PowerShell | cmd |
| --- | --- | --- |
| `ls` | `Get-ChildItem` | `dir` |
| `ll` | `Get-ChildItem ... Format-Table ...` | `dir` |
| `pwd` | `Get-Location` | `cd` |
| `clear` | `Clear-Host` | `cls` |
| `cat file` | `Get-Content file` | `type file` |
| `rm path` | `Remove-Item -Force path` | `del /f path` |
| `mv a b` | `Move-Item a b` | `move a b` |
| `cp a b` | `Copy-Item a b` | `copy a b` |

## Notes and Known Limitations

- This MVP has no AI features.
- Windows only.
- Non-ASCII characters in the project path can make Windows `RC.EXE` resource compilation fail.
  - Example problem path: `D:\Agent-projects\wingman`
  - Workaround: copy or move the project to an ASCII-only path, such as `C:\dev\wingman`.

## Historical prototype demo checklist

- [ ] Start a PowerShell session
- [ ] Switch to cmd
- [ ] Run `ls` and `pwd` with `Linux Familiar` enabled
- [ ] Confirm the status bar reflects the shell, compatibility mode, and directory
- [ ] Confirm `Ctrl+Shift+C`, `Ctrl+V`, and `Ctrl+Shift+V` work
