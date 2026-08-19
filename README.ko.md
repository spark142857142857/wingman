# Wingman

[English](README.md)

> **프로토타입 snapshot — 최종 P0 호환성 약속이 아닙니다.** 아래의 과거 기능과
> mapping 절은 cutover 전 prototype을 설명하며 target 밖 동작도 포함합니다.
> [프로토타입·목표 경계](docs/PROTOTYPE_TARGET_BOUNDARY.ko.md)와
> [목표 호환성 계약](docs/COMPATIBILITY_CONTRACT.ko.md)을 참고하세요.
>
> **현재 릴리스 후보 상태(2026-08-20): Familiar는 `PAUSED`로 시작합니다.**
> 검증된 Windows PowerShell 5.1 prompt에서 `familiar on`을 입력하면 계약한 P0
> Rust-runner 명령인 `pwd`, `clear`, `which`, `ls`/`ll`,
> `find`, `cat`, `head`, `tail`, `wc -l`, `grep`, `sort`, `uniq`, `mkdir`, `touch`,
> `cp`, `mv`, `rm`과 문서화한 pipeline·redirection만 활성화합니다. `cmd.exe`는
> 지원하는 native terminal session이지만 그대로 전달하며 Familiar 변환을 하지
> 않습니다. 최종 release 수락과 외부 Windows matrix는 아직 열려 있습니다.

Windows용 가벼운 native terminal 후보입니다. 실제 PowerShell 또는 `cmd.exe` process를
유지하고 검증된 PowerShell prompt에서 의도적으로 작은 Unix 명령 친숙성 계층을
제공합니다. WSL, Bash 또는 Linux runtime을 제공하지 않습니다.

**스택:** Tauri 2, Rust (`portable-pty`), Vite, TypeScript, xterm.js

## 목표

Wingman은 Windows 터미널에서 자주 겪는 다음 불편을 줄입니다.

- PowerShell과 cmd를 따로 열어야 하는 전환 비용
- `ls`, `pwd`, `cat` 등 Linux 명령에 익숙한 사용 흐름
- 현재 shell, 호환 모드, 시작 경로를 한눈에 보기 어려운 문제

이 MVP는 AI 기능 없이 **빠른 로컬 터미널 UX**에 집중합니다.

## 현재 P0 후보

- Windows PowerShell 5.1 또는 native `cmd.exe` root session 실행
- Native shell 문법, 환경, 현재 directory, 권한, foreground child, 외부 프로그램 보존
- 검증된 PowerShell prompt에서 문서화한 P0 Familiar 문법 opt-in
- 제한된 path, stream, resource, 취소, session isolation을 갖춘 packaged Rust sidecar
- Line break가 있는 paste는 원래 byte를 보내기 전에 확인
- 설치본은 `wingman [--shell powershell|cmd] [--] [PATH]`로 실행

현재 검증 기준은 [릴리스 테스트 매트릭스](docs/RELEASE_TEST_MATRIX.ko.md),
[릴리스 수동 smoke](docs/RELEASE_SMOKE_TEST.ko.md),
[기록한 성능 기준](docs/PERFORMANCE_BASELINES.ko.md)이다.

## 과거 prototype 기능 snapshot

다음 목록은 migration 증거로 보존한다. 현재 P0 지원 약속이 아니다.

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
    main.ts          # 터미널 UI, session-tagged 입력, 단축키
    styles.css       # glass UI
  src-tauri/
    src/lib.rs       # PTY/session, broker, launch, Tauri 경계
    tauri.conf.json  # Tauri 설정
  docs/              # 테스트 계획과 수동 스모크 테스트 가이드
  index.html
  package.json
  README.md
```

## 로컬 빌드 요구 사항

- 현재 기록한 로컬 증거는 Windows 11이며 최종 지원 Windows matrix는 외부 릴리스
  게이트로 남아 있음
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

기본 session은 PowerShell이다. `cmd`를 입력하면 일반 native foreground child가 열리고
`exit`으로 PowerShell에 돌아온다. `cmd.exe` root session은 공개 release CLI의
`--shell cmd`로 시작한다. Familiar interception은 검증된 Windows PowerShell 5.1
prompt에서만 제공한다.

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
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
```

`npm test`는 frontend 입력·보안, Windows layout·packaging과 ignored가 아닌 전체 Rust
계약 suite를 실행한다.

- 현재 릴리스 matrix: [docs/RELEASE_TEST_MATRIX.ko.md](docs/RELEASE_TEST_MATRIX.ko.md)
- 현재 수동 앱 게이트: [docs/RELEASE_SMOKE_TEST.ko.md](docs/RELEASE_SMOKE_TEST.ko.md)
- 과거 prototype matrix: [docs/TEST_MATRIX.md](docs/TEST_MATRIX.md)
- 과거 prototype smoke: [docs/MANUAL_SMOKE_TEST.md](docs/MANUAL_SMOKE_TEST.md)

## 과거 prototype mapping

이 표는 옛 shell별 prototype 기록이다. 현재 P0 후보에서는 문서화한 PowerShell
adapter만 Familiar 입력을 가로챌 수 있고 `cmd.exe`는 native pass-through다.

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

## 과거 prototype 데모 체크리스트

- [ ] PowerShell 세션 시작
- [ ] cmd 전환
- [ ] `Linux Familiar` ON에서 `ls` / `pwd` 동작
- [ ] 상태바에 shell / compat / 경로 반영
- [ ] `Ctrl+Shift+C` / `Ctrl+V` / `Ctrl+Shift+V` 동작
