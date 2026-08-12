# Performance Baselines

Korean version: [PERFORMANCE_BASELINES.ko.md](PERFORMANCE_BASELINES.ko.md)

This file stores reproducible component measurements. A component result is a
necessary check, not a substitute for the whole Wingman process-tree release
gate in [PERFORMANCE_BUDGET.md](PERFORMANCE_BUDGET.md).

## 2026-08-11: idle `tail -f` runner component

- Commit: `2901cba79180d8eab99cd5df1b41c2e110b7ef93`
- OS: Microsoft Windows 11 Home `10.0.26200` (build `26200`)
- CPU: AMD Ryzen 7 9700X, 8 physical cores, 16 logical processors
- Power: Windows Balanced (`381b4222-f694-41f0-9685-ff5bb260df2e`)
- Toolchain: `rustc 1.96.1`, `cargo 1.96.1`
- Build: optimized Cargo `release`

Command:

```powershell
cargo test --release --test runner_process_contract idle_tail_follow_runner_stays_below_the_cpu_ceiling -- --ignored --exact --nocapture
```

The test starts the real `wingman-runner` through its broker with
`tail -n 0 -f` on an unchanged empty file. It waits 10 seconds, records ten
one-second process CPU-time samples, normalizes by 16 logical processors, then
sends the normal process-group cancellation and requires exit `130`.

| Independent run | Median CPU | p95 CPU | Result |
| --- | ---: | ---: | --- |
| 1 | 0.000% | 0.000% | Pass |
| 2 | 0.000% | 0.000% | Pass |
| 3 | 0.000% | 0.000% | Pass |

All one-second deltas were below the process CPU-time measurement resolution.
The component therefore passes the release ceilings of median 0.5% and p95 2%.
The full app, WebView2, PTY, shell, and child-process tree still requires the
separate ETW/WPR release measurement on the reference matrix.

### Revalidation after filesystem safety refactoring

The same release test was repeated at commit
`180e44d5343b72dc553f2a13400a4b48ac85a366` after the transfer, verified-path,
and access-mode modules were refactored. The OS, CPU, power plan, toolchain,
build profile, settle period, and sample procedure were unchanged.

| Independent run | Median CPU | p95 CPU | Result |
| --- | ---: | ---: | --- |
| 1 | 0.000% | 0.000% | Pass |
| 2 | 0.000% | 0.000% | Pass |
| 3 | 0.000% | 0.098% | Pass |

All three runs exited `130` after process-group cancellation. The revalidation
passes the component ceilings of median 0.5% and p95 2%; it does not change the
separate whole-process-tree requirement.

## 2026-08-12: settled release GUI process tree

- App source: `6ea7939cfd6bbd99739312d461e8d69c01018274`
- Measurement harness: `0f660cec116ad1f1d1d517f62f3119811d9efe02`
- OS: Microsoft Windows 11 Home `10.0.26200`
- CPU: AMD Ryzen 7 9700X, 8 physical cores, 16 logical processors
- Power: Windows Balanced (`381b4222-f694-41f0-9685-ff5bb260df2e`)
- WebView2 Runtime: `151.0.4129.78`
- Toolchain: `rustc 1.96.1`, `cargo 1.96.1`
- Build: official Tauri release frontend, no installer bundle

Commands:

```powershell
npm run tauri build -- --no-bundle
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_process_tree.tests.ps1 -Executable src-tauri/target/release/wingman.exe
```

The harness waits for the real PowerShell PTY process, then waits another 10
seconds. It recursively discovers the process tree on every sample, records ten
one-second CPU-time deltas normalized by 16 logical processors, and obtains
private working set from `Win32_PerfRawData_PerfProc_Process`. Every run held a
stable nine-process tree containing `wingman.exe`, WebView2, PowerShell, and
`conhost.exe`; no runner was alive.

| Independent run | Median CPU | p95 CPU | Median private working set | Maximum private working set | Result |
| --- | ---: | ---: | ---: | ---: | --- |
| 1 | 0.293% | 0.684% | 148.32 MiB | 149.05 MiB | Release ceiling pass |
| 2 | 0.439% | 0.684% | 151.78 MiB | 152.37 MiB | Release ceiling pass |
| 3 | 0.391% | 0.586% | 148.38 MiB | 149.07 MiB | Release ceiling pass |

Private working set passed both the 250 MiB target and 350 MiB release ceiling.
CPU p95 passed the 1% target, while CPU median missed the 0.2% target but passed
the 0.5% release ceiling in all three runs. The one-second CPU samples are
quantized in approximately 0.098 percentage-point steps on this 16-logical-CPU
machine. Diagnostic total working set ranged from 481.8 to 489.9 MiB and private
bytes from 267.1 to 271.3 MiB.

This is a reproducible black-box resource-ceiling precheck, not the complete
release matrix gate. The current shell-process observation is not the contract's
authoritative accepted-and-echoed prompt probe, the machine is faster than the
minimum reference tier, and only Windows PowerShell was measured. An attempted
`WPR GeneralProfile` capture was rejected by the unelevated environment with
`0xc5585011`; ETW/WPR diagnosis remains for an authorized performance session.
