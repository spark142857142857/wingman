# Wingman

[Korean](README.ko.md)

Wingman is a lightweight terminal MVP for Windows. It lets you switch between PowerShell and cmd while using familiar Linux commands and pipelines—without WSL.

**Stack:** Tauri 2, Rust (`portable-pty`), Vite, TypeScript, and xterm.js.

## Why

Wingman reduces a few common sources of friction in Windows terminals:

- The context-switching cost of opening PowerShell and cmd separately.
- Missing Linux command muscle memory, such as `ls`, `pwd`, and `cat`.
- Having to infer the active shell, compatibility mode, and starting directory.

This MVP deliberately focuses on a fast, local terminal experience; it does not include AI features.

## Features

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
    main.ts          # terminal UI, Linux Familiar mappings, shortcuts
    styles.css       # glass UI
  src-tauri/
    src/lib.rs       # PTY session start/write/resize
    tauri.conf.json  # Tauri configuration
  docs/              # test plan and manual smoke-test guide
  index.html
  package.json
  README.md
```

## Requirements

- Windows 10 or 11
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

The app starts in PowerShell by default. Enter `cmd` to open a nested cmd in the same terminal and `exit` to return. Enable Familiar mode to use commands such as `ls`, `pwd`, and `cat` through the appropriate Windows mappings.

## Build

```powershell
npm run tauri build
```

Build artifacts are generated under `src-tauri/target/release/`.

## Verify

```powershell
npm run typecheck
npm test
npm run build
```

`npm test` covers terminal input and shell state, PowerShell Linux Familiar pipelines, standard PowerShell/cmd regressions, Rust PTY session shutdown, and PowerShell profile loading.

- Full test plan: [docs/TEST_MATRIX.md](docs/TEST_MATRIX.md)
- Manual app checks: [docs/MANUAL_SMOKE_TEST.md](docs/MANUAL_SMOKE_TEST.md)

## Linux Familiar Mapping

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

## Demo Checklist

- [ ] Start a PowerShell session
- [ ] Switch to cmd
- [ ] Run `ls` and `pwd` with `Linux Familiar` enabled
- [ ] Confirm the status bar reflects the shell, compatibility mode, and directory
- [ ] Confirm `Ctrl+Shift+C`, `Ctrl+V`, and `Ctrl+Shift+V` work
