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
