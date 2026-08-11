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
