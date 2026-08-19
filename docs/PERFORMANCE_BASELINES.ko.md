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
닫았다. 이 결과 하나만으로 전체 대용량 성능 gate를 통과한 것은 아니다.
Retained-memory 회수, 명시적 scrollback 상한, Windows Terminal과 일치시킨 경과 시간은
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
PowerShell 측정 seam을 닫았다. 별도 `cmd.exe` release matrix는 아직 남아 있다.

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

## 2026-08-12: 명시적 release scrollback 상한

- App source: `90bc57ec0a9d842eaf34c1220308347857a8d626`
- Harness 최초 도입: `cedbba0430615050f8825ec34f580a0ab5e533e3`
- Build와 machine: 앞선 대용량 출력 측정과 동일

실행 명령:

```powershell
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_scrollback_ceiling.tests.ps1 -Executable src-tauri/target/release/wingman.exe -TimeoutSeconds 60
```

Release 전용 probe는 검증된 100,000줄·11,900,000바이트 foreground PowerShell
workload를 그대로 사용한다. xterm이 전체 stream을 해석하고 끝 marker가 화면에 나타난
뒤 frontend가 설정된 scrollback, 활성 viewport 행 수, normal buffer 행 수를 보고한다.
Rust는 현재 세션, 정확한 probe opt-in, 설정된 P0 상한, 보존 행이 상한을 정확히 채운
buffer인 경우에만 보고를 받는다. Black-box harness도 같은 값을 독립적으로 요구하고
PowerShell PTY가 살아 있는지 확인한다.

처음 검토한 10,000줄 후보는 clear 후 idle 대비 54.66 MiB를 남겨 50 MiB release
상한을 넘었다. 5,000줄 후보는 한 번 48.33 MiB로 통과했지만 측정 여유가 부족했다.
따라서 P0는 이전 xterm 암묵적 기본값의 4배이면서 기존 memory release gate를
유지하는 4,000줄로 제한한다.

Release scrollback 테스트는 100,000줄 처리 후 정확히 4,000줄을 보존했다. 이 상한으로
다시 측정한 독립 retained-memory 분포 세 개는 다음과 같다.

| 독립 실행 | Idle median | Retained median | Retained maximum | 최대 증가 | 결과 |
| --- | ---: | ---: | ---: | ---: | --- |
| 1 | 148.43 MiB | 190.78 MiB | 193.52 MiB | 45.08 MiB | 상한 통과 |
| 2 | 148.19 MiB | 191.18 MiB | 195.00 MiB | 46.81 MiB | 상한 통과 |
| 3 | 150.62 MiB | 193.59 MiB | 197.66 MiB | 47.03 MiB | 상한 통과 |

Idle 원시 표본 (MiB):

```text
실행 1: 147.922, 147.809, 148.074, 148.266, 148.359, 148.508, 148.824, 149.055, 148.918, 149.109
실행 2: 147.668, 147.652, 147.770, 148.047, 148.094, 148.277, 148.613, 148.828, 148.730, 148.855
실행 3: 150.152, 150.055, 150.258, 150.488, 150.547, 150.699, 151.047, 151.203, 151.066, 151.270
```

Retained 원시 표본 (MiB):

```text
실행 1: 193.438, 193.438, 193.438, 193.516, 190.719, 190.781, 190.781, 190.766, 190.664, 190.730
실행 2: 194.992, 194.992, 194.996, 194.996, 191.137, 191.121, 191.184, 191.156, 191.176, 191.129
실행 3: 197.648, 197.656, 197.656, 197.656, 193.547, 193.555, 193.617, 193.539, 193.566, 193.555
```

세 실행 모두 GUI와 통합 PowerShell이 살아 있는 동안 retained-memory release 상한
50 MiB 아래를 유지했다. 명시적 scrollback 공백은 닫혔다. 재검증에서도 11,900,000
바이트를 5,105.0 ms에 모두 보존했고 대용량 출력 중 입력 latency는 median 41.8 ms,
p95 48.6 ms, maximum 50.8 ms였다. 같은 조건의 Windows Terminal 비교는 아래에서
측정한다.

## 2026-08-12: Windows Terminal 대량 render 일치 비교

- Harness 최초 도입: `8f004899b4207c3053a1050e8edf238160862fa3`
- Wingman app source: `90bc57ec0a9d842eaf34c1220308347857a8d626`
- Windows Terminal: `1.24.11911.0`, x64 stable package
- Windows: `10.0.26200.0`
- Windows PowerShell: `5.1.26100.8875`

실행 명령:

```powershell
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_bulk_host_comparison.tests.ps1 -WingmanExecutable src-tauri/target/release/wingman.exe -RunCount 3 -TimeoutSeconds 90
```

각 paired 실행은 같은 작업 디렉터리에서 새 host와 shell process를 시작하고 동일한
결정적 100,000줄·11,900,000바이트 PowerShell payload를 출력한다. 두 host 모두 탭
하나와 외곽 1,116 x 759 pixel 창을 사용한다. 비공개 임시 신호로 generator 시작 전에
Windows Terminal 창 크기를 맞추고 매 실행 뒤 신호를 삭제한다. 설치된 Windows
Terminal 기본 profile은 Windows PowerShell이며 history, font, 초기 열·행에 사용자
override가 없으므로 package의 history 기본값 9,001줄이 적용된다. Wingman은 명시적인
P0 상한 4,000줄을 사용한다.

시간은 host 실행 직전에 측정하기 시작한다. Wingman은 전체 stream hash와 byte 수를
검증하고 렌더된 xterm buffer에서 끝 marker를 찾은 뒤 animation frame 두 번을 기다려야
완료된다. Windows Terminal의 고정 완료 title은 flush된 payload 뒤에 console 순서로
적용되며, harness는 title을 본 뒤 성공한 DWM compositor flush 두 번을 기다린다. 두
완료 경계 모두 benchmark PowerShell process가 살아 있어야 한다.

| Paired 실행 | Wingman | Windows Terminal | Pair 비율 |
| --- | ---: | ---: | ---: |
| 1 | 4,807.9 ms | 3,869.3 ms | 1.2426x |
| 2 | 4,610.4 ms | 3,894.1 ms | 1.1840x |
| 3 | 4,712.2 ms | 3,877.3 ms | 1.2153x |
| Median | 4,712.2 ms | 3,877.3 ms | 1.2153x |

원시 경과 시간 표본 (ms):

```text
Wingman: 4807.884, 4610.408, 4712.232
Windows Terminal: 3869.302, 3894.059, 3877.279
```

Median 비율 1.2153x로 목표 2x와 release 상한 3x를 모두 통과했다. 사용자 Windows
Terminal 설정을 바꾸거나 benchmark process·신호 파일을 남기지 않고 이 장비의
Windows Terminal 대량 render 일치 비교 공백을 닫았다.

## 2026-08-12: warm-cache release runner timing

- Runner source: `d9ef6c557e31c9468d6df2ae41ab217be9ece4f6`
- Harness와 수락 목표: `dc46478fd6b4a6194fa5be8b41f55e70ad9b96db`
- OS: Microsoft Windows 11 Home `10.0.26200`
- CPU: AMD Ryzen 7 9700X, physical 8 core, logical processor 16개
- Power: Windows 균형 조정 (`381b4222-f694-41f0-9685-ff5bb260df2e`)
- Toolchain: `rustc 1.96.1`, `cargo 1.96.1`
- Build: 최적화된 Cargo `release`

실행 명령:

```powershell
cargo test --release --manifest-path src-tauri/Cargo.toml --test runner_performance_contract cached_runner_timing_baseline -- --ignored --exact --nocapture
```

각 독립 실행은 고정 128바이트 LF record 819,200개로 정확히 100 MiB인 UTF-8 corpus,
정확히 항목 20,000개인 tree, 역순의 고정 32바이트 sort record 200,000개가 든 비공개
sandbox를 만든다. 측정하지 않는 pass로 각 작업을 warm-up한다. 이어지는 세 timed pass는
실제 `wingman-runner`를 시작하고 one-shot broker로 typed request 하나를 가져와 데이터를
처리하며 결과와 종료 상태를 검증한 뒤 sandbox 전체를 제거한다.

`grep`은 100 MiB corpus 전체를 읽어 마지막 record의 고정 match 하나를 찾는다. `find`는
항목 20,000개를 모두 출력하고 검증한다. `cat`은 renderer 비용을 제외하도록 100 MiB
corpus의 정규화 출력을 redirect한다. `sort`는 200,000개 record를 모두 materialize하고
redirect한다. Corpus 생성, warm-up, 결과 검증, 정리는 기록 시간 밖이며 broker fetch,
process 시작, runner 검증, 파일 작업, 출력 완료, process 종료는 기록 시간 안이다.

| 독립 분포 | `grep` median | `find` median | Redirect `cat` median | Redirect `sort` median |
| --- | ---: | ---: | ---: | ---: |
| 1 | 825.8 ms | 442.8 ms | 3,070.8 ms | 663.2 ms |
| 2 | 832.3 ms | 439.0 ms | 3,081.2 ms | 653.7 ms |
| 3 | 818.7 ms | 442.8 ms | 3,093.1 ms | 654.7 ms |
| Outer median | 825.8 ms | 442.8 ms | 3,081.2 ms | 654.7 ms |
| 수락 목표 | 1,000 ms | 535 ms | 3,700 ms | 790 ms |

Outer-median throughput은 `grep` 121.09 MiB/s, `find` 45,166.9 entries/s, redirect
`cat` 32.45 MiB/s, redirect `sort` 305,483.5 records/s였다. 수락 목표는 첫 outer
median에 정책의 runner-throughput 회귀 조사선 20%를 더하고 올림한 값이다. 이제 ignored
release test가 이 목표를 검사한다.

원시 표본 (ms):

```text
분포 1
grep: 825.849, 827.105, 824.290
find: 446.221, 442.802, 436.071
redirect cat: 3121.691, 3061.706, 3070.828
redirect sort: 666.742, 663.203, 650.044

분포 2
grep: 813.251, 832.275, 838.572
find: 439.243, 438.975, 438.469
redirect cat: 3122.941, 3081.241, 3065.282
redirect sort: 652.718, 656.612, 653.664

분포 3
grep: 841.868, 812.706, 818.675
find: 438.766, 461.041, 442.828
redirect cat: 3143.331, 3087.476, 3093.148
redirect sort: 649.877, 656.727, 654.700
```

목표를 적용한 뒤 마지막 실행도 각각 median 813.5 ms, 438.6 ms, 3,057.6 ms,
649.6 ms로 통과했고 sandbox는 남지 않았다. 이 결과로 재현 가능한 warm-cache runner
timing seam을 닫았다. 이는 uncached 근거가 아니다. 진짜 uncached 분포에는 검증되지 않은
system-cache 비우기 대신 사전에 만든 corpus와 controlled restart가 필요하다.

## 2026-08-12: release runner `sort` 자원 상한

- Runner source: `d9ef6c557e31c9468d6df2ae41ab217be9ece4f6`
- Harness와 수락 상한: `f71b82a45ccf48356b92e15cebe9787170e1ebcc`
- OS, CPU, power plan, toolchain, release profile: 앞선 runner timing 기준과 동일

실행 명령:

```powershell
cargo test --release --manifest-path src-tauri/Cargo.toml --test runner_resource_contract sort_resource_limit_stays_bounded_and_fails_closed -- --ignored --exact --nocapture
```

각 독립 분포는 비공개 입력 두 개를 만든다. Byte-limit 입력은 text가 정확히 65,536
바이트인 record 1,024개로 retained sort text가 정확히 64 MiB다. 실제 broker와 release
runner가 모든 record를 수락하고 redirect한다. 이어서 같은 크기의 record 하나를 추가하고
fail-closed 거부를 요구한다. 두 번째 입력은 짧은 record 262,145개로 독립적인 262,144개
record 상한을 증명한다. 각 시나리오는 세 번 실행한다.

Parent는 `GetProcessMemoryInfo`로 `PrivateUsage`를 2 ms마다 표본화하며 최대 표본을 peak
private bytes로 보고한다. 같은 API의 process-lifetime `PeakWorkingSetSize`가 peak working
set 측정값이다. 둘 다 수락된 release 상한 96 MiB 이하여야 한다. Exact-limit 성공은
정규화된 출력 67,110,912바이트를 모두 쓴다. 두 초과 경우는 exit `1`, 고정된 55바이트
CRLF 진단 `wingman sort: materialization resource limit exceeded`만 출력하고 열린 redirect
target을 0바이트로 남긴다.

| 분포 | Exact 64 MiB peak WS | Exact 64 MiB peak private | Byte + 1 record peak WS | Byte + 1 record peak private | Count + 1 peak WS | Count + 1 peak private |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 69.47 MiB | 65.69 MiB | 69.46 MiB | 65.90 MiB | 18.95 MiB | 18.09 MiB |
| 2 | 69.48 MiB | 65.99 MiB | 69.45 MiB | 65.75 MiB | 18.96 MiB | 18.05 MiB |
| 3 | 69.50 MiB | 65.92 MiB | 69.46 MiB | 65.75 MiB | 18.96 MiB | 18.09 MiB |

원시 표본은 순서대로 경과 ms, peak working-set MiB, peak private-byte MiB다.

```text
분포 1
exact: (479.197, 69.426, 65.609), (461.989, 69.473, 65.637), (458.922, 69.465, 65.691)
byte + 1: (445.400, 69.461, 65.898), (454.205, 69.434, 65.559), (435.508, 69.438, 65.695)
count + 1: (38.185, 18.930, 18.016), (36.818, 18.949, 18.090), (37.897, 18.934, 18.020)

분포 2
exact: (461.897, 69.484, 65.758), (452.405, 69.477, 65.816), (456.404, 69.477, 65.988)
byte + 1: (447.168, 69.445, 65.684), (430.147, 69.449, 65.754), (447.632, 69.449, 65.555)
count + 1: (39.320, 18.957, 15.160), (40.400, 18.953, 18.047), (37.663, 18.957, 15.289)

분포 3
exact: (453.231, 69.496, 65.918), (468.619, 69.449, 65.914), (456.376, 69.445, 65.680)
byte + 1: (437.813, 69.465, 65.754), (429.688, 69.434, 65.555), (431.011, 69.438, 65.492)
count + 1: (39.006, 18.945, 18.035), (37.901, 18.953, 18.043), (37.025, 18.961, 18.094)
```

전체 최대값은 working set 69.50 MiB, private bytes 65.99 MiB였고 runner process 27개가
모두 release 상한 96 MiB를 통과했다. 부분 sorted record, 크기 무제한 진단, 살아남은
runner, resource sandbox는 없었다. 이 결과로 bounded `sort`의 release process-memory
근거를 닫았고 traversal, listing, mutation 자원 제한 경우는 별도 gate에서 측정한다.

## 2026-08-19: release traversal·listing 자원 상한

- Runner source: `b9b3510d5adb14dbac707839445733ddc16f687a`
- Harness와 수락 상한: `c2f8fae67b8c792297e10fe9fea8fdb355b7b5df`
- OS, CPU, 전원 설정, toolchain, release profile: 앞선 runner 기준과 동일

실행 명령:

```powershell
cargo test --release --manifest-path src-tauri/Cargo.toml --test runner_resource_contract traversal_and_listing_resource_limits_are_bounded -- --ignored --exact --nocapture
```

실제 broker와 release runner를 비공개 NTFS sandbox에서 실행했다. 하나의 평평한 tree로
`find` 방문 개체 정확히 100,000개와 재귀 `grep` directory entry 정확히 100,000개를
증명한 뒤 파일 하나를 추가해 두 거부 경우를 검사했다. 새 directory는 `ls` 항목
262,144개와 262,145개를 증명한다. 두 번째 listing directory는 727바이트 Unicode 이름
92,309개와 221바이트 이름 하나로 보관한 UTF-8 이름을 정확히 67,108,864바이트로
채운다. 마지막 이름 하나가 항목 수 상한보다 먼저 독립적인 이름 바이트 상한을 넘는다.
각 경우는 분포마다 release sidecar 세 개를 실행한다.

정확한계 성공은 redirect된 모든 CRLF record를 검증한다. 각 초과 경우는 exit `1`,
고정 크기 진단 하나로 끝나고 새 filesystem 작업을 시작하지 않는다. 미리 수집하는
`find`·`ls`는 넣어 둔 redirect target을 그대로 두며, 평평한 재귀 `grep`은 계약대로
target을 먼저 열고 빈 상태로 둔다. Process lifetime peak working set과 2 ms 간격으로
표본화한 peak private bytes는 각각 144 MiB 이하여야 한다.

아래 peak pair는 working-set/private-byte MiB다. 분포 1은 상한을 고른 진단 실행이고,
분포 2·3은 harness에서 상한을 강제했다. 세 분포 모두 최종 상한 아래다.

| 분포 | Find 정확한계 | Find +1 | 재귀 grep 정확한계 | 재귀 grep +1 |
| --- | ---: | ---: | ---: | ---: |
| 1 | 36.05 / 38.58 | 34.61 / 32.99 | 80.93 / 114.59 | 80.94 / 114.60 |
| 2 | 35.76 / 38.58 | 34.67 / 33.23 | 80.94 / 114.60 | 80.92 / 114.59 |
| 3 | 35.77 / 38.59 | 34.61 / 32.97 | 80.92 / 114.59 | 80.95 / 114.59 |

| 분포 | ls 262,144 | ls 262,145 | ls 이름 64 MiB | ls 이름 +1 |
| --- | ---: | ---: | ---: | ---: |
| 1 | 40.66 / 51.68 | 40.59 / 47.28 | 80.29 / 90.41 | 80.21 / 82.74 |
| 2 | 40.66 / 51.68 | 40.60 / 47.28 | 80.28 / 90.38 | 80.20 / 82.73 |
| 3 | 40.66 / 51.72 | 40.60 / 47.30 | 80.25 / 90.40 | 80.16 / 82.74 |

원시 표본은 순서대로 경과 ms, peak working-set MiB, peak private-byte MiB다.

```text
분포 1
find exact: (1704.420, 35.734, 38.582), (1608.360, 36.047, 38.484), (1670.427, 35.762, 38.484)
find +1: (67.504, 34.379, 32.992), (70.814, 34.559, 32.883), (68.376, 34.613, 32.297)
grep exact: (4198.368, 80.926, 114.586), (3995.311, 80.934, 114.590), (4209.800, 80.934, 114.590)
grep +1: (44.171, 80.941, 114.594), (44.356, 80.926, 114.602), (42.695, 80.941, 114.594)
ls entries exact: (6436.743, 40.656, 51.680), (6226.650, 40.656, 51.676), (6801.617, 40.656, 51.672)
ls entries +1: (5591.081, 40.594, 47.281), (5421.979, 40.590, 47.281), (5420.708, 40.547, 47.211)
ls names exact: (4253.949, 80.293, 90.379), (4118.229, 80.258, 90.414), (4135.006, 80.285, 90.379)
ls names +1: (3730.151, 80.188, 82.738), (3738.254, 80.156, 82.727), (3732.764, 80.207, 82.730)

분포 2
find exact: (1611.557, 35.742, 38.582), (1601.542, 35.758, 38.246), (1623.823, 35.730, 38.566)
find +1: (66.820, 34.672, 33.227), (68.493, 34.625, 32.629), (65.973, 34.617, 32.629)
grep exact: (4287.382, 80.926, 114.570), (3953.238, 80.938, 114.598), (3954.493, 80.906, 114.602)
grep +1: (42.503, 80.922, 114.594), (42.749, 80.898, 114.594), (42.673, 80.906, 114.582)
ls entries exact: (6205.780, 40.664, 51.680), (6199.050, 40.656, 51.672), (6162.222, 40.656, 51.676)
ls entries +1: (5565.393, 40.563, 47.215), (5397.553, 40.555, 47.242), (5412.735, 40.598, 47.277)
ls names exact: (4233.677, 80.277, 90.375), (4330.600, 80.277, 90.367), (4226.999, 80.273, 90.355)
ls names +1: (3756.382, 80.199, 82.727), (3979.974, 80.176, 82.727), (3750.843, 80.191, 82.711)

분포 3
find exact: (1604.332, 35.668, 38.574), (1579.947, 35.676, 38.234), (1548.413, 35.773, 38.586)
find +1: (67.401, 34.523, 32.223), (65.862, 34.328, 32.918), (65.806, 34.609, 32.973)
grep exact: (3933.848, 80.922, 114.590), (3885.300, 80.910, 114.586), (3894.936, 80.906, 114.594)
grep +1: (44.362, 80.914, 114.590), (42.881, 80.945, 114.574), (42.652, 80.891, 114.586)
ls entries exact: (6163.696, 40.660, 51.676), (6222.849, 40.660, 51.723), (6122.143, 40.656, 51.668)
ls entries +1: (5339.327, 40.602, 47.297), (5341.426, 40.598, 47.281), (5313.350, 40.543, 47.215)
ls names exact: (4153.648, 80.238, 90.402), (4353.721, 80.254, 90.363), (4153.380, 80.250, 90.375)
ls names +1: (3703.147, 80.156, 82.734), (3708.163, 80.164, 82.738), (3701.969, 79.531, 82.047)
```

전체 최대값은 working set 80.95 MiB, private bytes 114.61 MiB였다. Runner process
72개가 모두 144 MiB 아래였고, 모든 정확한계는 수락하고 모든 +1 경계는 거부했으며,
cleanup 뒤 runner process나 resource sandbox는 남지 않았다. 이 결과로 traversal과
listing release 자원 gate를 닫았고 mutation 자원 측정은 별도로 남아 있다.

## 2026-08-19: release mutation 자원 상한

- Runner source: `b9b3510d5adb14dbac707839445733ddc16f687a`
- Harness와 수락 상한: `dfb67530f85a10c1528b0a79d3b67e069282cd94`
- OS, CPU, 전원 설정, toolchain, release profile: 앞선 runner 기준과 동일

실행 명령:

```powershell
cargo test --release --manifest-path src-tauri/Cargo.toml --test runner_mutation_resource_contract mutation_resource_limits_are_bounded_and_atomic -- --ignored --exact --nocapture
```

실제 broker와 release runner는 먼저 `mkdir`와 `touch`를 경로 128개 wire 정확한계로
실행하고, 129개가 하나도 만들기 전에 거부되는지 증명한다. 비공개 평평한 tree 하나는
root와 파일 99,999개로 구성된다. 각 분포는 실제 같은-parent staging/commit 경로로 이
100,000개 tree를 세 번 복사하고, 완전한 destination을 실제 재귀 `rm`으로 각각 지운다.
그다음 같은 NTFS volume에서 source를 세 번 이동하며 runner process 사이에는 test
fixture만 원복한다. 파일 하나를 추가하면 재귀 `cp`, `mv`, `rm` 모두 global preflight에서
실패해야 한다.

모든 정확한계 작업은 완전한 최종 filesystem tree를 검증한다. 모든 +1 경우는 exit
`2`, 고정 크기 진단 하나, 변경되지 않은 source, destination 부재,
`.wingman-stage-*` artifact 부재를 검증한다. Process lifetime peak working set과 2 ms
표본 peak private bytes는 각각 80 MiB 이하여야 한다.

아래 peak pair는 working-set/private-byte MiB다. 분포 1에서 상한을 골랐고 분포 2·3은
이를 강제했다. 세 분포 모두 최종 상한 아래다.

| 분포 | mkdir 128 / +1 | touch 128 / +1 | cp 100k / +1 |
| --- | ---: | ---: | ---: |
| 1 | 4.62/0.87 · 4.45/0.74 | 4.60/0.87 · 4.45/0.72 | 36.19/42.12 · 26.73/25.37 |
| 2 | 4.59/0.86 · 4.45/0.72 | 4.60/0.87 · 4.45/0.70 | 36.20/42.15 · 26.76/25.38 |
| 3 | 4.59/0.86 · 4.46/0.71 | 4.62/0.87 · 4.45/0.70 | 36.18/42.16 · 26.78/25.45 |

| 분포 | mv 100k / +1 | rm 100k / +1 |
| --- | ---: | ---: |
| 1 | 32.96/38.04 · 26.75/25.38 | 52.98/58.35 · 49.86/49.17 |
| 2 | 33.09/38.19 · 26.81/25.45 | 53.00/58.37 · 49.87/49.17 |
| 3 | 33.10/38.19 · 26.82/25.46 | 53.01/58.36 · 49.86/49.17 |

원시 표본은 순서대로 경과 ms, peak working-set MiB, peak private-byte MiB다.

```text
분포 1
mkdir exact: (48.937, 4.598, 0.863), (38.456, 4.617, 0.871), (35.920, 4.594, 0.863)
mkdir +1: (7.914, 4.426, 0.703), (7.498, 4.449, 0.715), (7.076, 4.430, 0.738)
touch exact: (37.097, 4.602, 0.871), (39.123, 4.582, 0.867), (39.137, 4.602, 0.863)
touch +1: (8.175, 4.418, 0.715), (6.947, 4.449, 0.719), (7.773, 4.449, 0.723)
cp exact: (76553.940, 36.145, 42.043), (77061.022, 36.133, 42.051), (81915.964, 36.188, 42.121)
cp +1: (3279.188, 26.703, 25.359), (3230.340, 26.730, 25.367), (3203.067, 26.730, 25.371)
mv exact: (5485.596, 32.930, 38.027), (5457.134, 32.926, 38.020), (5466.160, 32.965, 38.043)
mv +1: (3802.266, 26.734, 25.363), (3613.037, 26.754, 25.375), (3592.775, 26.711, 25.352)
rm exact: (11833.502, 52.973, 58.324), (11717.701, 52.941, 58.348), (11966.245, 52.977, 58.352)
rm +1: (3639.559, 49.859, 49.172), (3604.628, 49.801, 49.141), (3783.326, 49.809, 49.172)

분포 2
mkdir exact: (42.121, 4.594, 0.863), (39.305, 4.594, 0.863), (39.247, 4.578, 0.859)
mkdir +1: (7.114, 4.430, 0.719), (7.252, 4.445, 0.703), (7.171, 4.426, 0.691)
touch exact: (43.435, 4.602, 0.867), (42.463, 4.594, 0.863), (40.691, 4.590, 0.859)
touch +1: (9.737, 4.449, 0.695), (7.128, 4.418, 0.688), (6.628, 4.441, 0.688)
cp exact: (68421.922, 36.168, 42.148), (94819.193, 36.031, 41.934), (62191.611, 36.199, 42.148)
cp +1: (3390.574, 26.750, 25.367), (3313.122, 26.762, 25.375), (3551.066, 26.723, 25.383)
mv exact: (5433.191, 33.039, 38.184), (5462.054, 33.043, 38.191), (5408.298, 33.086, 38.184)
mv +1: (4484.835, 26.746, 25.367), (3577.803, 26.754, 25.359), (3649.303, 26.809, 25.453)
rm exact: (12105.853, 52.949, 58.363), (11563.213, 53.004, 58.367), (11755.126, 52.945, 58.348)
rm +1: (3630.914, 49.867, 49.168), (3588.822, 49.828, 49.168), (3692.383, 49.844, 49.164)

분포 3
mkdir exact: (36.667, 4.594, 0.852), (35.738, 4.594, 0.863), (37.641, 4.594, 0.863)
mkdir +1: (7.323, 4.449, 0.715), (7.267, 4.434, 0.691), (6.842, 4.461, 0.715)
touch exact: (40.789, 4.594, 0.852), (37.360, 4.609, 0.871), (37.765, 4.621, 0.867)
touch +1: (7.973, 4.426, 0.695), (7.503, 4.449, 0.699), (6.878, 4.438, 0.695)
cp exact: (71058.632, 36.148, 42.164), (116181.171, 36.176, 42.148), (44308.381, 36.109, 42.043)
cp +1: (3243.085, 26.781, 25.449), (3220.411, 26.750, 25.359), (3218.458, 26.734, 25.355)
mv exact: (5709.724, 33.031, 38.113), (5353.394, 33.102, 38.191), (5317.231, 33.043, 38.109)
mv +1: (3692.203, 26.816, 25.461), (3501.324, 26.719, 25.371), (3512.441, 26.707, 25.359)
rm exact: (12314.285, 53.008, 58.355), (11735.352, 53.004, 58.355), (12841.725, 52.957, 58.355)
rm +1: (3546.170, 49.813, 49.145), (3799.951, 49.855, 49.160), (3823.587, 49.809, 49.172)
```

전체 최대값은 working set 53.01 MiB, private bytes 58.37 MiB였다. Runner process
90개가 모두 80 MiB 아래였다. 정확한계 작업은 filesystem 효과를 모두 완료했고, 모든
+1 경우는 atomic했으며 staging item, runner process, resource sandbox가 남지 않았다.
이 결과로 mutation release 자원 gate를 닫았다.

## 2026-08-19: controlled-restart runner harness

Windows system file cache를 비웠다고 주장하지 않는 재현 가능한 uncached runner
harness를 추가했다. Marker가 있는 영구 fixture 하나를 준비하고 boot 하나마다
`grep`, `find`, redirect `cat`, redirect `sort` 중 정확히 하나만 실행한다. Coordinator는
15분이 지난 boot와 같은 boot의 두 번째 표본을 거부하고 정확한 source commit과
Windows build를 로컬에 기록하며 operation마다 서로 다른 boot 5개가 모여야 분포를
출력한다.

```powershell
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_uncached_runner.tests.ps1 -Mode Prepare -FixtureRoot C:\wingman-perf\runner-uncached
# 통제된 재시작 뒤 operation 하나만 실행한다.
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_uncached_runner.tests.ps1 -Mode Sample -FixtureRoot C:\wingman-perf\runner-uncached -Operation grep
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_uncached_runner.tests.ps1 -Mode Validate -FixtureRoot C:\wingman-perf\runner-uncached
```

따뜻한 상태의 harness smoke로 100 MiB corpus를 통과하는 실제 release broker·runner
경로와 versioned result marker를 확인했다. 이 시간값은 의도적으로 버렸고 uncached
근거가 아니다. 아직 controlled-restart 표본을 저장소에 기록하지 않았으므로 uncached
target은 미설정이고 이 release gate는 열려 있다.

## 2026-08-19: 30분 release 내구성 workload

- App source: `dc593882fdaedcd4c8ecb3d4c9c1e396d79e41ba`
- 측정 harness: `9fd1e6844ef0ca2fc262abf170d42527d07e7d52`
- OS: Windows `10.0.26200.9168`, display version `25H2`
- CPU: AMD Ryzen 7 9700X, physical core 8개, logical processor 16개
- Power: Windows 균형 조정 (`381b4222-f694-41f0-9685-ff5bb260df2e`)
- WebView2 Runtime: `151.0.4129.93`
- Toolchain: `rustc 1.96.1`, `cargo 1.96.1`
- Build: 공식 Tauri release frontend, installer bundle 없음

실행 명령:

```powershell
npm run tauri build -- --no-bundle
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_endurance.tests.ps1 -Executable src-tauri/target/release/wingman.exe
```

Release 전용 probe는 처음 10초 동안 안정화를 기다린 뒤 30분 동안 실행했다. 각
사이클은 필요할 때 Familiar mode를 켜고 250줄을 렌더링한 뒤 clear하고, `tail -f`를
시작·취소하고, PTY 크기를 번갈아 바꾸고, Windows PowerShell을 재시작한 뒤 다음
단계로 가기 전에 인증된 editor readiness를 요구했다. 총 907사이클을 완료했다.
Harness는 10초마다 전체 process tree를 표본화했고, 안정화된 양 끝값은 250ms 간격
표본 5개의 중앙값으로 계산했다. Gate가 실패해도 0이 아닌 종료에 앞서 같은 versioned
원시 결과를 출력하므로 assertion 경계에서 근거를 잃지 않는다.

| 지표 | 결과 | Release 상한 |
| --- | ---: | ---: |
| 기준 private working set | 146.789 MiB | 해당 없음 |
| 최종 안정화 private working set | 175.090 MiB | 350 MiB |
| 증가량 | 28.301 MiB | 50 MiB |
| 증가율 | 19.280% | 20% |
| 남은 runner process | 0 | 0 |

최종 tree에는 `wingman.exe`, WebView2, PowerShell, `conhost.exe`만 있었다. 양 끝의
모든 표본은 내부적으로 안정적이었고 process는 성공 종료했다. 이 결과로 이 machine의
자동화된 PowerShell 내구성 자원 상한 검사를 닫는다. 별도의 입력 latency 분포나 현재
및 직전 Windows release matrix를 대신하는 결과는 아니다.

## 2026-08-19: 현재 release PowerShell·cmd 시작 및 cmd idle tree

- App source: `b2ef9f2a495b95c2717955c6d4fa712cb9f97109`
- 측정 harness: `ce809e49f0a39f3e326f7c86e7159756d0f01950`
- OS: Windows `10.0.26200.9168`, display version `25H2`, x64
- CPU: AMD Ryzen 7 9700X, physical core 8개, logical processor 16개
- 전원: Windows 균형 조정 (`381b4222-f694-41f0-9685-ff5bb260df2e`)
- WebView2 Runtime: `151.0.4129.93`
- Toolchain: `rustc 1.96.1`
- Build: 공식 Tauri release frontend, installer bundle 없음

두 shell 모두 environment로 명시적으로 켜야 하는 동일한 xterm 렌더링 완료 입력
marker를 사용했다. App은 공개 same-binary handoff와 명시적 `--shell` 선택을 거쳐
시작한다. Probe flag는 shell을 spawn하기 전에 제거한다. 일반 production 실행에서는
probe가 꺼져 있으며 cmd 시작 시 probe IPC도 수행하지 않는다.

```powershell
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_warm_startup_distribution.tests.ps1 -Executable src-tauri/target/release/wingman.exe -ShellKind powershell -WarmupCount 3 -SampleCount 20 -TimeoutSeconds 15
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_warm_startup_distribution.tests.ps1 -Executable src-tauri/target/release/wingman.exe -ShellKind cmd -WarmupCount 3 -SampleCount 20 -TimeoutSeconds 15
```

| Shell | 중앙값 | p95 | 최댓값 | 1.5초 상한 |
| --- | ---: | ---: | ---: | --- |
| Windows PowerShell 5.1 | 742.8 ms | 833.5 ms | 834.5 ms | 통과 |
| cmd.exe | 476.8 ms | 492.3 ms | 520.4 ms | 통과 |

PowerShell 원시 표본(ms):

```text
833.5, 801.8, 834.5, 727.3, 732.3, 712.1, 729.0, 773.9, 722.2, 793.4,
704.4, 786.1, 771.1, 802.7, 753.4, 708.7, 703.4, 708.6, 718.9, 810.3
```

cmd 원시 표본(ms):

```text
479.4, 489.3, 492.3, 475.2, 460.8, 479.3, 471.3, 478.0, 463.0, 485.0,
463.6, 491.9, 456.1, 479.7, 462.0, 475.6, 444.9, 475.0, 480.1, 520.4
```

cmd idle harness는 이후 10초 안정화를 기다리고 독립 실행 3회마다 1초 표본 10개를
전체 process tree에서 기록했다. 모든 tree에는 `wingman.exe`, WebView2, `cmd.exe`,
`conhost.exe`가 있었고 runner는 남아 있지 않았다.

| 실행 | CPU 중앙값 | CPU p95 | Private working set 중앙값 | Private working set 최댓값 | 결과 |
| --- | ---: | ---: | ---: | ---: | --- |
| 1 | 0.195% | 0.488% | 118.78 MiB | 119.66 MiB | 통과 |
| 2 | 0.098% | 0.391% | 120.51 MiB | 121.51 MiB | 통과 |
| 3 | 0.195% | 0.391% | 117.67 MiB | 118.71 MiB | 통과 |

모든 cmd 표본은 전체 tree의 release 상한인 CPU 중앙값 0.5%, CPU p95 2%, private
working set 350 MiB를 통과했다. 앞선 PowerShell 측정과 합치면 이 machine의 두 shell
warm-start 및 안정화 idle matrix가 닫힌다. Controlled-restart cold 표본과 별도의 지원
Windows version은 외부 matrix 증거로 남는다.

## 2026-08-20: 현재 릴리스 로컬 게이트 종합

이 절은 앞에서 `cmd.exe` 대량 출력 matrix가 남았다고 적은 과거 상태를 대체한다.
`4b9d1aaf081d3746966defc8b578c22ee1241b79`부터
`83d27cd098d867735dc1745d019a8afdfdb33048`까지 수집한 최종 로컬 측정을 한곳에
모았다. 환경은 Windows `10.0.26200.9168`(`25H2`, x64), AMD Ryzen 7 9700X,
logical processor 16개, Windows 균형 조정 전원, WebView2 `151.0.4129.93`,
Windows Terminal `1.24.11911.0`, Rust `1.96.1`로 유지됐다.

### Cached runner와 idle follow 게이트

```powershell
cargo test --release --manifest-path src-tauri/Cargo.toml --test runner_performance_contract cached_runner_timing_baseline -- --ignored --exact --nocapture
cargo test --release --manifest-path src-tauri/Cargo.toml --test runner_process_contract idle_tail_follow_runner_stays_below_the_cpu_ceiling -- --ignored --exact --nocapture
```

| Runner workload | 원시 표본 | 중앙값 | 목표 | 결과 |
| --- | --- | ---: | ---: | --- |
| `grep` raw 100 MiB | 836.124, 853.818, 821.350 ms | 836.124 ms | 1,000 ms | 통과 |
| `find` traversal | 466.515, 458.932, 459.499 ms | 459.499 ms | 535 ms | 통과 |
| redirect `cat` | 3,196.221, 3,150.363, 3,136.449 ms | 3,150.363 ms | 3,700 ms | 통과 |
| redirect `sort` | 675.790, 683.106, 671.270 ms | 675.790 ms | 790 ms | 통과 |

최적화한 `tail -n 0 -f` process의 1초 CPU 표본 10개는 중앙값과 p95가 모두
`0.000%`였다. Process group 취소 뒤에도 exit `130`, 빈 stdout과 stderr를 유지했다.

### 두 shell GUI 대량 출력 matrix

두 shell 모두 같은 release 전용 probe 경로와 100,000줄, 11,900,000바이트 workload를
사용했다. 모든 byte/hash 검증이 끝났고 xterm은 scrollback row를 정확히 4,000개
보존했으며 어느 표본도 release 상한을 넘지 않았다.

| Shell | 전체 render | 입력 latency 중앙값 / p95 / 최대 | 보존 중앙 증가량 | 보존 최대 증가량 | Scrollback |
| --- | ---: | ---: | ---: | ---: | ---: |
| Windows PowerShell 5.1 | 6,507.3 ms | 27.4 / 34.9 / 35.8 ms | 40.398 MiB | 44.471 MiB | 4,000 |
| `cmd.exe` | 4,769.6 ms | 26.9 / 34.9 / 35.3 ms | 43.104 MiB | 43.951 MiB | 4,000 |

PowerShell 보존 메모리 기준 표본은 `148.328, 148.230, 148.445, 148.637,
148.746, 148.914, 149.301, 149.402, 149.309, 149.574` MiB이고 보존 표본은
`193.238, 193.301, 193.301, 193.301, 188.895, 188.508, 189.563, 187.945,
188.309, 188.492` MiB였다. 기준 중앙값은 148.830 MiB, 보존 중앙값은
189.229 MiB, 보존 최댓값은 193.301 MiB였다.

`cmd.exe` 보존 메모리 기준 표본은 `116.594, 116.344, 116.344, 116.355,
116.348, 116.352, 116.352, 116.379, 116.367, 116.355` MiB이고 보존 표본은
`159.457, 159.457, 159.457, 159.457, 160.305, 159.992, 159.156, 159.199,
159.234, 159.273` MiB였다. 기준 중앙값은 116.354 MiB, 보존 중앙값은
159.457 MiB, 보존 최댓값은 160.305 MiB였다.

### Windows Terminal 짝 비교

```powershell
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_bulk_host_comparison.tests.ps1 -WingmanExecutable src-tauri/target/release/wingman.exe -ShellKind powershell
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_bulk_host_comparison.tests.ps1 -WingmanExecutable src-tauri/target/release/wingman.exe -ShellKind cmd
```

Harness는 정확하고 고유한 최상위 benchmark window 제목으로 자기 Windows Terminal
창을 식별하고 그 창만 닫았다. 이미 열려 있던 사용자 소유 Windows Terminal process를
재사용하거나 종료하지 않았다.

| Shell | Wingman 표본 / 중앙값 | Windows Terminal 표본 / 중앙값 | 비율 | 2배 목표 / 3배 상한 |
| --- | --- | --- | ---: | --- |
| Windows PowerShell 5.1 | 5,973.242, 5,947.354, 5,870.321 / 5,947.354 ms | 3,850.520, 3,632.636, 3,567.335 / 3,632.636 ms | 1.637배 | 통과 / 통과 |
| `cmd.exe` | 4,920.106, 4,604.803, 4,659.013 / 4,659.013 ms | 3,792.327, 3,544.846, 3,586.961 / 3,586.961 ms | 1.299배 | 통과 / 통과 |

### 설치 용량과 로컬 앱 데이터

최종 NSIS bundle은 설치, 재설치, launch contract, 제거 smoke를 완료했다. 설치 tree는
13,307,499바이트(12.69 MiB)로 60 MiB 상한을 통과했다. 별도의 격리한 WebView2
profile test는 interactive PowerShell PTY를 포함한 Wingman을 100번 실행하고 닫았다.
그 profile은 14,559,049바이트(13.885 MiB)로 100 MiB 상한을 통과했고, harness는
성공한 뒤 격리 profile을 제거한다.

```powershell
npm run test:installer
npm run test:app-data
```

### 외부 증거로 남는 항목

로컬 게이트는 이 machine과 현재 Windows release에 대해서만 닫혔다. 다음 항목은
의도적으로 **통과로 기록하지 않는다**.

- 통제된 재시작 뒤 cold-start 표본 5개
- workload마다 서로 다른 boot에서 얻은 uncached runner 표본 5개
- 별도의 직전 지원 Windows release
- 기준 또는 최소 hardware tier
- 릴리스 인증서를 사용한 최종 Authenticode 서명
- 최종 릴리스 범위가 요구하는 관리자, elevated shell, 다른 login session 또는
  multi-user matrix

이 항목들은 다른 machine state, OS 설치, 자격 증명 또는 명시적 권한이 필요하다.
따뜻한 결과와 로컬 smoke test로 대신하지 않는다.
