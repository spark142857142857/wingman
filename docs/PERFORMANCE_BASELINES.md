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
performance gate: matched Windows Terminal elapsed time remains pending.
Retained-memory recovery and the explicit scrollback ceiling are measured
below.

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
this machine. The matched Windows Terminal comparison and the separate
`cmd.exe` release matrix remain pending.

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
The matched Windows Terminal comparison remains separate.
