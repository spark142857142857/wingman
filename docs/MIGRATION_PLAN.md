# Common Interpreter Migration Plan (Draft)

Status: accepted migration direction. This plan does not authorize implementation.

## Scope

Preserve the terminal UI, xterm integration, ConPTY/PTY session management,
resize behavior, and Familiar UI state where the boundary spikes prove them.
Replace heuristic prompt/input and speculative shell-stack tracking as well as
the duplicated compatibility layer: frontend `cmd` string mapping,
command-specific PowerShell functions, shell-specific option parsing, and shell
command-string construction.

## Gate

Before implementation, re-review all product, command, parser, runner, maintenance, and test contracts as one proposal. Resolve contradictions and migration risks, present the consolidated plan, and receive explicit user approval under the implementation gate.

## Phase 1: boundary spikes

Before command implementation, verify these risky boundaries:

1. Packaged out-of-band readiness distinguishes root and same-process nested
   PowerShell editing from native foreground children; `cmd` remains native.
2. Unicode-safe mirroring, completion/history fallback, multiline paste, and
   `Ctrl+C` follow the terminal session contract.
3. Fixed runner-invocation replacement does not damage the prompt, first output,
   or next prompt; a short visible invocation and native-history entry are safe
   P0 fallbacks.
4. A separate `wingman-runner.exe` packages and launches reliably with Tauri.
5. It inherits the active shell's current filesystem directory and environment.
6. A request ID transports a validated plan without shell interpolation.
7. The same-binary launcher/GUI handoff satisfies the CLI launch contract in
   `cmd` and PowerShell without an orphan, console child, or lost startup error.
8. The first release-build performance calibration is recorded; its accepted
   P0 ceilings are then frozen rather than repeatedly relaxed during migration.

## Phase 2: shared pure core

Create one shared Rust implementation for session evidence, classification,
lexing, parsing, catalog validation, validated path values, execution-plan and
protocol types, diagnostics, and resource-limit constants. Do not connect real
filesystem mutations or P0 command implementations in this phase. TypeScript
forwards input events and asks Rust for a decision only when the terminal
session contract permits; it neither asserts prompt reliability nor parses P0
options. The runner later validates the same plan types defensively.
Rust returns either the authoritative pass-through line or an opaque one-shot
request ID; TypeScript never receives a serialized plan or prepared diagnostic.
The shared library also owns `ValidatedPathSpec`; only the runner resolves it
against inherited shell cwd and acquires filesystem identity.
This phase passes the pure-contract and protocol serialization suites before
process or filesystem behavior is added.

## Phase 3: runner, broker, and shell-transport skeleton

Package the dedicated runner and implement the local broker and one-shot request
ID design in [the runner transport contract](RUNNER_TRANSPORT.md). Use test-only
prepared operations to prove protocol validation, current directory,
environment, token, stdout/stderr/status, expiry, replay rejection, restart, and
cancellation. No real P0 command is required yet.

Replace the command-specific PowerShell profile with protected packaged prompt
integration and a small transport shim that reports filesystem versus
non-filesystem location, invokes the runner, and preserves its exit status.
`cmd` receives equivalent prompt integration and transports a request only. No
shell shim owns P0 option meaning.

Prompt, editing, paste, recall, and transition behavior follows the [terminal
submission and session contract](TERMINAL_SESSION_CONTRACT.md).

## Phase 4: runner stream and pipeline engine

Before migrating any real command, implement and test the shared `RecordFrame`
UTF-8/BOM decoder, file and test sources, transforms, bounded channels,
backpressure, final sink, `>`/`>>` open order, same-file rejection, stage outcome
priority, normal downstream stop, resource limits, and Ctrl+C cancellation.

Use synthetic test stages and disposable fixtures to pass the pipeline,
redirection, fatal/result status, partial-output, and cancellation suites. Only
this engine may carry migrated text commands; no command receives a raw-byte or
shell-pipeline shortcut.

## Phase 5: read-only and control commands

1. `pwd`, `which`, and `clear`
2. `cat`, `head`, `tail`, and `wc`
3. `grep`
4. `sort` and `uniq`
5. `ls` and `find`

Each group must pass its exact command contract through the already-tested
runner, pipeline, redirection, cancellation, and both-shell transport paths
before the next group begins.

## Phase 6: filesystem mutation

1. `mkdir` and `touch`
2. staged `cp` and `mv`
3. `rm`

Every group follows the [mutation execution
contract](MUTATION_EXECUTION_CONTRACT.md). Destructive `rm` is connected last,
only after global preflight, path/identity, reparse, root/ancestor, staging,
partial-failure, cancellation, and controlled-race tests pass.

## Phase 7: controlled cutover

An internal development flag may temporarily compare legacy and common-v1 paths. It is not a permanent user-facing dual engine. Once the new P0 matrix passes, remove frontend `cmd` mapping, command-specific PowerShell compatibility functions, legacy behavior tests, and the temporary flag.

The intended large change is a replacement of the compatibility subsystem, not an unnecessary rewrite of stable terminal and PTY foundations.

Application command registration and launch behavior are defined in [the CLI launch contract](CLI_LAUNCH_CONTRACT.md).
