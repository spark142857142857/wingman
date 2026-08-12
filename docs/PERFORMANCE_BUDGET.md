# Performance Budget and Measurement Contract (Draft)

Status: proposed P0 performance contract. Its numbers become binding only when
the consolidated implementation plan is reviewed and approved. This document
does not authorize implementation.

Korean version: [PERFORMANCE_BUDGET.ko.md](PERFORMANCE_BUDGET.ko.md)

## Product-level decision

Wingman must feel like an interactive terminal, but it is not required to use
the same resources as bare `cmd.exe`. Wingman includes a terminal renderer, a
WebView2 process group, a Rust host, a PTY, and an active native shell. The fair
comparison is a complete terminal host running the same shell.

Performance is measured at stable architectural boundaries so the same suite
still applies if xterm/WebView2 is later replaced by a native renderer.

## Reference environment

Release-blocking measurements use an optimized release build on the supported
Windows 11 24H2-or-later x64 matrix and this minimum reference class:

- 4 physical CPU cores / 8 logical processors
- 8 GiB RAM, SSD storage, integrated or entry-level GPU
- default 1100 x 720 Wingman window at 100% or 125% display scaling
- Windows Balanced power mode, plugged in, Microsoft Defender enabled
- current Evergreen WebView2 Runtime
- one local window, one WebView, and one active shell session

Both `cmd.exe` and Windows PowerShell 5.1 are measured. The exact CPU, storage,
Windows build, shell build, WebView2 version, power mode, and Wingman commit are
recorded with each result. A secondary unplugged or power-limited run is
reported for regressions but is not the initial P0 release gate.

## Comparison baselines

Every benchmark records these cases separately:

1. bare shell in its standard console host, as a lower-bound diagnostic only;
2. Windows Terminal with one tab and the same shell, as the fair terminal-host
   comparison;
3. Wingman with Familiar mode off;
4. Wingman with Familiar mode on and a native pass-through command;
5. Wingman with a P0 command executed by `wingman-runner`.

Wingman does not fail merely for exceeding bare `cmd.exe`. It fails an absolute
release ceiling or a confirmed regression against the last accepted Wingman
baseline.

## P0 budget

| Metric | Target | Release ceiling | Measurement boundary |
| --- | ---: | ---: | --- |
| Cold launch to interactive shell | <= 1.5 s | <= 3.0 s | process start to accepted and echoed probe input after the prompt is ready |
| Warm launch to interactive shell | <= 0.8 s | <= 1.5 s | same boundary after runtime and OS caches are warm |
| Local key-to-shell echo | p95 <= 50 ms | p95 <= 100 ms | xterm input event through PTY output rendered on screen |
| Reliable-line classification, pass-through | p95 <= 2 ms | p95 <= 10 ms | Enter at validated `Editing/Reliable` state to `PassThrough` decision |
| P0 runner dispatch overhead | p95 <= 75 ms | p95 <= 150 ms | submitted line to runner operation start, excluding the operation itself |
| Terminal resize settle | p95 <= 100 ms | p95 <= 300 ms | resize event to fitted renderer and acknowledged PTY size |
| Ctrl+C cancellation | p95 <= 200 ms | p95 <= 500 ms | interrupt input to runner exit `130` and control returned to the shell |
| Settled idle CPU | median <= 0.2%, p95 <= 1% | median <= 0.5%, p95 <= 2% | whole Wingman process tree after a 10 s settle period |
| Settled idle private working set | <= 250 MiB | <= 350 MiB | host, WebView2 group, PTY support, and one active shell; no runner alive |
| 30-minute idle memory growth | <= 10% and <= 25 MiB | <= 20% and <= 50 MiB | increase from the settled idle baseline |
| Installed Wingman files | <= 30 MiB | <= 60 MiB | application, runner, and assets; shared Evergreen WebView2 excluded |
| Local app data after 100 clean launches | <= 25 MiB | <= 100 MiB | Wingman/WebView profile data excluding user-created exports |

Both the time/percentage condition and the absolute condition apply where a
row contains both. During the Phase 1 boundary spikes, an optimized release
build may produce one calibration proposal using recorded raw data and an
explained reason. The user reviews that proposal with the consolidated
implementation plan. Once accepted, targets and ceilings are frozen for P0
acceptance; changing one requires an explicit performance-contract decision
rather than quietly moving a target or relaxing a test.

## Readiness and process accounting

"Window shown" is not startup completion. Interactive readiness requires the
renderer to be focused, the PTY and selected shell to be alive, a valid prompt
marker to establish `Editing/Reliable`, and a probe sent through the normal
terminal input path to return through normal PTY rendering.

Memory and CPU include the entire Wingman-owned process tree:

```text
wingman.exe
  + associated WebView2 browser/renderer/GPU/utility processes
  + PTY or console support processes
  + one active powershell.exe or cmd.exe
  + wingman-runner.exe while a P0 request is active
```

Launch timing begins at the public launcher process and includes the protected
same-binary GUI handoff in the [CLI launch contract](CLI_LAUNCH_CONTRACT.md).
The launcher is included while alive; settled measurements begin only after it
has acknowledged readiness and exited. The surviving GUI-role process remains
`wingman.exe` and owns the listed runtime tree.

Private working set is the primary memory figure because summing shared working
sets can double-count shared runtime pages. Total working set, private bytes,
process count, JavaScript heap, and Rust allocations are also recorded for
diagnosis. The shared installed WebView2 Runtime is excluded from Wingman's disk
footprint, but WebView2 processes are included in runtime resource use.

## Required workloads

### Startup and interaction

- launch into both supported shells from a normal filesystem directory;
- type, erase, paste, submit, switch Familiar mode, resize, and restart a
  session through normal UI paths;
- compare reliable prompt editing with completion/unknown-edit and foreground
  pass-through under the [terminal session contract](TERMINAL_SESSION_CONTRACT.md);
- compare Familiar OFF, native pass-through, P0 acceptance, and P0 rejection;
- test Korean text, spaces, and long Windows paths as well as ASCII input.

### Output and responsiveness

A deterministic helper emits 100,000 UTF-8 lines totaling at least 10 MiB
through the real PTY. Wingman must render the complete stream without byte loss,
process failure, or an unresponsive window. During output, sampled input
latency stays below 200 ms at p95. Elapsed rendering time should be at most 2x
the matched Windows Terminal baseline, with 3x as the release ceiling.

After output is cleared and the app settles, retained private working set is no
more than 25 MiB above the earlier idle value as a target and 50 MiB as a
ceiling. Scrollback is explicitly bounded; unlimited terminal history is not a
P0 feature.

Pipeline benchmarks also include channel capacity `1`, a deliberately slow
consumer, one maximum-size record, invalid UTF-8 split across reads, and early
`head` stop. They must preserve the [text stream
contract](TEXT_STREAM_MODEL.md) while meeting cancellation and memory ceilings;
throughput does not justify an unbounded channel or raw-byte bypass.

### Runner and filesystem

The runner suite measures cached and uncached cases separately:

- `grep` over a deterministic 100 MiB UTF-8 corpus;
- `find` over a 20,000-entry directory tree;
- redirected `cat` over 100 MiB so renderer cost is excluded;
- redirected `sort` over 200,000 lines so required materialization is visible;
- recursive traversal cancellation and idle `tail -f`.

The first implementation baseline establishes operation-specific throughput
targets before each command is accepted. Regardless of disk speed, these cases
must remain cancellable, obey memory/resource limits, and not busy-poll while
idle. `tail -f` must satisfy the normal idle CPU ceiling when the file is
unchanged.

Reproducible component measurements are recorded in
[PERFORMANCE_BASELINES.md](PERFORMANCE_BASELINES.md); they do not replace the
whole process-tree release gate.

### Endurance

A 30-minute scenario repeatedly emits output, clears, resizes, starts and
cancels a P0 command, and restarts the shell. It must stay within the memory
growth ceiling, preserve input responsiveness, and leave no runner or broker
request after the owning session ends.

## Measurement procedure

- Use release builds; development server, DevTools, debugger, and hot reload
  results are never release evidence.
- Close unrelated foreground apps and keep antivirus and ordinary Windows
  services in their normal user configuration.
- Warm launch uses three warmups followed by at least 20 recorded runs. Cold
  launch records the first launch after at least five controlled restarts or
  equivalent documented cold-runtime conditions.
- Interaction distributions contain at least 100 samples and report median,
  p95, maximum, and raw data. Do not accept an average that hides stalls.
- Measure the same shell, directory, window size, corpus, and power state in
  each comparison group.
- Use monotonic in-process markers for Wingman boundaries and ETW/WPR/WPA for
  whole-system startup, CPU, disk, and WebView2 diagnosis. Instrumentation is a
  development measurement path, not production telemetry.
- Store the benchmark definition and summary result with the release record.
  Raw traces containing paths or terminal data remain local and follow the
  [security model](SECURITY_MODEL.md).

## Regression policy

An absolute release-ceiling failure blocks P0 acceptance. Even below the
ceiling, a repeatable regression against the last accepted Wingman baseline
triggers investigation when it exceeds:

- 10% for launch, input, resize, runner dispatch, or cancellation time;
- 15% for settled memory, idle CPU, installed size, or endurance growth;
- 20% for bulk output or runner throughput.

A regression is considered repeatable only after the standardized suite
reproduces it in three independent runs. Fix correctness, security, and data
integrity defects before optimizing; performance work may not remove input
validation, CSP, request authentication, cancellation, or safety checks.

An exception requires a documented cause, user-visible effect, mitigation, and
explicit approval. A faster development machine is not a valid exception.

## WebView replacement trigger

WebView2 is retained for P0 unless profiling shows it is the blocking source.
The response to a failed budget is:

1. reproduce with the standardized suite;
2. identify host, WebView, renderer, IPC, PTY, shell, or runner cost with traces;
3. remove redundant work, WebViews, IPC, retained DOM/JS state, or polling;
4. remeasure the release build;
5. if launch or idle-memory ceilings still fail materially, perform a separate
   native-renderer spike and compare it with the same black-box suite.

A renderer rewrite is considered when a profiled WebView cost alone keeps the
release candidate more than 20% beyond a hard ceiling and ordinary optimization
cannot close the gap. Such a rewrite still requires user approval under the
implementation gate; the performance contract does not authorize it.

## Prototype gaps before measurement

The prototype now has release-only whole-process-tree accounting for the settled
Windows PowerShell idle case, an authenticated editor-readiness marker,
environment-gated accepted-and-rendered normal-input and bulk input-latency
probes, a deterministic 100,000-line/10-MiB PTY completeness probe, and a
release whole-process-tree retained-memory distribution after clear. It still
lacks a matched Windows Terminal comparison, an explicit scrollback ceiling,
runner timing, resource limits, and endurance automation. These are planned
measurement needs, not permission to add production instrumentation before
implementation approval.

## Research basis

- [Microsoft: Plan and measure app performance](https://learn.microsoft.com/en-us/windows/apps/develop/performance/planning-measuring-performance)
- [Microsoft: WebView2 performance best practices](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/performance)
- [Microsoft: WebView2 process model](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-model)
