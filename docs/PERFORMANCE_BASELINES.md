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
performance gate by itself. Retained-memory recovery, the explicit scrollback
ceiling, and the matched Windows Terminal elapsed time are measured below.

## 2026-08-12: release bulk-output input-latency distribution

- App source: `6089a96cb5aab99797852ddd36dbe13b22752e49`
- Harness introduced at: `3a40d001ecd8d8a5f54944830efceff06c53f34a`
- Build and machine: unchanged from the deterministic bulk-render precheck

Command:

```powershell
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_bulk_input_latency.tests.ps1 -Executable src-tauri/target/release/wingman.exe -TimeoutSeconds 60
```

The environment-gated probe divides the same 100,000-line, 11,900,000-byte
PowerShell workload into 100 ordered 1,000-line bursts. At each sample boundary,
the frontend takes a monotonic timestamp immediately before sending one fixed
character through xterm's normal user-input event. The character passes through
the normal Tauri command, Rust terminal session, PTY, and PowerShell console
input. PowerShell consumes it without echoing and emits an indexed response
marker; completion is recorded only after xterm parses that PTY chunk and two
animation frames elapse. The next output burst then continues. Indexed markers
make missing, repeated, and out-of-order samples fail closed. Rust accepts
exactly 100 finite bounded samples, independently recomputes the summary, and
exposes the raw distribution to the local black-box harness. The probe flag is
removed from the PowerShell child environment.

| Independent run | Median | p95 (nearest-rank) | Maximum | Result |
| --- | ---: | ---: | ---: | --- |
| 1 | 41.0 ms | 47.8 ms | 48.6 ms | Pass |
| 2 | 40.6 ms | 48.4 ms | 48.9 ms | Pass |
| 3 | 41.1 ms | 48.1 ms | 48.8 ms | Pass |

Raw samples, run 1 (ms):

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

Raw samples, run 2 (ms):

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

Raw samples, run 3 (ms):

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

All three independent distributions pass the 200 ms p95 bulk-output input
latency ceiling with more than 4x headroom, while the GUI and integrated
PowerShell process remain alive. This closes the PowerShell measurement seam on
this machine. The separate `cmd.exe` release matrix remains pending.

## 2026-08-12: release bulk-output retained-memory distribution

- App source: `d1415336dcec620b3a6e3e8d00d38a3cd9a07f54`
- Harness introduced at: `a04fa4e464b58cd223ec57f11e64f6160394803b`
- Per-process diagnostics added at: `7447aac59fc1ecd5082d37b8bdccaff542dba9d9`
- Build and machine: unchanged from the preceding bulk-output measurements

Command:

```powershell
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_bulk_retained_memory.tests.ps1 -Executable src-tauri/target/release/wingman.exe -PhaseTimeoutSeconds 90
```

The black-box harness records ten one-second whole-process-tree private-working-
set samples after a 10-second idle settle. The probe then runs the verified
100,000-line, 11,900,000-byte generator as a foreground child PowerShell,
renders and validates the entire stream, submits `Clear-Host` through the normal
xterm/Tauri/Rust/PTY input path, and exposes completion only after a fixed marker
is rendered. After another 10-second settle, the harness records ten retained
samples. The child generator must have exited while the GUI and integrated
PowerShell remain alive.

The first implementation had no output backpressure. A standard diagnostic run
retained as much as 280.42 MiB from a 147.68 MiB idle median, a 132.74 MiB
increase and a hard-ceiling failure. Short per-process diagnostics attributed
most growth to the WebView renderer. The fix sequences every PTY chunk and lets
the Rust reader deliver the next chunk only after xterm acknowledges parsing the
previous one. Session replacement closes this flow and wakes a blocked old
reader. Moving the deterministic generator into a foreground child also avoids
retaining its heap in the long-lived interactive PowerShell.

| Independent run | Idle median | Retained median | Retained maximum | Maximum growth | Result |
| --- | ---: | ---: | ---: | ---: | --- |
| 1 | 146.99 MiB | 187.28 MiB | 187.30 MiB | 40.31 MiB | Ceiling pass |
| 2 | 148.66 MiB | 187.66 MiB | 191.55 MiB | 42.89 MiB | Ceiling pass |
| 3 | 147.78 MiB | 184.88 MiB | 187.62 MiB | 39.84 MiB | Ceiling pass |

Raw idle samples (MiB):

```text
run 1: 147.207, 146.992, 146.992, 146.992, 146.977, 146.980, 146.980, 146.996, 146.758, 146.738
run 2: 147.688, 147.664, 148.246, 148.469, 148.566, 148.762, 149.102, 149.203, 149.086, 149.238
run 3: 147.250, 147.215, 147.328, 147.613, 147.699, 147.859, 148.230, 148.387, 148.250, 148.406
```

Raw retained samples (MiB):

```text
run 1: 187.301, 187.301, 187.301, 187.301, 187.281, 187.281, 187.281, 187.281, 187.281, 187.195
run 2: 191.488, 191.551, 191.555, 187.598, 187.656, 187.664, 187.664, 187.453, 187.469, 187.379
run 3: 187.621, 187.621, 187.621, 187.621, 184.844, 184.852, 184.914, 184.602, 184.523, 184.594
```

All three runs pass the absolute 350 MiB ceiling and the relative 50 MiB
retained-memory release ceiling. They miss the 25 MiB target by 14.84 to 17.89
MiB, so further renderer allocation work remains an optimization opportunity,
not a P0 release blocker. Revalidation also preserved the 11.9 MB stream in
4,819.2 ms and measured input latency at 41.8 ms median, 48.7 ms p95, and
50.0 ms maximum.

## 2026-08-12: explicit release scrollback ceiling

- App source: `90bc57ec0a9d842eaf34c1220308347857a8d626`
- Harness introduced at: `cedbba0430615050f8825ec34f580a0ab5e533e3`
- Build and machine: unchanged from the preceding bulk-output measurements

Command:

```powershell
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_scrollback_ceiling.tests.ps1 -Executable src-tauri/target/release/wingman.exe -TimeoutSeconds 60
```

The release-only probe uses the same verified 100,000-line, 11,900,000-byte
foreground PowerShell workload. After xterm parses the complete stream and the
end marker is visible, the frontend reports the configured scrollback, active
viewport rows, and normal-buffer rows. Rust accepts the report only for the
current session, exact probe opt-in, configured P0 ceiling, and a buffer whose
retained rows exactly fill that ceiling. The black-box harness independently
requires the same value and an active PowerShell PTY.

An initial 10,000-row candidate retained 54.66 MiB above idle after clear and
failed the 50 MiB release ceiling. A 5,000-row candidate passed once at
48.33 MiB but left too little measurement margin. P0 therefore fixes the limit
at 4,000 rows, four times xterm's previous implicit default while preserving the
existing memory release gate.

The release scrollback test retained exactly 4,000 rows after all 100,000 lines.
Three independent retained-memory rechecks with that ceiling produced:

| Independent run | Idle median | Retained median | Retained maximum | Maximum growth | Result |
| --- | ---: | ---: | ---: | ---: | --- |
| 1 | 148.43 MiB | 190.78 MiB | 193.52 MiB | 45.08 MiB | Ceiling pass |
| 2 | 148.19 MiB | 191.18 MiB | 195.00 MiB | 46.81 MiB | Ceiling pass |
| 3 | 150.62 MiB | 193.59 MiB | 197.66 MiB | 47.03 MiB | Ceiling pass |

Raw idle samples (MiB):

```text
run 1: 147.922, 147.809, 148.074, 148.266, 148.359, 148.508, 148.824, 149.055, 148.918, 149.109
run 2: 147.668, 147.652, 147.770, 148.047, 148.094, 148.277, 148.613, 148.828, 148.730, 148.855
run 3: 150.152, 150.055, 150.258, 150.488, 150.547, 150.699, 151.047, 151.203, 151.066, 151.270
```

Raw retained samples (MiB):

```text
run 1: 193.438, 193.438, 193.438, 193.516, 190.719, 190.781, 190.781, 190.766, 190.664, 190.730
run 2: 194.992, 194.992, 194.996, 194.996, 191.137, 191.121, 191.184, 191.156, 191.176, 191.129
run 3: 197.648, 197.656, 197.656, 197.656, 193.547, 193.555, 193.617, 193.539, 193.566, 193.555
```

All three runs stay below the 50 MiB retained-memory release ceiling while the
GUI and integrated PowerShell remain alive. The explicit scrollback gap is now
closed. Revalidation preserved all 11,900,000 bytes in 5,105.0 ms and measured
bulk-output input latency at 41.8 ms median, 48.6 ms p95, and 50.8 ms maximum.
The matched Windows Terminal comparison is measured below.

## 2026-08-12: matched Windows Terminal bulk-render comparison

- Harness introduced at: `8f004899b4207c3053a1050e8edf238160862fa3`
- Wingman app source: `90bc57ec0a9d842eaf34c1220308347857a8d626`
- Windows Terminal: `1.24.11911.0`, x64 stable package
- Windows: `10.0.26200.0`
- Windows PowerShell: `5.1.26100.8875`

Command:

```powershell
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_bulk_host_comparison.tests.ps1 -WingmanExecutable src-tauri/target/release/wingman.exe -RunCount 3 -TimeoutSeconds 90
```

Each paired run starts fresh host and shell processes in the same working
directory and emits the same deterministic 100,000-line, 11,900,000-byte
PowerShell payload. Both hosts use one tab and an outer window measured at
1,116 x 759 pixels. A private temporary signal lets the harness resize Windows
Terminal before its generator starts; the signal is deleted after each run.
The installed Windows Terminal default profile is Windows PowerShell and has no
user history, font, initial-column, or initial-row override, so its packaged
9,001-row history default remains effective. Wingman uses its explicit
4,000-row P0 ceiling.

Timing starts immediately before host launch. Wingman completes only after it
validates the full stream hash and byte count, finds the end marker in the
rendered xterm buffer, and waits two animation frames. Windows Terminal's fixed
completion title follows the flushed payload in console order; after observing
that title, the harness waits for two successful DWM compositor flushes. The
benchmark PowerShell process must still be alive at both completion boundaries.

| Paired run | Wingman | Windows Terminal | Pair ratio |
| --- | ---: | ---: | ---: |
| 1 | 4,807.9 ms | 3,869.3 ms | 1.2426x |
| 2 | 4,610.4 ms | 3,894.1 ms | 1.1840x |
| 3 | 4,712.2 ms | 3,877.3 ms | 1.2153x |
| Median | 4,712.2 ms | 3,877.3 ms | 1.2153x |

Raw elapsed samples (ms):

```text
Wingman: 4807.884, 4610.408, 4712.232
Windows Terminal: 3869.302, 3894.059, 3877.279
```

The median ratio of 1.2153x passes both the 2x target and the 3x release
ceiling. This closes the matched Windows Terminal bulk-render comparison on
this machine without changing the user's Windows Terminal settings or leaving
benchmark processes and signal files behind.

## 2026-08-12: warmed-cache release runner timing

- Runner source: `d9ef6c557e31c9468d6df2ae41ab217be9ece4f6`
- Harness and accepted targets: `dc46478fd6b4a6194fa5be8b41f55e70ad9b96db`
- OS: Microsoft Windows 11 Home `10.0.26200`
- CPU: AMD Ryzen 7 9700X, 8 physical cores, 16 logical processors
- Power: Windows Balanced (`381b4222-f694-41f0-9685-ff5bb260df2e`)
- Toolchain: `rustc 1.96.1`, `cargo 1.96.1`
- Build: optimized Cargo `release`

Command:

```powershell
cargo test --release --manifest-path src-tauri/Cargo.toml --test runner_performance_contract cached_runner_timing_baseline -- --ignored --exact --nocapture
```

Each independent invocation creates a private sandbox containing an exact
100 MiB UTF-8 corpus of 819,200 fixed 128-byte LF records, a tree with exactly
20,000 entries, and 200,000 fixed 32-byte reverse-ordered sort records. An
untimed pass warms each operation. Three timed passes then start the real
`wingman-runner`, fetch one typed request through a one-shot broker, process the
data, validate the result and exit status, and remove the entire sandbox.

`grep` scans the full 100 MiB corpus for one fixed match in the final record.
`find` emits and verifies all 20,000 entries. `cat` redirects normalized output
from the 100 MiB corpus, excluding renderer cost. `sort` materializes and
redirects all 200,000 records. Corpus creation, warm-up, result validation, and
cleanup are outside the recorded interval; broker fetch, process startup,
runner validation, file work, output completion, and process exit are inside.

| Independent distribution | `grep` median | `find` median | Redirected `cat` median | Redirected `sort` median |
| --- | ---: | ---: | ---: | ---: |
| 1 | 825.8 ms | 442.8 ms | 3,070.8 ms | 663.2 ms |
| 2 | 832.3 ms | 439.0 ms | 3,081.2 ms | 653.7 ms |
| 3 | 818.7 ms | 442.8 ms | 3,093.1 ms | 654.7 ms |
| Outer median | 825.8 ms | 442.8 ms | 3,081.2 ms | 654.7 ms |
| Accepted target | 1,000 ms | 535 ms | 3,700 ms | 790 ms |

Outer-median throughput was 121.09 MiB/s for `grep`, 45,166.9 entries/s for
`find`, 32.45 MiB/s for redirected `cat`, and 305,483.5 records/s for redirected
`sort`. Accepted targets are the first outer medians plus the policy's 20%
runner-throughput regression line, rounded upward. The ignored release test now
enforces those targets.

Raw samples (ms):

```text
distribution 1
grep: 825.849, 827.105, 824.290
find: 446.221, 442.802, 436.071
redirected cat: 3121.691, 3061.706, 3070.828
redirected sort: 666.742, 663.203, 650.044

distribution 2
grep: 813.251, 832.275, 838.572
find: 439.243, 438.975, 438.469
redirected cat: 3122.941, 3081.241, 3065.282
redirected sort: 652.718, 656.612, 653.664

distribution 3
grep: 841.868, 812.706, 818.675
find: 438.766, 461.041, 442.828
redirected cat: 3143.331, 3087.476, 3093.148
redirected sort: 649.877, 656.727, 654.700
```

A final run after target enforcement also passed, with medians of 813.5 ms,
438.6 ms, 3,057.6 ms, and 649.6 ms respectively. No sandbox remained. This
closes the reproducible warmed-cache runner timing seam. It is not uncached
evidence: a true uncached distribution requires a controlled restart and a
pre-existing corpus, rather than an unverified system-cache purge.

## 2026-08-12: release runner `sort` resource ceiling

- Runner source: `d9ef6c557e31c9468d6df2ae41ab217be9ece4f6`
- Harness and accepted ceiling: `f71b82a45ccf48356b92e15cebe9787170e1ebcc`
- OS, CPU, power plan, toolchain, and release profile: unchanged from the
  preceding runner timing baseline

Command:

```powershell
cargo test --release --manifest-path src-tauri/Cargo.toml --test runner_resource_contract sort_resource_limit_stays_bounded_and_fails_closed -- --ignored --exact --nocapture
```

Each independent distribution creates two private inputs. The byte-limit input
contains 1,024 records of exactly 65,536 text bytes, for exactly 64 MiB of
retained sort text. The real broker and release runner accept and redirect all
records. The harness then appends one equally sized record and requires a
fail-closed rejection. A second input contains 262,145 short records and proves
the independent 262,144-record limit. Every scenario runs three times.

The parent samples `PrivateUsage` every 2 ms through
`GetProcessMemoryInfo`; the maximum sample is reported as peak private bytes.
The same API's process-lifetime `PeakWorkingSetSize` is the peak working-set
measurement. Both must remain at or below the accepted 96 MiB release ceiling.
The exact-limit success writes all 67,110,912 normalized output bytes. Both
over-limit cases exit `1`, emit exactly the fixed 55-byte CRLF diagnostic
`wingman sort: materialization resource limit exceeded`, and leave the opened
redirect target at zero bytes.

| Distribution | Exact 64 MiB peak WS | Exact 64 MiB peak private | Byte + 1 record peak WS | Byte + 1 record peak private | Count + 1 peak WS | Count + 1 peak private |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 69.47 MiB | 65.69 MiB | 69.46 MiB | 65.90 MiB | 18.95 MiB | 18.09 MiB |
| 2 | 69.48 MiB | 65.99 MiB | 69.45 MiB | 65.75 MiB | 18.96 MiB | 18.05 MiB |
| 3 | 69.50 MiB | 65.92 MiB | 69.46 MiB | 65.75 MiB | 18.96 MiB | 18.09 MiB |

Raw samples contain elapsed milliseconds, peak working-set MiB, and peak
private-byte MiB in that order:

```text
distribution 1
exact: (479.197, 69.426, 65.609), (461.989, 69.473, 65.637), (458.922, 69.465, 65.691)
byte + 1: (445.400, 69.461, 65.898), (454.205, 69.434, 65.559), (435.508, 69.438, 65.695)
count + 1: (38.185, 18.930, 18.016), (36.818, 18.949, 18.090), (37.897, 18.934, 18.020)

distribution 2
exact: (461.897, 69.484, 65.758), (452.405, 69.477, 65.816), (456.404, 69.477, 65.988)
byte + 1: (447.168, 69.445, 65.684), (430.147, 69.449, 65.754), (447.632, 69.449, 65.555)
count + 1: (39.320, 18.957, 15.160), (40.400, 18.953, 18.047), (37.663, 18.957, 15.289)

distribution 3
exact: (453.231, 69.496, 65.918), (468.619, 69.449, 65.914), (456.376, 69.445, 65.680)
byte + 1: (437.813, 69.465, 65.754), (429.688, 69.434, 65.555), (431.011, 69.438, 65.492)
count + 1: (39.006, 18.945, 18.035), (37.901, 18.953, 18.043), (37.025, 18.961, 18.094)
```

The overall maximum was 69.50 MiB working set and 65.99 MiB private bytes, so
all 27 runner processes passed the 96 MiB release ceiling. No partial sorted
record, unbounded diagnostic, surviving runner, or resource sandbox remained.
This closes the release process-memory evidence for bounded `sort` while the
separate traversal, listing, and mutation resource-limit cases are measured in
their own gates.

## 2026-08-19: release traversal and listing resource ceilings

- Runner source: `b9b3510d5adb14dbac707839445733ddc16f687a`
- Harness and accepted ceiling: `c2f8fae67b8c792297e10fe9fea8fdb355b7b5df`
- OS, CPU, power plan, toolchain, and release profile: unchanged from the
  preceding runner baselines

Command:

```powershell
cargo test --release --manifest-path src-tauri/Cargo.toml --test runner_resource_contract traversal_and_listing_resource_limits_are_bounded -- --ignored --exact --nocapture
```

The real broker and release runner are exercised in a private NTFS sandbox.
One shared flat tree proves `find` at exactly 100,000 visited objects and
recursive `grep` at exactly 100,000 directory entries, then adds one file for
both rejection cases. A fresh directory proves `ls` at 262,144 entries and
then at 262,145. A second listing directory uses 92,309 fixed 727-byte Unicode
names plus one 221-byte name for exactly 67,108,864 retained UTF-8 name bytes;
one final name exceeds that independent limit while remaining below the entry
limit. Each case runs three release sidecars per distribution.

Exact successes validate every redirected CRLF record. Each over-limit case
exits `1`, emits exactly one bounded diagnostic, and starts no further
filesystem work. Seeded redirect targets remain unchanged for pre-collected
`find` and `ls`; flat recursive `grep` opens its target first and leaves it
empty, as required by the streaming redirection contract. Lifetime peak
working set and 2 ms sampled peak private bytes must each remain at or below
144 MiB.

Peak pairs below are working-set/private-byte MiB. Distribution 1 was the
diagnostic run used to choose the ceiling; distributions 2 and 3 enforce it in
the harness. All three distributions are below the finalized ceiling.

| Distribution | Find exact | Find +1 | Recursive grep exact | Recursive grep +1 |
| --- | ---: | ---: | ---: | ---: |
| 1 | 36.05 / 38.58 | 34.61 / 32.99 | 80.93 / 114.59 | 80.94 / 114.60 |
| 2 | 35.76 / 38.58 | 34.67 / 33.23 | 80.94 / 114.60 | 80.92 / 114.59 |
| 3 | 35.77 / 38.59 | 34.61 / 32.97 | 80.92 / 114.59 | 80.95 / 114.59 |

| Distribution | ls 262,144 | ls 262,145 | ls 64 MiB names | ls names +1 |
| --- | ---: | ---: | ---: | ---: |
| 1 | 40.66 / 51.68 | 40.59 / 47.28 | 80.29 / 90.41 | 80.21 / 82.74 |
| 2 | 40.66 / 51.68 | 40.60 / 47.28 | 80.28 / 90.38 | 80.20 / 82.73 |
| 3 | 40.66 / 51.72 | 40.60 / 47.30 | 80.25 / 90.40 | 80.16 / 82.74 |

Raw samples contain elapsed milliseconds, peak working-set MiB, and peak
private-byte MiB in that order:

```text
distribution 1
find exact: (1704.420, 35.734, 38.582), (1608.360, 36.047, 38.484), (1670.427, 35.762, 38.484)
find +1: (67.504, 34.379, 32.992), (70.814, 34.559, 32.883), (68.376, 34.613, 32.297)
grep exact: (4198.368, 80.926, 114.586), (3995.311, 80.934, 114.590), (4209.800, 80.934, 114.590)
grep +1: (44.171, 80.941, 114.594), (44.356, 80.926, 114.602), (42.695, 80.941, 114.594)
ls entries exact: (6436.743, 40.656, 51.680), (6226.650, 40.656, 51.676), (6801.617, 40.656, 51.672)
ls entries +1: (5591.081, 40.594, 47.281), (5421.979, 40.590, 47.281), (5420.708, 40.547, 47.211)
ls names exact: (4253.949, 80.293, 90.379), (4118.229, 80.258, 90.414), (4135.006, 80.285, 90.379)
ls names +1: (3730.151, 80.188, 82.738), (3738.254, 80.156, 82.727), (3732.764, 80.207, 82.730)

distribution 2
find exact: (1611.557, 35.742, 38.582), (1601.542, 35.758, 38.246), (1623.823, 35.730, 38.566)
find +1: (66.820, 34.672, 33.227), (68.493, 34.625, 32.629), (65.973, 34.617, 32.629)
grep exact: (4287.382, 80.926, 114.570), (3953.238, 80.938, 114.598), (3954.493, 80.906, 114.602)
grep +1: (42.503, 80.922, 114.594), (42.749, 80.898, 114.594), (42.673, 80.906, 114.582)
ls entries exact: (6205.780, 40.664, 51.680), (6199.050, 40.656, 51.672), (6162.222, 40.656, 51.676)
ls entries +1: (5565.393, 40.563, 47.215), (5397.553, 40.555, 47.242), (5412.735, 40.598, 47.277)
ls names exact: (4233.677, 80.277, 90.375), (4330.600, 80.277, 90.367), (4226.999, 80.273, 90.355)
ls names +1: (3756.382, 80.199, 82.727), (3979.974, 80.176, 82.727), (3750.843, 80.191, 82.711)

distribution 3
find exact: (1604.332, 35.668, 38.574), (1579.947, 35.676, 38.234), (1548.413, 35.773, 38.586)
find +1: (67.401, 34.523, 32.223), (65.862, 34.328, 32.918), (65.806, 34.609, 32.973)
grep exact: (3933.848, 80.922, 114.590), (3885.300, 80.910, 114.586), (3894.936, 80.906, 114.594)
grep +1: (44.362, 80.914, 114.590), (42.881, 80.945, 114.574), (42.652, 80.891, 114.586)
ls entries exact: (6163.696, 40.660, 51.676), (6222.849, 40.660, 51.723), (6122.143, 40.656, 51.668)
ls entries +1: (5339.327, 40.602, 47.297), (5341.426, 40.598, 47.281), (5313.350, 40.543, 47.215)
ls names exact: (4153.648, 80.238, 90.402), (4353.721, 80.254, 90.363), (4153.380, 80.250, 90.375)
ls names +1: (3703.147, 80.156, 82.734), (3708.163, 80.164, 82.738), (3701.969, 79.531, 82.047)
```

The overall maximum was 80.95 MiB working set and 114.61 MiB private bytes.
All 72 runner processes stayed below 144 MiB, every exact boundary was
accepted, every plus-one boundary was rejected, and no runner process or
resource sandbox survived cleanup. This closes the traversal and listing
release resource gate; mutation resource measurements remain separate.

## 2026-08-19: release mutation resource ceiling

- Runner source: `b9b3510d5adb14dbac707839445733ddc16f687a`
- Harness and accepted ceiling: `dfb67530f85a10c1528b0a79d3b67e069282cd94`
- OS, CPU, power plan, toolchain, and release profile: unchanged from the
  preceding runner baselines

Command:

```powershell
cargo test --release --manifest-path src-tauri/Cargo.toml --test runner_mutation_resource_contract mutation_resource_limits_are_bounded_and_atomic -- --ignored --exact --nocapture
```

The real broker and release runner first execute `mkdir` and `touch` with the
exact 128-path wire limit, then prove that 129 paths are rejected before any
item is created. One private flat tree contains its root plus 99,999 files.
Each distribution copies that exact 100,000-entry tree three times through the
real same-parent staging/commit path and removes each complete destination with
the real recursive `rm`. It then moves the source three times on the same NTFS
volume, restoring only the test fixture between runner processes. Adding one
file makes all recursive `cp`, `mv`, and `rm` requests fail global preflight.

Every exact operation validates the complete final filesystem tree. Every
plus-one case validates exit `2`, one bounded diagnostic, an unchanged source,
no destination, and no `.wingman-stage-*` artifact. Lifetime peak working set
and 2 ms sampled peak private bytes must each remain at or below 80 MiB.

Peak pairs below are working-set/private-byte MiB. Distribution 1 selected the
ceiling; distributions 2 and 3 enforce it. All three are below the final bound.

| Distribution | mkdir 128 / +1 | touch 128 / +1 | cp 100k / +1 |
| --- | ---: | ---: | ---: |
| 1 | 4.62/0.87 · 4.45/0.74 | 4.60/0.87 · 4.45/0.72 | 36.19/42.12 · 26.73/25.37 |
| 2 | 4.59/0.86 · 4.45/0.72 | 4.60/0.87 · 4.45/0.70 | 36.20/42.15 · 26.76/25.38 |
| 3 | 4.59/0.86 · 4.46/0.71 | 4.62/0.87 · 4.45/0.70 | 36.18/42.16 · 26.78/25.45 |

| Distribution | mv 100k / +1 | rm 100k / +1 |
| --- | ---: | ---: |
| 1 | 32.96/38.04 · 26.75/25.38 | 52.98/58.35 · 49.86/49.17 |
| 2 | 33.09/38.19 · 26.81/25.45 | 53.00/58.37 · 49.87/49.17 |
| 3 | 33.10/38.19 · 26.82/25.46 | 53.01/58.36 · 49.86/49.17 |

Raw samples contain elapsed milliseconds, peak working-set MiB, and peak
private-byte MiB in that order:

```text
distribution 1
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

distribution 2
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

distribution 3
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

The overall maximum was 53.01 MiB working set and 58.37 MiB private bytes. All
90 runner processes stayed below 80 MiB. Exact operations completed their full
filesystem effects; every plus-one case was atomic, and no staging item,
runner process, or resource sandbox remained. This closes the mutation release
resource gate.

## 2026-08-19: controlled-restart runner harness

The uncached runner harness is now reproducible without claiming to purge the
Windows system file cache. It prepares one persistent, marked fixture and runs
exactly one of `grep`, `find`, redirected `cat`, or redirected `sort` per boot.
The coordinator rejects a boot older than 15 minutes, rejects a second sample
from the same boot, records the exact source commit and Windows build locally,
and requires five unique boots for every operation before producing a
distribution.

```powershell
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_uncached_runner.tests.ps1 -Mode Prepare -FixtureRoot C:\wingman-perf\runner-uncached
# After a controlled restart, run one operation only:
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_uncached_runner.tests.ps1 -Mode Sample -FixtureRoot C:\wingman-perf\runner-uncached -Operation grep
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_uncached_runner.tests.ps1 -Mode Validate -FixtureRoot C:\wingman-perf\runner-uncached
```

A warmed harness smoke completed the real release broker/runner path over the
100 MiB corpus and emitted the versioned result marker. Its timing was
deliberately discarded and is not uncached evidence. No controlled-restart
sample is checked into the repository yet, so uncached targets remain unset
and this release gate remains open.

## 2026-08-19: 30-minute release endurance workload

- App source: `dc593882fdaedcd4c8ecb3d4c9c1e396d79e41ba`
- Measurement harness: `9fd1e6844ef0ca2fc262abf170d42527d07e7d52`
- OS: Windows `10.0.26200.9168`, display version `25H2`
- CPU: AMD Ryzen 7 9700X, 8 physical cores, 16 logical processors
- Power: Windows Balanced (`381b4222-f694-41f0-9685-ff5bb260df2e`)
- WebView2 Runtime: `151.0.4129.93`
- Toolchain: `rustc 1.96.1`, `cargo 1.96.1`
- Build: official Tauri release frontend, no installer bundle

Command:

```powershell
npm run tauri build -- --no-bundle
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_endurance.tests.ps1 -Executable src-tauri/target/release/wingman.exe
```

The release-only probe waited ten seconds for initial settling, then ran for 30
minutes. Every cycle enabled Familiar mode when needed, rendered and cleared
250 lines, started and cancelled `tail -f`, alternated the PTY size, restarted
Windows PowerShell, and required authenticated editor readiness before
continuing. It completed 907 cycles. The harness sampled the complete process
tree every ten seconds and used the median of five 250 ms-spaced samples for
each settled endpoint. Failed gates emit the same versioned raw result before
returning nonzero, so evidence is not lost at the assertion boundary.

| Metric | Result | Release ceiling |
| --- | ---: | ---: |
| Baseline private working set | 146.789 MiB | n/a |
| Final settled private working set | 175.090 MiB | 350 MiB |
| Growth | 28.301 MiB | 50 MiB |
| Growth | 19.280% | 20% |
| Remaining runner processes | 0 | 0 |

The final tree contained only `wingman.exe`, WebView2, PowerShell, and
`conhost.exe`. All endpoint samples were internally stable, and the process
exited successfully. This closes the automated PowerShell endurance
resource-ceiling check on this machine. It is not a substitute for the
separate input-latency distributions or the current-and-previous Windows
release matrix.
