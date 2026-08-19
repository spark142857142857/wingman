# Wingman Release Test Matrix

Korean version: [RELEASE_TEST_MATRIX.ko.md](RELEASE_TEST_MATRIX.ko.md)

Status: current P0 release-verification authority. The historical prototype
matrix remains in [TEST_MATRIX.md](TEST_MATRIX.md) only as migration evidence.

## Scope under test

- Windows PowerShell 5.1 is the only P0 shell with validated Familiar
  interception.
- `cmd.exe` is a supported native terminal session, but all of its input is
  native pass-through. A `cmd` performance probe does not imply Familiar
  command conversion.
- Familiar starts `PAUSED`. A validated PowerShell prompt may run
  `familiar on`, `familiar off`, and `familiar status`.
- The P0 command surface and grammar come from
  [COMPATIBILITY_CONTRACT.md](COMPATIBILITY_CONTRACT.md). Prototype-only
  `cut`, `tr`, `sed`, and `xargs` behavior is not a release feature.
- A test passes only the behavior it directly observes. Unit coverage does not
  replace the packaged sidecar, GUI, installer, manual, or external matrix.

## Evidence states

| State | Meaning |
| --- | --- |
| Pass | The listed authoritative command or observation passed for the exact release candidate. |
| Fail | The observed behavior contradicted its contract or ceiling. |
| External | The item needs a different boot, OS, machine, credential, or authorized context. |
| Not run | No current-candidate evidence has been collected. |

Historical results are recorded in
[PERFORMANCE_BASELINES.md](PERFORMANCE_BASELINES.md). A final candidate must be
run again; an older pass is not silently promoted to the new commit.

## 1. Deterministic source gate

Run from the repository root in an ordinary, non-elevated PowerShell session:

```powershell
npm ci
npm run typecheck
npm test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
```

| Area | Authoritative coverage |
| --- | --- |
| Frontend input | `terminal-input`, `terminal-paste`, `terminal-shortcuts`, and `terminal-security` TypeScript tests |
| Windows layout | `layout_regression.tests.ps1` |
| Sidecar layout | `sidecar_packaging.tests.ps1` |
| Rust contracts | Every non-ignored unit and integration test under `src-tauri/src` and `src-tauri/tests` |
| Frontend artifact | TypeScript typecheck and Vite production build |
| Rust hygiene | `cargo fmt --check` and warning-free Clippy |

`npm ci` is a clean dependency-reconstruction prerequisite, not proof that the
application works.

## 2. Contract-to-test map

| Contract surface | Primary automated evidence |
| --- | --- |
| Ownership, lexer, parser, catalog | `lexer_contract`, `parser_contract`, `catalog_contract`, `frontend_decision_contract` |
| `pwd`, `clear`, `which`, `ls`/`ll`, `mkdir`, `touch` | `clear_contract`, `which_contract`, `ls_contract`, `mkdir_contract`, `touch_contract` |
| `find` and pattern grammar | `find_contract`, `find_pattern_contract`, `runner_readonly_contract` |
| `grep` and pattern grammar | `grep_pattern_contract`, `runner_grep_contract` |
| `cat`, `head`, `tail`, `wc -l`, `sort`, `uniq` | `runner_readonly_contract`, `runner_dispatch_contract`, `sort_support` unit tests |
| `cp`, `mv`, `rm` | `cp_contract`, `mv_contract`, `rm_contract` and mutation resource tests |
| Pipelines, redirection, records, cancellation | `pipeline_contract`, `text_stream_contract`, `ordered_pipeline` unit tests, `runner_io_contract`, `runner_process_contract` |
| Windows paths, reparse points, races | `windows_path_contract`, `runner_io_contract`, command-specific filesystem suites |
| Prepared request and runner transport | `runner_transport_contract`, `powershell_runner_transport_contract`, `session_broker_contract`, `named_pipe_security_contract` |
| Prompt readiness and editor replacement | `editor_readiness_contract`, `shell_adapter_contract`, `oob_vertical_contract` |
| Session generation and input ordering | `terminal_session_contract`, `session_input_contract`, `session_runtime_contract`, `pty_output_flow` unit tests |
| Public CLI and protected GUI handoff | `cli_launch_contract` plus the black-box CLI suite below |

The map is navigational. Acceptance still requires the complete test command,
because cross-module regressions are not assigned to a single row.

## 3. Release artifact and security gate

Build the exact candidate before running any artifact test:

```powershell
npm run tauri build
npm run test:release-bundle
npm run test:release-security
npm run test:cli-launch
npm run test:installer
npm run test:app-data
```

| Gate | Required evidence |
| --- | --- |
| Bundle | Packaged GUI and runner exist at the protected installed layout; development-only scripts are absent. |
| Security | Release binary roles, runtime assets, pipe ACL/security context, and direct internal-role rejection satisfy their contracts. |
| CLI | PowerShell and `cmd` callers cover help/version, valid shell/path launch, invalid syntax/path, handoff, child lifetime, timeout/Ctrl+C, and no orphan. |
| Installer | Install, reinstall, launch registration, Unicode/space install path, uninstall, and no unrelated deletion pass. Installed tree is at most 60 MiB. |
| App data | 100 isolated interactive PowerShell launches complete and the isolated local profile remains at most 100 MiB. |

Code signing is checked separately because the local development certificate is
not the final release identity.

## 4. Release performance and resource gate

### Runner component

```powershell
cargo test --release --manifest-path src-tauri/Cargo.toml --test runner_performance_contract cached_runner_timing_baseline -- --ignored --exact --nocapture
cargo test --release --manifest-path src-tauri/Cargo.toml --test runner_process_contract idle_tail_follow_runner_stays_below_the_cpu_ceiling -- --ignored --exact --nocapture
cargo test --release --manifest-path src-tauri/Cargo.toml --test runner_resource_contract sort_resource_limit_stays_bounded_and_fails_closed -- --ignored --exact --nocapture
cargo test --release --manifest-path src-tauri/Cargo.toml --test runner_resource_contract traversal_and_listing_resource_limits_are_bounded -- --ignored --exact --nocapture
cargo test --release --manifest-path src-tauri/Cargo.toml --test runner_mutation_resource_contract mutation_resource_limits_are_bounded_and_atomic -- --ignored --exact --nocapture
```

### GUI matrix

Run every shell-parameterized script once with `powershell` and once with
`cmd`. The `cmd` scripts exercise terminal transport and rendering with a
test-only native workload; they do not activate Familiar interception.

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

PowerShell additionally runs the authenticated editor and 30-minute endurance
paths:

```powershell
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_editor_readiness.tests.ps1 -Executable $Exe -TimeoutSeconds 30
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File src-tauri/tests/release_endurance.tests.ps1 -Executable $Exe
```

All ceilings and sampling rules come from
[PERFORMANCE_BUDGET.md](PERFORMANCE_BUDGET.md). The harness must emit its raw,
versioned result; a window that merely stayed open is not a pass.

## 5. Manual application gate

Run [RELEASE_SMOKE_TEST.md](RELEASE_SMOKE_TEST.md) against the same release
artifact after the automated gates. Record the Windows build, display scale,
shell, install/build path, commit, and every failed checklist ID. Visual checks
and real IME/clipboard behavior may not be waived by headless tests.

## 6. External release matrix

The following remain `External` until direct evidence exists:

| Item | Required evidence |
| --- | --- |
| Cold startup | Five controlled-restart samples of the exact signed candidate |
| Uncached runner | Five unique recent boots per workload through `release_uncached_runner.tests.ps1` |
| Windows support | Current and previous supported Windows releases |
| Hardware | Reference and minimum supported hardware tiers |
| Signing | Valid final Authenticode chain and clean signature verification of every executable/installer |
| Privilege/session scope | Any elevated, administrator, cross-login-session, or multi-user behavior included in the release promise |

Warm samples, compatibility mode, or a different commit cannot substitute for
these entries.

## Final acceptance record

For the release commit, store one row per section:

```text
Release commit:
Artifact SHA-256:
Windows build and hardware:
1 deterministic source gate: Pass / Fail
2 contract coverage audit: Pass / Fail
3 artifact and security gate: Pass / Fail
4 performance and resource gate: Pass / Fail
5 manual application gate: Pass / Fail
6 external matrix: Pass / External / Fail
Known limitations:
Reviewer and date:
```

An item marked `External` keeps that part of the release claim open; it is not a
conditional pass.
