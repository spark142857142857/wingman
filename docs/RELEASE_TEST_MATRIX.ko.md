# Wingman 릴리스 테스트 매트릭스

영문판: [RELEASE_TEST_MATRIX.md](RELEASE_TEST_MATRIX.md)

상태: 현재 P0 릴리스 검증 기준. 과거 prototype 매트릭스인
[TEST_MATRIX.md](TEST_MATRIX.md)는 migration 증거로만 보존한다.

## 검증 범위

- Windows PowerShell 5.1만 검증된 P0 Familiar interception을 제공한다.
- `cmd.exe`는 지원하는 native terminal session이지만 모든 입력을 그대로 전달한다.
  `cmd` 성능 probe가 통과해도 Familiar 명령 변환을 지원한다는 뜻은 아니다.
- Familiar는 `PAUSED`로 시작한다. 검증된 PowerShell prompt에서만
  `familiar on`, `familiar off`, `familiar status`를 실행할 수 있다.
- P0 명령과 문법은 [COMPATIBILITY_CONTRACT.ko.md](COMPATIBILITY_CONTRACT.ko.md)를
  따른다. Prototype 전용 `cut`, `tr`, `sed`, `xargs` 동작은 릴리스 기능이 아니다.
- Test는 직접 관찰한 동작만 통과시킨다. Unit coverage는 packaged sidecar, GUI,
  installer, manual 또는 외부 matrix를 대신하지 않는다.

## 증거 상태

| 상태 | 의미 |
| --- | --- |
| 통과 | 정확한 릴리스 후보에서 적힌 명령이나 관찰이 통과했다. |
| 실패 | 관찰한 동작이 계약이나 상한을 위반했다. |
| 외부 | 다른 boot, OS, machine, 자격 증명 또는 승인된 context가 필요하다. |
| 미실행 | 현재 후보의 증거를 수집하지 않았다. |

과거 결과는 [PERFORMANCE_BASELINES.ko.md](PERFORMANCE_BASELINES.ko.md)에 기록한다.
최종 후보는 다시 실행해야 하며 이전 commit의 통과를 자동 승계하지 않는다.

## 1. 결정론적 source 게이트

Repository root의 일반 비관리자 PowerShell에서 실행한다.

```powershell
npm ci
npm run typecheck
npm test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
```

| 영역 | 기준 coverage |
| --- | --- |
| Frontend 입력 | `terminal-input`, `terminal-paste`, `terminal-shortcuts`, `terminal-security` TypeScript test |
| Windows layout | `layout_regression.tests.ps1` |
| Sidecar layout | `sidecar_packaging.tests.ps1` |
| Rust 계약 | `src-tauri/src`와 `src-tauri/tests`의 ignored가 아닌 모든 unit·integration test |
| Frontend artifact | TypeScript typecheck와 Vite production build |
| Rust 위생 | `cargo fmt --check`와 warning 없는 Clippy |

`npm ci`는 dependency를 깨끗하게 재구성하는 전제 조건이지 앱 동작 증거가 아니다.

## 2. 계약과 test 대응표

| 계약 영역 | 주요 자동 증거 |
| --- | --- |
| Ownership, lexer, parser, catalog | `lexer_contract`, `parser_contract`, `catalog_contract`, `frontend_decision_contract` |
| `pwd`, `clear`, `which`, `ls`/`ll`, `mkdir`, `touch` | `clear_contract`, `which_contract`, `ls_contract`, `mkdir_contract`, `touch_contract` |
| `find`와 pattern 문법 | `find_contract`, `find_pattern_contract`, `runner_readonly_contract` |
| `grep`과 pattern 문법 | `grep_pattern_contract`, `runner_grep_contract` |
| `cat`, `head`, `tail`, `wc -l`, `sort`, `uniq` | `runner_readonly_contract`, `runner_dispatch_contract`, `sort_support` unit test |
| `cp`, `mv`, `rm` | `cp_contract`, `mv_contract`, `rm_contract`, mutation resource test |
| Pipeline, redirection, record, 취소 | `pipeline_contract`, `text_stream_contract`, `ordered_pipeline` unit test, `runner_io_contract`, `runner_process_contract` |
| Windows path, reparse point, race | `windows_path_contract`, `runner_io_contract`, 명령별 filesystem suite |
| Prepared request와 runner transport | `runner_transport_contract`, `powershell_runner_transport_contract`, `session_broker_contract`, `named_pipe_security_contract` |
| Prompt readiness와 editor replacement | `editor_readiness_contract`, `shell_adapter_contract`, `oob_vertical_contract` |
| Session generation과 입력 순서 | `terminal_session_contract`, `session_input_contract`, `session_runtime_contract`, `pty_output_flow` unit test |
| 공개 CLI와 보호된 GUI handoff | `cli_launch_contract`와 아래 black-box CLI suite |

이 표는 위치를 알려 주는 용도다. Module 사이 regression은 한 행에만 속하지 않으므로
수락할 때는 전체 test 명령을 실행한다.

## 3. 릴리스 artifact와 보안 게이트

Artifact test 전에 정확한 후보를 build한다.

```powershell
npm run tauri build
npm run test:release-bundle
npm run test:release-security
npm run test:cli-launch
npm run test:installer
npm run test:app-data
```

| 게이트 | 필요한 증거 |
| --- | --- |
| Bundle | Packaged GUI와 runner가 보호된 설치 layout에 있고 개발 전용 script는 없다. |
| 보안 | Release binary role, runtime asset, pipe ACL/security context와 직접 internal-role 거부가 계약을 만족한다. |
| CLI | PowerShell과 `cmd` caller에서 help/version, 유효 shell/path, 잘못된 문법/path, handoff, child lifetime, timeout/Ctrl+C, orphan 부재를 검사한다. |
| Installer | 설치, 재설치, launch 등록, 한글/공백 설치 path, 제거와 무관한 파일 비삭제를 통과한다. 설치 tree는 60 MiB 이하다. |
| App data | 격리한 interactive PowerShell 실행 100회가 끝나고 local profile이 100 MiB 이하다. |

로컬 개발 인증서는 최종 릴리스 identity가 아니므로 code signing은 별도로 확인한다.

## 4. 릴리스 성능과 자원 게이트

### Runner component

```powershell
cargo test --release --manifest-path src-tauri/Cargo.toml --test runner_performance_contract cached_runner_timing_baseline -- --ignored --exact --nocapture
cargo test --release --manifest-path src-tauri/Cargo.toml --test runner_process_contract idle_tail_follow_runner_stays_below_the_cpu_ceiling -- --ignored --exact --nocapture
cargo test --release --manifest-path src-tauri/Cargo.toml --test runner_resource_contract sort_resource_limit_stays_bounded_and_fails_closed -- --ignored --exact --nocapture
cargo test --release --manifest-path src-tauri/Cargo.toml --test runner_resource_contract traversal_and_listing_resource_limits_are_bounded -- --ignored --exact --nocapture
cargo test --release --manifest-path src-tauri/Cargo.toml --test runner_mutation_resource_contract mutation_resource_limits_are_bounded_and_atomic -- --ignored --exact --nocapture
```

### GUI matrix

Shell parameter가 있는 script는 `powershell`과 `cmd`로 각각 한 번 실행한다. `cmd`
script는 test 전용 native workload로 terminal transport와 rendering을 검사할 뿐 Familiar
interception을 켜지 않는다.

```powershell
$Exe = 'src-tauri\target\release\wingman.exe'
$Shells = @('powershell', 'cmd')
foreach ($Shell in $Shells) {
  powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_warm_startup_distribution.tests.ps1 -Executable $Exe -ShellKind $Shell -WarmupCount 3 -SampleCount 20 -TimeoutSeconds 15
  powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_process_tree.tests.ps1 -Executable $Exe -ShellKind $Shell
  powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_bulk_output.tests.ps1 -Executable $Exe -ShellKind $Shell
  powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_bulk_input_latency.tests.ps1 -Executable $Exe -ShellKind $Shell
  powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_bulk_retained_memory.tests.ps1 -Executable $Exe -ShellKind $Shell
  powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_scrollback_ceiling.tests.ps1 -Executable $Exe -ShellKind $Shell
  powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_bulk_host_comparison.tests.ps1 -WingmanExecutable $Exe -ShellKind $Shell
}
```

PowerShell에서는 인증된 editor와 30분 endurance 경로도 실행한다.

```powershell
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_editor_readiness.tests.ps1 -Executable $Exe -TimeoutSeconds 30
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_endurance.tests.ps1 -Executable $Exe
```

모든 상한과 표본 규칙은 [PERFORMANCE_BUDGET.ko.md](PERFORMANCE_BUDGET.ko.md)를
따른다. Harness가 versioned 원시 결과를 출력해야 하며 창이 열려 있었다는 사실만으로
통과하지 않는다.

## 5. 수동 앱 게이트

자동 게이트 뒤 같은 release artifact에서
[RELEASE_SMOKE_TEST.ko.md](RELEASE_SMOKE_TEST.ko.md)를 실행한다. Windows build,
display scale, shell, 설치/build path, commit과 실패한 checklist ID를 기록한다. 시각
검사와 실제 IME/clipboard 동작은 headless test로 면제하지 않는다.

## 6. 외부 릴리스 matrix

직접 증거가 생길 때까지 다음 항목은 `외부`다.

| 항목 | 필요한 증거 |
| --- | --- |
| Cold 시작 | 정확한 signed 후보를 통제된 재시작 뒤 측정한 표본 5개 |
| Uncached runner | `release_uncached_runner.tests.ps1`로 workload마다 서로 다른 최근 boot 표본 5개 |
| Windows 지원 | 현재와 직전 지원 Windows release |
| Hardware | 기준과 최소 지원 hardware tier |
| 서명 | 모든 실행 파일과 installer의 유효한 최종 Authenticode chain과 깨끗한 signature 검증 |
| 권한/session 범위 | 릴리스 약속에 포함한 elevated, 관리자, 다른 login session 또는 multi-user 동작 |

따뜻한 표본, compatibility mode 또는 다른 commit은 이 항목을 대신하지 않는다.

## 최종 수락 기록

Release commit마다 절별로 한 행을 보관한다.

```text
Release commit:
Artifact SHA-256:
Windows build와 hardware:
1 결정론적 source 게이트: 통과 / 실패
2 계약 coverage 감사: 통과 / 실패
3 artifact와 보안 게이트: 통과 / 실패
4 성능과 자원 게이트: 통과 / 실패
5 수동 앱 게이트: 통과 / 실패
6 외부 matrix: 통과 / 외부 / 실패
알려진 제한:
검토자와 날짜:
```

`외부`로 표시한 항목은 해당 릴리스 약속을 열린 상태로 남긴다. 조건부 통과가 아니다.
