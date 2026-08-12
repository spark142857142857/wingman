# 성능 기준 측정 기록

영문판: [PERFORMANCE_BASELINES.md](PERFORMANCE_BASELINES.md)

이 문서는 재현 가능한 component 측정 결과를 보관한다. Component 결과는 필요한
검사이지만 [성능 예산](PERFORMANCE_BUDGET.ko.md)의 Wingman 전체 process-tree 배포
게이트를 대신하지 않는다.

## 2026-08-11: idle `tail -f` runner component

- Commit: `2901cba79180d8eab99cd5df1b41c2e110b7ef93`
- OS: Microsoft Windows 11 Home `10.0.26200` (build `26200`)
- CPU: AMD Ryzen 7 9700X, physical 8 core, logical processor 16개
- 전원: Windows 균형 조정 (`381b4222-f694-41f0-9685-ff5bb260df2e`)
- Toolchain: `rustc 1.96.1`, `cargo 1.96.1`
- Build: 최적화된 Cargo `release`

실행 명령:

```powershell
cargo test --release --test runner_process_contract idle_tail_follow_runner_stays_below_the_cpu_ceiling -- --ignored --exact --nocapture
```

Test는 실제 broker를 통해 `wingman-runner`를 시작하고 변경 없는 빈 파일에
`tail -n 0 -f`를 실행한다. 10초 안정화 뒤 1초 process CPU-time 표본 10개를 수집해
logical processor 16개 기준으로 정규화하고, 정상 process-group 취소를 보내 종료
`130`을 요구한다.

| 독립 실행 | Median CPU | p95 CPU | 결과 |
| --- | ---: | ---: | --- |
| 1 | 0.000% | 0.000% | 통과 |
| 2 | 0.000% | 0.000% | 통과 |
| 3 | 0.000% | 0.000% | 통과 |

모든 1초 delta가 process CPU-time 측정 해상도보다 작았다. 따라서 이 component는
배포 상한 median 0.5%, p95 2%를 통과한다. 전체 app·WebView2·PTY·shell·child process
tree는 reference matrix에서 별도 ETW/WPR 배포 측정을 수행해야 한다.

### 파일시스템 안전성 리팩터링 뒤 재검증

Transfer, 검증 경로, access-mode module 리팩터링 뒤 commit
`180e44d5343b72dc553f2a13400a4b48ac85a366`에서 같은 release test를 다시 실행했다.
OS, CPU, 전원 구성표, toolchain, build profile, 안정화 시간과 표본 절차는 동일했다.

| 독립 실행 | Median CPU | p95 CPU | 결과 |
| --- | ---: | ---: | --- |
| 1 | 0.000% | 0.000% | 통과 |
| 2 | 0.000% | 0.000% | 통과 |
| 3 | 0.000% | 0.098% | 통과 |

세 실행 모두 process-group 취소 뒤 `130`으로 종료했다. 재검증은 component 상한인
median 0.5%, p95 2%를 통과하며, 별도의 전체 process-tree 측정 요구는 바꾸지 않는다.

## 2026-08-12: 안정화된 release GUI process tree

- App source: `6ea7939cfd6bbd99739312d461e8d69c01018274`
- 측정 harness: `0f660cec116ad1f1d1d517f62f3119811d9efe02`
- OS: Microsoft Windows 11 Home `10.0.26200`
- CPU: AMD Ryzen 7 9700X, physical 8 core, logical processor 16개
- 전원: Windows 균형 조정 (`381b4222-f694-41f0-9685-ff5bb260df2e`)
- WebView2 Runtime: `151.0.4129.78`
- Toolchain: `rustc 1.96.1`, `cargo 1.96.1`
- Build: installer bundle을 제외한 공식 Tauri release frontend

실행 명령:

```powershell
npm run tauri build -- --no-bundle
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_process_tree.tests.ps1 -Executable src-tauri/target/release/wingman.exe
```

Harness는 실제 PowerShell PTY process를 기다린 뒤 10초 더 안정화한다. 각 표본마다
process tree를 재귀적으로 찾고, 1초 process CPU-time delta 10개를 logical processor
16개 기준으로 정규화하며, `Win32_PerfRawData_PerfProc_Process`에서 private working
set을 얻는다. 모든 실행은 `wingman.exe`, WebView2, PowerShell, `conhost.exe`를 포함한
9개 process를 안정적으로 유지했고 runner는 실행 중이지 않았다.

| 독립 실행 | Median CPU | p95 CPU | Median private working set | Maximum private working set | 결과 |
| --- | ---: | ---: | ---: | ---: | --- |
| 1 | 0.293% | 0.684% | 148.32 MiB | 149.05 MiB | 배포 상한 통과 |
| 2 | 0.439% | 0.684% | 151.78 MiB | 152.37 MiB | 배포 상한 통과 |
| 3 | 0.391% | 0.586% | 148.38 MiB | 149.07 MiB | 배포 상한 통과 |

Private working set은 목표 250 MiB와 배포 상한 350 MiB를 모두 통과했다. CPU p95는
목표 1%를 통과했지만 CPU median은 목표 0.2%에 미달했고, 세 실행 모두 배포 상한
0.5%는 통과했다. 이 logical processor 16개 환경의 1초 CPU 표본은 약 0.098%p
단위로 양자화된다. 진단용 total working set은 481.8~489.9 MiB, private bytes는
267.1~271.3 MiB였다.

이는 재현 가능한 black-box 자원 상한 사전 검사이지 완전한 release matrix 게이트는
아니다. 현재 셸 process 관측은 계약이 요구하는 prompt 준비 후 수락·echo probe가
아니고, 측정 PC는 최소 reference tier보다 빠르며, Windows PowerShell만 측정했다.
권한이 없는 환경에서 `WPR GeneralProfile` capture를 시도했지만 `0xc5585011`로
거부됐다. ETW/WPR 진단은 권한이 허용된 성능 측정 session의 남은 작업이다.

## 2026-08-12: 검증된 PowerShell editor readiness 사전 검사

- App source와 harness: `76211fd5b9112cfd72b4a9f6f3d6a9af2c0b5c0f`
- Build와 machine: 위의 안정화된 process-tree 측정과 같은 공식 Tauri release 및 환경

실행 명령:

```powershell
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_editor_readiness.tests.ps1 -Executable src-tauri/target/release/wingman.exe -TimeoutSeconds 30
```

Release harness는 실제 GUI를 시작하고 현재 session의 인증된 OOB PowerShell
readiness frame이 nonce, sequence, shell, depth, filesystem location,
PSReadLine adapter 검증을 통과할 때까지 기다린다. Rust는 그 뒤 ASCII native window
title `Wingman - Ready`를 노출하며, harness는 통합 PowerShell PTY child가 살아 있는지도
함께 요구한다.

| 독립 실행 | 검증된 editor readiness |
| --- | ---: |
| 1 | 6,418.2 ms |
| 2 | 6,439.0 ms |
| 3 | 6,232.5 ms |

이 marker는 계약의 수락·echo된 PTY probe보다 앞선 시점이므로 완전한 cold 또는 warm
launch 분포가 아니다. 그럼에도 세 lower-bound 측정이 이미 3.0초 hard launch 상한을
넘으므로 현재 환경에서 완전한 launch gate는 아직 통과할 수 없다. 다음 사전 검사에서
정상 입력 echo seam을 실행하며, 표준 3회 warmup+20회 warm 표본과 통제된 cold 표본
5회 측정은 아직 남아 있다.

## 2026-08-12: 렌더된 PowerShell 입력 echo 사전 검사

- App source: `b921e95de128dc30181940b791bed81e7386477e`
- Harness 최초 도입: `a9fca85`
- Build와 machine: 위의 안정화된 process-tree 측정과 같은 공식 Tauri release 및 환경

실행 명령:

```powershell
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_shell_echo.tests.ps1 -Executable src-tauri/target/release/wingman.exe -TimeoutSeconds 15
```

환경 플래그로만 켜지는 개발용 probe는 인증된 editor readiness를 기다린 뒤, xterm의
사용자 입력 event를 통해 무해한 고정 PowerShell 주석을 넣는다. 입력은 정상 Tauri 입력
command, Rust terminal session, PTY, PowerShell echo, PTY output event를 거쳐야 한다.
xterm이 ANSI stream을 해석하고 렌더된 terminal buffer에 token이 나타난 뒤 animation
frame 두 번이 지나야 완료로 기록한다. Probe 플래그는 child shell 환경에서 제거되며
일반 실행에서는 비활성이다.

| 연속 실행 | 수락되고 렌더된 입력 echo |
| --- | ---: |
| 1 | 6,441.1 ms |
| 2 | 6,416.1 ms |
| 3 | 6,207.4 ms |
| 4 | 6,203.0 ms |
| 5 | 6,226.9 ms |

Median은 6,226.9 ms이고 5회 모두 완료됐다. 이 결과로 누락됐던 측정 seam을 닫고
계약의 startup 완료 경계를 직접 검증하지만, 반복성 사전 검사일 뿐이다. 표준 warm
분포는 아래에 기록하며 통제된 5회 cold 분포는 아직 남아 있다. 모든 표본이 cold hard
ceiling 3.0초와 warm hard ceiling 1.5초를 넘으므로, 현재 환경에서 startup 성능은
계속 release blocker다.

## 2026-08-12: 표준화된 warm PowerShell startup 분포

- App source: `b921e95de128dc30181940b791bed81e7386477e`
- 분포 harness: `6983b2c`
- Build와 machine: 렌더된 입력 echo 사전 검사와 동일

실행 명령:

```powershell
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_warm_startup_distribution.tests.ps1 -Executable src-tauri/target/release/wingman.exe -WarmupCount 3 -SampleCount 20 -TimeoutSeconds 15
```

3회 warmup은 6,248.3, 6,247.0, 6,256.9 ms였다. 기록한 20회의 수락·렌더 입력 echo
표본은 다음과 같다.

```text
6237.3, 6227.0, 6182.2, 6156.8, 6241.8, 6210.0, 6217.2, 6243.8, 6216.3, 6217.6,
6211.5, 6208.5, 6242.1, 6216.0, 6246.9, 6265.4, 6190.3, 6204.9, 6226.4, 6242.6
```

| 통계 | Warm startup |
| --- | ---: |
| Median | 6,217.4 ms |
| p95 (nearest-rank) | 6,246.9 ms |
| Maximum | 6,265.4 ms |

기록한 20회는 모두 완료됐지만 모든 실행이 warm hard ceiling 1.5초를 4배 넘게
초과했다. 따라서 warm startup gate는 실패한다. Cold-cache 측정과 ETW 원인 귀속은
별도 후속 작업이며, 어느 쪽도 이미 관측된 warm hard-ceiling 실패를 바꾸지는 않는다.

### 중복 startup cwd probe 제거 후 재검증

Commit `8ff27ae38f3f33c8133546029c97a6a985732937` 뒤 같은 release build와 3+20
절차를 다시 실행했다. Startup은 실제 PTY 전에 cwd 확인용 PowerShell을 별도로 동기
실행하고 이후 cwd를 다시 조회하는 대신, `start_shell`이 반환한 cwd를 바로 사용한다.

3회 warmup은 830.2, 789.1, 775.4 ms였다. 기록한 20회 표본은 다음과 같다.

```text
795.5, 813.0, 729.7, 761.4, 728.3, 782.3, 792.8, 791.9, 813.0, 765.1,
804.9, 782.4, 783.3, 775.9, 778.1, 750.9, 819.2, 749.1, 799.3, 804.0
```

| 통계 | 수정 전 | 수정 후 | 변화 |
| --- | ---: | ---: | ---: |
| Median | 6,217.4 ms | 782.9 ms | -87.4% |
| p95 (nearest-rank) | 6,246.9 ms | 813.0 ms | -87.0% |
| Maximum | 6,265.4 ms | 819.2 ms | -86.9% |

20회 모두 warm hard ceiling 1.5초를 통과한다. Median은 목표 0.8초 아래지만 p95는
목표를 13.0 ms 초과하므로, 추가 개선은 측정 없는 재작성보다 ETW 원인 귀속을 먼저
해야 한다. 통제된 5회 cold 분포는 아직 남아 있다.

## 2026-08-12: 결정적 release PTY 대용량 렌더 사전 검사

- App source: `8b069e849b3a97100f54073655147f1206574ff1`
- Harness 최초 도입: `2eb44806bec63152ce319112d15d3c32e3c3d5e3`
- Build와 machine: 최적화된 warm startup 재검증과 동일

실행 명령:

```powershell
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_bulk_output.tests.ps1 -Executable src-tauri/target/release/wingman.exe -TimeoutSeconds 30
```

환경 플래그로만 켜지는 개발용 probe는 xterm의 정상 사용자 입력 event를 통해 고정된
native PowerShell generator 하나를 제출한다. Generator는 `é` 55개를 포함하는 순서가
고정된 100,000줄, 정확히 11,900,000 logical UTF-8 data bytes를 출력한다. Frontend는
marker 크기의 carry state만 유지하고 검증 stream에서 ConPTY의 VT screen-update
sequence를 제거한 뒤, 독립적으로 고정한 FNV-1a hash와 정확한 UTF-8 길이로 전체
payload를 검증한다. 수정하지 않은 원본 stream은 그대로 xterm에 전달하며, 렌더된
terminal buffer에 end marker가 나타나고 animation frame 두 번이 지난 뒤에만 완료를
노출한다. Probe 플래그는 PowerShell child 환경에서 제거된다.

| 독립 실행 | 시작부터 검증된 최종 렌더까지 |
| --- | ---: |
| 1 | 4,566.2 ms |
| 2 | 4,892.5 ms |
| 3 | 4,508.7 ms |

Median은 4,566.2 ms이고 세 실행 모두 모든 순서 줄을 보존했으며 GUI와 통합
PowerShell process가 살아 있었다. 이 결과로 결정적 100,000줄/10MiB 완전성 seam을
닫았다. 다만 Windows Terminal과 일치시킨 경과 시간과 명시적 scrollback 상한 측정은
아직 남아 있으므로 완전한 대용량 성능 gate 통과 결과는 아니다. Retained-memory 회수는
아래에서 측정한다.

## 2026-08-12: release 대용량 출력 중 입력 latency 분포

- App source: `6089a96cb5aab99797852ddd36dbe13b22752e49`
- Harness 최초 도입: `3a40d001ecd8d8a5f54944830efceff06c53f34a`
- Build와 machine: 결정적 대용량 렌더 사전 검사와 동일

실행 명령:

```powershell
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_bulk_input_latency.tests.ps1 -Executable src-tauri/target/release/wingman.exe -TimeoutSeconds 60
```

환경 플래그로만 켜지는 probe는 같은 100,000줄·11,900,000바이트 PowerShell workload를
순서가 고정된 1,000줄 묶음 100개로 나눈다. 각 표본 경계에서 frontend는 xterm의 정상
사용자 입력 event로 고정 문자 하나를 보내기 직전에 monotonic timestamp를 기록한다.
문자는 정상 Tauri command, Rust terminal session, PTY, PowerShell console input을
통과한다. PowerShell은 화면에 문자를 되비추지 않고 소비한 뒤 번호가 붙은 응답 marker를
출력하며, xterm이 해당 PTY chunk를 해석하고 animation frame 두 번이 지난 뒤에만 완료
시간을 기록한다. 그 뒤 다음 출력 묶음이 이어진다. 번호 marker가 누락·중복·순서 위반을
fail-closed 처리한다. Rust는 유한하고 범위 안인 표본 정확히 100개만 받아 통계를 독립
계산하고, 로컬 black-box harness에 원시 분포를 노출한다. Probe 플래그는 PowerShell
child 환경에서 제거된다.

| 독립 실행 | Median | p95 (nearest-rank) | Maximum | 결과 |
| --- | ---: | ---: | ---: | --- |
| 1 | 41.0 ms | 47.8 ms | 48.6 ms | 통과 |
| 2 | 40.6 ms | 48.4 ms | 48.9 ms | 통과 |
| 3 | 41.1 ms | 48.1 ms | 48.8 ms | 통과 |

원시 표본, 실행 1 (ms):

```text
43.2, 47.2, 32.0, 37.2, 42.6, 47.0, 35.4, 40.1, 44.3, 31.8,
37.8, 41.4, 46.3, 33.9, 37.9, 42.0, 46.9, 35.5, 40.1, 42.9,
46.9, 35.8, 39.3, 43.6, 46.5, 33.7, 37.5, 41.8, 46.5, 33.8,
38.6, 42.1, 46.9, 34.3, 37.8, 40.8, 44.3, 48.6, 37.0, 41.6,
45.7, 32.4, 37.5, 42.6, 46.2, 33.9, 38.0, 42.5, 46.8, 34.9,
39.4, 42.0, 46.5, 34.6, 38.8, 43.1, 47.8, 36.0, 40.6, 43.9,
46.9, 31.9, 37.4, 42.5, 47.9, 36.1, 41.2, 46.2, 35.2, 39.9,
44.2, 47.8, 35.8, 39.3, 42.8, 47.2, 35.6, 40.4, 45.0, 47.8,
36.1, 39.6, 44.1, 48.0, 36.9, 41.7, 46.7, 32.3, 38.6, 42.3,
45.6, 34.3, 39.6, 45.4, 32.2, 37.1, 42.6, 47.1, 35.2, 40.4
```

원시 표본, 실행 2 (ms):

```text
41.3, 45.3, 32.3, 37.2, 40.1, 44.7, 32.1, 37.2, 41.5, 44.0,
48.7, 36.2, 41.8, 45.8, 32.2, 37.5, 42.1, 46.6, 35.2, 39.2,
43.7, 48.1, 34.7, 39.1, 44.3, 48.3, 35.5, 39.1, 43.7, 48.4,
35.8, 39.8, 45.4, 32.1, 37.6, 41.2, 46.0, 48.8, 36.5, 39.6,
43.6, 48.4, 35.6, 40.2, 43.6, 48.9, 35.6, 40.0, 43.8, 48.3,
36.3, 41.5, 45.6, 34.5, 38.1, 42.3, 46.7, 48.8, 37.8, 42.5,
46.8, 34.3, 39.8, 45.7, 31.8, 38.0, 42.6, 46.5, 35.4, 39.0,
43.7, 32.0, 36.2, 41.1, 45.8, 32.1, 37.4, 40.7, 45.7, 33.9,
38.1, 43.2, 46.8, 35.0, 39.0, 46.0, 43.2, 36.1, 42.6, 46.2,
33.9, 37.7, 41.2, 45.7, 31.9, 37.3, 40.4, 43.6, 34.1, 38.3
```

원시 표본, 실행 3 (ms):

```text
46.1, 32.3, 36.2, 39.2, 43.4, 48.7, 36.1, 40.2, 43.9, 34.1,
39.0, 43.8, 46.4, 32.3, 38.7, 43.3, 47.4, 34.7, 39.7, 43.0,
46.7, 35.5, 38.3, 42.5, 47.4, 35.2, 38.7, 42.3, 45.5, 32.2,
36.9, 41.3, 46.0, 35.2, 40.8, 48.1, 35.0, 39.4, 43.8, 31.8,
38.2, 42.0, 44.8, 31.6, 37.8, 42.2, 46.3, 35.8, 40.1, 43.0,
47.5, 32.0, 38.3, 43.2, 47.7, 35.6, 38.5, 42.6, 46.9, 34.8,
39.7, 43.8, 48.3, 36.6, 42.2, 45.8, 34.3, 37.7, 42.3, 46.6,
33.8, 37.8, 43.3, 47.5, 35.4, 39.7, 43.3, 48.0, 35.4, 39.9,
42.8, 47.5, 35.4, 39.6, 44.5, 48.8, 36.8, 41.9, 45.8, 34.1,
37.7, 41.5, 45.0, 48.6, 37.1, 40.6, 43.9, 48.7, 37.5, 41.7
```

세 독립 분포 모두 대용량 출력 중 입력 latency p95 상한 200 ms를 4배가 넘는 여유로
통과했고 GUI와 통합 PowerShell process도 살아 있었다. 이 결과로 현재 장비의
PowerShell 측정 seam을 닫았다. 같은 조건의 Windows Terminal 비교, scrollback 상한,
별도 `cmd.exe` release matrix는 아직 남아 있다.

## 2026-08-12: release 대용량 출력 후 retained-memory 분포

- App source: `d1415336dcec620b3a6e3e8d00d38a3cd9a07f54`
- Harness 최초 도입: `a04fa4e464b58cd223ec57f11e64f6160394803b`
- Process별 진단 추가: `7447aac59fc1ecd5082d37b8bdccaff542dba9d9`
- Build와 machine: 앞선 대용량 출력 측정과 동일

실행 명령:

```powershell
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_bulk_retained_memory.tests.ps1 -Executable src-tauri/target/release/wingman.exe -PhaseTimeoutSeconds 90
```

Black-box harness는 10초 idle 안정화 후 전체 process tree private working set을 1초
간격으로 10회 기록한다. Probe는 검증된 100,000줄·11,900,000바이트 generator를
foreground child PowerShell로 실행하고 전체 stream을 렌더·검증한다. 이어서 정상
xterm/Tauri/Rust/PTY 입력 경로로 `Clear-Host`를 제출하고 고정 marker가 렌더된 뒤에만
완료를 노출한다. 다시 10초 안정화한 후 retained 표본 10개를 기록한다. 이때 child
generator는 종료됐고 GUI와 통합 PowerShell은 살아 있어야 한다.

첫 구현은 출력 backpressure가 없었다. 표준 진단 실행은 idle median 147.68 MiB에서
최대 280.42 MiB를 남겨 132.74 MiB 증가로 hard ceiling에 실패했다. 짧은 process별
진단에서 증가 대부분이 WebView renderer에 집중됐다. 수정 후 모든 PTY chunk에 sequence를
붙이고 xterm이 앞 chunk의 해석 완료를 ACK한 뒤에만 Rust reader가 다음 chunk를 전달한다.
Session 교체는 이 flow를 닫아 대기 중인 이전 reader를 깨운다. 결정적 generator를
foreground child로 옮겨 장기 실행 PowerShell에 generator heap이 남는 것도 막았다.

| 독립 실행 | Idle median | Retained median | Retained maximum | 최대 증가 | 결과 |
| --- | ---: | ---: | ---: | ---: | --- |
| 1 | 146.99 MiB | 187.28 MiB | 187.30 MiB | 40.31 MiB | 상한 통과 |
| 2 | 148.66 MiB | 187.66 MiB | 191.55 MiB | 42.89 MiB | 상한 통과 |
| 3 | 147.78 MiB | 184.88 MiB | 187.62 MiB | 39.84 MiB | 상한 통과 |

Idle 원시 표본 (MiB):

```text
실행 1: 147.207, 146.992, 146.992, 146.992, 146.977, 146.980, 146.980, 146.996, 146.758, 146.738
실행 2: 147.688, 147.664, 148.246, 148.469, 148.566, 148.762, 149.102, 149.203, 149.086, 149.238
실행 3: 147.250, 147.215, 147.328, 147.613, 147.699, 147.859, 148.230, 148.387, 148.250, 148.406
```

Retained 원시 표본 (MiB):

```text
실행 1: 187.301, 187.301, 187.301, 187.301, 187.281, 187.281, 187.281, 187.281, 187.281, 187.195
실행 2: 191.488, 191.551, 191.555, 187.598, 187.656, 187.664, 187.664, 187.453, 187.469, 187.379
실행 3: 187.621, 187.621, 187.621, 187.621, 184.844, 184.852, 184.914, 184.602, 184.523, 184.594
```

세 실행 모두 absolute 350 MiB 상한과 상대 증가 50 MiB retained-memory 배포 상한을
통과했다. 25 MiB 목표는 14.84~17.89 MiB 초과하므로 renderer 할당 추가 개선은 P0
release blocker가 아니라 최적화 기회로 남는다. 재검증에서도 11.9 MB stream을
4,819.2 ms에 보존했고 입력 latency는 median 41.8 ms, p95 48.7 ms, maximum 50.0 ms였다.
