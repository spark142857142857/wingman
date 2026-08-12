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

## 2026-08-12: verified PowerShell editor readiness precheck

- App source and harness: `76211fd5b9112cfd72b4a9f6f3d6a9af2c0b5c0f`
- Build and machine: the same official Tauri release and environment as the
  settled process-tree measurement above

Command:

```powershell
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_editor_readiness.tests.ps1 -Executable src-tauri/target/release/wingman.exe -TimeoutSeconds 30
```

The release harness starts the real GUI and waits for the current session's
authenticated OOB PowerShell readiness frame to pass nonce, sequence, shell,
depth, filesystem-location, and PSReadLine-adapter validation. Rust then exposes
the ASCII native-window title `Wingman - Ready`; the harness additionally
requires the integrated PowerShell PTY child to be alive.

| Independent run | Verified editor readiness |
| --- | ---: |
| 1 | 6,418.2 ms |
| 2 | 6,439.0 ms |
| 3 | 6,232.5 ms |

This marker is earlier than the contract's accepted-and-echoed PTY probe, so it
is not a complete cold or warm launch distribution. Nevertheless, all three
lower-bound measurements already exceed the 3.0-second hard launch ceiling;
the complete launch gate therefore cannot pass on this environment yet. The
normal-input echo seam is exercised in the next precheck; the standard three
warmups plus 20 warm samples and five controlled cold samples remain pending.

## 2026-08-12: rendered PowerShell input-echo precheck

- App source: `b921e95de128dc30181940b791bed81e7386477e`
- Harness introduced at: `a9fca85`
- Build and machine: the same official Tauri release and environment as the
  settled process-tree measurement above

Command:

```powershell
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_shell_echo.tests.ps1 -Executable src-tauri/target/release/wingman.exe -TimeoutSeconds 15
```

The environment-gated development probe waits for authenticated editor
readiness, injects a fixed harmless PowerShell comment through xterm's user
input event, and requires it to travel through the normal Tauri input command,
Rust terminal session, PTY, PowerShell echo, and PTY output event. Completion is
reported only after xterm has parsed the ANSI stream, the token is present in
the rendered terminal buffer, and two animation frames have elapsed. The probe
flag is removed from the child-shell environment and is inactive in ordinary
launches.

| Consecutive run | Accepted and rendered input echo |
| --- | ---: |
| 1 | 6,441.1 ms |
| 2 | 6,416.1 ms |
| 3 | 6,207.4 ms |
| 4 | 6,203.0 ms |
| 5 | 6,226.9 ms |

The median was 6,226.9 ms and all five runs completed. This closes the missing
measurement seam and directly exercises the contract's startup-completion
boundary, but it is still a repeatability precheck. The standardized warm
distribution is recorded below; the controlled five-cold distribution remains
pending. Every sample exceeds the 3.0-second cold hard ceiling and the
1.5-second warm hard ceiling, so startup performance remains a release blocker
on this environment.

## 2026-08-12: standardized warm PowerShell startup distribution

- App source: `b921e95de128dc30181940b791bed81e7386477e`
- Distribution harness: `6983b2c`
- Build and machine: unchanged from the rendered input-echo precheck

Command:

```powershell
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_warm_startup_distribution.tests.ps1 -Executable src-tauri/target/release/wingman.exe -WarmupCount 3 -SampleCount 20 -TimeoutSeconds 15
```

The three warmups were 6,248.3, 6,247.0, and 6,256.9 ms. The 20 recorded
accepted-and-rendered input-echo samples were:

```text
6237.3, 6227.0, 6182.2, 6156.8, 6241.8, 6210.0, 6217.2, 6243.8, 6216.3, 6217.6,
6211.5, 6208.5, 6242.1, 6216.0, 6246.9, 6265.4, 6190.3, 6204.9, 6226.4, 6242.6
```

| Statistic | Warm startup |
| --- | ---: |
| Median | 6,217.4 ms |
| p95 (nearest-rank) | 6,246.9 ms |
| Maximum | 6,265.4 ms |

All 20 recorded runs completed, but every run exceeded the 1.5-second warm
hard ceiling by more than four times. The warm startup gate therefore fails.
Cold-cache measurement and ETW attribution remain separate follow-up work;
neither can change this already-observed warm hard-ceiling failure.

### Revalidation after removing redundant startup cwd probes

The same release build and 3+20 procedure was repeated after commit
`8ff27ae38f3f33c8133546029c97a6a985732937`. Startup now uses the cwd returned
by `start_shell` instead of synchronously spawning a separate PowerShell cwd
probe before the real PTY and then querying cwd again afterward.

The three warmups were 830.2, 789.1, and 775.4 ms. The 20 recorded samples were:

```text
795.5, 813.0, 729.7, 761.4, 728.3, 782.3, 792.8, 791.9, 813.0, 765.1,
804.9, 782.4, 783.3, 775.9, 778.1, 750.9, 819.2, 749.1, 799.3, 804.0
```

| Statistic | Before | After | Change |
| --- | ---: | ---: | ---: |
| Median | 6,217.4 ms | 782.9 ms | -87.4% |
| p95 (nearest-rank) | 6,246.9 ms | 813.0 ms | -87.0% |
| Maximum | 6,265.4 ms | 819.2 ms | -86.9% |

All 20 runs now pass the 1.5-second warm hard ceiling. The median is below the
0.8-second target, while p95 misses that target by 13.0 ms; further attribution
should use ETW rather than another unprofiled rewrite. The controlled five-cold
distribution is still pending.

## 2026-08-12: deterministic release PTY bulk-render precheck

- App source: `8b069e849b3a97100f54073655147f1206574ff1`
- Harness introduced at: `2eb44806bec63152ce319112d15d3c32e3c3d5e3`
- Build and machine: unchanged from the optimized warm-startup revalidation

Command:

```powershell
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_bulk_output.tests.ps1 -Executable src-tauri/target/release/wingman.exe -TimeoutSeconds 30
```

The environment-gated development probe submits one fixed native PowerShell
generator through xterm's normal user-input event. It emits 100,000 ordered
lines containing 55 copies of `é`, for exactly 11,900,000 logical UTF-8 data
bytes. The frontend retains only marker-sized carry state, removes ConPTY's VT
screen-update sequences from the verification stream, and validates the full
payload with an independently pinned FNV-1a hash and exact UTF-8 length. The
unmodified stream still goes to xterm, and completion is exposed only after the
end marker is present in the rendered terminal buffer and two animation frames
have elapsed. The probe flag is removed from the PowerShell child environment.

| Independent run | Launch through validated final render |
| --- | ---: |
| 1 | 4,566.2 ms |
| 2 | 4,892.5 ms |
| 3 | 4,508.7 ms |

The median was 4,566.2 ms, and all three runs preserved every ordered line and
kept the GUI and integrated PowerShell process alive. This closes the
deterministic 100,000-line/10-MiB completeness seam. It is not the complete bulk
performance gate: matched Windows Terminal elapsed time, at least 100
input-latency samples during output, retained-memory recovery after clear, and
an explicit scrollback ceiling measurement remain pending.
