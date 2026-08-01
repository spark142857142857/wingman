# Wingman

Windows용 가벼운 터미널 MVP입니다.  
PowerShell / cmd를 바로 전환하고, Linux에 익숙한 명령(`ls`, `pwd`, `cat` 등)을 Windows 명령으로 매핑합니다.

스택: **Tauri 2 + Rust (portable-pty) + Vite + TypeScript + xterm.js**

## Why

Windows 터미널에서 자주 겪는 불편을 줄이는 게 목표입니다.

- PowerShell과 cmd를 따로 켜야 하는 전환 비용
- `ls`, `pwd`, `cat` 같은 Linux 습관이 깨지는 문제
- 현재 shell / compat / cwd를 한눈에 보고 싶은 니즈

이 MVP는 AI 기능 없이, **빠른 로컬 터미널 UX**에만 집중합니다.

## Features

- PowerShell / cmd 세션 전환
- Linux Familiar 모드 ON/OFF
- 명령 매핑
  - `ls`, `ll`, `pwd`, `clear`, `cat`, `rm`, `mv`, `cp`
  - PowerShell / cmd 각각에 맞는 Windows 명령으로 변환
- 하단 상태바: `Shell` / `Compat` / `cwd`
- 단축키
  - `Ctrl+Shift+C` 복사
  - `Ctrl+Shift+V` 붙여넣기
- acrylic/glass 스타일 데모 UI
- UTF-8 코드페이지 설정 포함

## Tech Stack

| Layer | Tech |
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
    main.ts          # 터미널 UI, Linux Familiar 매핑, 단축키
    styles.css       # glass UI
  src-tauri/
    src/lib.rs       # PTY 세션 start/write/resize
    tauri.conf.json  # 앱 설정
  index.html
  package.json
  README.md
```

## Requirements

- Windows 10/11
- Node.js 18+
- Rust (rustc / cargo)
- WebView2 (보통 Windows에 기본 포함)

## Setup

```powershell
git clone https://github.com/spark142857142857/wingman.git
cd wingman
npm install
```

## Run (Dev)

```powershell
npm run tauri dev
```

실행되면:

1. 기본 셸은 PowerShell
2. 상단에서 `cmd`로 전환 가능
3. `Linux Familiar`를 켠 뒤 `ls`, `pwd`, `cat` 등을 입력하면 Windows 명령으로 매핑됩니다
4. 하단 상태바에서 shell / compat / cwd를 확인할 수 있습니다

## Build

```powershell
npm run tauri build
```

빌드 산출물은 `src-tauri/target/release/` 아래에 생성됩니다.

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

## Notes / Known Limitation

- 이 MVP에는 AI 기능이 없습니다.
- Windows 전용입니다.
- 프로젝트 경로에 한글/비ASCII 문자가 있으면 Windows `RC.EXE` 리소스 컴파일이 실패할 수 있습니다.
  - 예: `D:\Agent프로젝트\wingman`
  - 해결: ASCII 경로로 복사/이동 후 실행
  - 예: `C:\dev\wingman`

## Demo Checklist

- [ ] PowerShell 세션 시작
- [ ] cmd 전환
- [ ] `Linux Familiar` ON에서 `ls` / `pwd` 동작
- [ ] 상태바 shell / compat / cwd 반영
- [ ] `Ctrl+Shift+C` / `Ctrl+Shift+V` 동작

## License

MIT