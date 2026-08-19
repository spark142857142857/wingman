# Wingman

[English](README.md)

> **프로토타입 snapshot — 최종 P0 호환성 약속이 아닙니다.** 아래 기능은 현재
> repository의 prototype을 설명하며 target 밖 동작도 포함합니다.
> [프로토타입·목표 경계](docs/PROTOTYPE_TARGET_BOUNDARY.ko.md)와
> [목표 호환성 계약](docs/COMPATIBILITY_CONTRACT.ko.md)을 참고하세요.
>
> **현재 안전 cutover 상태(2026-08-19): Familiar는 `PAUSED`로 시작합니다.**
> `familiar on`은 계약한 P0 Rust-runner 명령인 `pwd`, `clear`, `which`, `ls`/`ll`,
> `find`, `cat`, `head`, `tail`, `wc -l`, `grep`, `sort`, `uniq`, `mkdir`, `touch`,
> `cp`, `mv`, `rm`과 문서화한 pipeline·redirection만 활성화합니다. 이 snapshot 뒤쪽에
> 적힌 prototype 전용 명령은 앱이 source하지 않습니다.

Windows용 가벼운 터미널 MVP입니다. PowerShell과 cmd를 바로 전환하고, WSL 없이도 Linux Familiar 명령과 파이프를 사용할 수 있습니다.

**스택:** Tauri 2, Rust (`portable-pty`), Vite, TypeScript, xterm.js

## 목표

Wingman은 Windows 터미널에서 자주 겪는 다음 불편을 줄입니다.

- PowerShell과 cmd를 따로 열어야 하는 전환 비용
- `ls`, `pwd`, `cat` 등 Linux 명령에 익숙한 사용 흐름
- 현재 shell, 호환 모드, 시작 경로를 한눈에 보기 어려운 문제

이 MVP는 AI 기능 없이 **빠른 로컬 터미널 UX**에 집중합니다.

## 기능

- 같은 터미널에서 PowerShell → cmd 진입, `exit`으로 부모 PowerShell 복귀
- Linux Familiar 모드 ON/OFF
- PowerShell Linux Familiar 호환 계층
  - `grep`, `head`, `tail`, `find`, `sort`, `uniq`, `wc`
  - `cut`, `tr`, 자주 쓰는 `sed`, 안전한 인자 전달 방식의 `xargs`
  - `ls`, `ll`, `cat`, `touch`, `which`, `mkdir -p`, `rm -rf`
  - `cat file | grep text | head -n 10` 같은 파이프 지원
- cmd Linux Familiar 매핑
  - `ls`, `pwd`, `cat`, `grep`, `head`, `tail`, `sort`, `wc -l`, `rm`, `mv`, `cp` 등
  - `cat file | grep text | head -n 10`과 `<`, `>`, `>>` 텍스트 파이프·리다이렉션
- 하단 상태바에 shell / compat / 시작 경로 표시
- 단축키
  - `Ctrl+Shift+C` 복사
  - `Ctrl+V` 또는 `Ctrl+Shift+V` 붙여넣기
  - `Ctrl+Shift+R` 새 세션
  - `Ctrl` + `+` / `-` 글자 크기
- acrylic/glass 스타일 데모 UI 및 UTF-8 코드페이지 설정

## 기술 스택

| 계층 | 기술 |
| --- | --- |
| 데스크톱 셸 | Tauri 2 |
| PTY 백엔드 | Rust + `portable-pty` |
| 프런트엔드 | Vite + TypeScript |
| 터미널 UI | xterm.js + FitAddon |
| 셸 | PowerShell, cmd |

## 프로젝트 구조

```text
wingman/
  src/
    main.ts          # 터미널 UI, Linux Familiar 매핑, 단축키
    styles.css       # glass UI
  src-tauri/
    src/lib.rs       # PTY 세션 시작/write/resize
    tauri.conf.json  # Tauri 설정
  docs/              # 테스트 계획과 수동 스모크 테스트 가이드
  index.html
  package.json
  README.md
```

## 요구 사항

- Windows 10/11
- Node.js 18+
- Rust (`rustc`, `cargo`)
- WebView2 (일반적으로 Windows에 기본 포함)

## 설치

```powershell
git clone https://github.com/spark142857142857/wingman.git
cd wingman
npm install
```

## 개발 실행

```powershell
npm run tauri dev
```

기본 세션은 PowerShell입니다. `cmd`를 입력하면 같은 터미널 안에서 cmd로 진입하고, `exit`으로 부모 PowerShell에 복귀합니다. Familiar 모드를 켜면 `ls`, `pwd`, `cat` 등의 명령을 각 Windows 셸에 맞게 매핑해 사용할 수 있습니다.

## 빌드

```powershell
npm run tauri build
```

빌드 산출물은 `src-tauri/target/release/` 아래에 생성됩니다.

## 검증

```powershell
npm run typecheck
npm test
npm run build
```

`npm test`는 터미널 입력과 셸 상태, PowerShell Linux Familiar 파이프, 일반 PowerShell/cmd 회귀 동작, Rust PTY 세션 종료와 PowerShell 프로필 로딩을 검사합니다.

- 전체 테스트 기준: [docs/TEST_MATRIX.md](docs/TEST_MATRIX.md)
- 실제 앱 수동 확인: [docs/MANUAL_SMOKE_TEST.md](docs/MANUAL_SMOKE_TEST.md)

## Linux Familiar 매핑

| 입력 | PowerShell | cmd |
| --- | --- | --- |
| `ls` | `Get-ChildItem` | `dir` |
| `ll` | `Get-ChildItem ... Format-Table ...` | `dir` |
| `pwd` | `Get-Location` | `cd` |
| `clear` | `Clear-Host` | `cls` |
| `cat file` | `Get-Content file` | `type file` |
| `rm path` | `Remove-Item -Force path` | `del /f path` |
| `mv a b` | `Move-Item a b` | `move a b` |
| `cp a b` | `Copy-Item a b` | `copy a b` |

## 참고 및 알려진 제한 사항

- 이 MVP에는 AI 기능이 없습니다.
- Windows 전용입니다.
- 프로젝트 경로에 한글 또는 비ASCII 문자가 있으면 Windows `RC.EXE` 리소스 컴파일이 실패할 수 있습니다.
  - 문제 예시: `D:\Agent프로젝트\wingman`
  - 해결: `C:\dev\wingman`처럼 ASCII 전용 경로로 복사하거나 이동해 실행하세요.

## 데모 체크리스트

- [ ] PowerShell 세션 시작
- [ ] cmd 전환
- [ ] `Linux Familiar` ON에서 `ls` / `pwd` 동작
- [ ] 상태바에 shell / compat / 경로 반영
- [ ] `Ctrl+Shift+C` / `Ctrl+V` / `Ctrl+Shift+V` 동작
