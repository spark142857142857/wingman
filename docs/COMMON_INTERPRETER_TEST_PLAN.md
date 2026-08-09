# Common Interpreter Test Plan (Draft)

Status: accepted test direction. This document defines the target acceptance suite; it does not authorize implementation.

## Test-case shape

```text
TestCase {
  id, initial_file_tree, current_directory, familiar_mode, active_shell, input_line,
  expected_classification, expected_stdout, expected_stderr, expected_exit_code,
  expected_final_file_tree
}
```

Tests assert classification, streams, exit status, and final filesystem state.

## A. Pure contract suite

Runs without Windows shell or PTY dependencies. It covers input ownership classification, lexer and parser structure, command-catalog validation and safety checks, execution-plan generation, deterministic diagnostics, and exit behavior.

Required examples include native pass-through, P0 execution, claimed unsupported syntax, quotes, redirection structure, and unqualified `.exe` bypass behavior.

Boundary assertions prove that pass-through creates no stored request, while
rejection, execution, and reserved controls return only an unpredictable ID and
display line to the frontend. No serialized plan, path, pattern, or diagnostic
payload appears in `FrontendDecisionV1`. Session and command-sequence envelope
fields must match the active prompt; stale or mismatched decisions are ignored.

## B. Runner filesystem suite

Runs the runner against a disposable, verified temporary Windows test root. Fixtures include empty files, UTF-8 and UTF-8-BOM files, undecodable input, recursive source trees, hidden and read-only items, spaces, Korean paths, and safe reparse-point cases where available.

The full accepted/rejected path-shape, hard-link identity, reparse, root,
containment, redirection-alias, and controlled race matrix comes from the
[Windows path contract](WINDOWS_PATH_CONTRACT.md).

The full global-preflight, multi-operand order, staging/commit, cleanup,
partial-result, cancellation, and exit-aggregation matrix comes from the
[mutation execution contract](MUTATION_EXECUTION_CONTRACT.md). Tests must
distinguish known safety rejection with no mutation (`2`), unavailable safety
evidence with no mutation (`1`), operational partial work (`1`), and cancelled
partial work (`130`).

It validates every P0 command's output, exit status, and final filesystem tree. Destructive tests must prove their absolute targets remain inside the test root before execution and during cleanup.

Command-detail fixtures additionally lock the complete P0 `grep` regex/class
grammar and Unicode folding, recursive order and displayed paths; `find` glob
grammar, depth/type/reparse behavior, path form, and preorder; exact `sort -n`
decimal parsing/stability; every `ls -l/-h` field and rounding boundary; `which`
cwd/PATH/PATHEXT ordering; and startup versus runtime multi-source failures.

## C. Pipeline, redirection, and status suite

The full decoder, BOM, `RecordFrame`, newline, final-terminator, transform,
redirection-open, bounded-channel, backpressure, short-circuit, `tail -f`,
partial-output, and outcome-priority matrix comes from the [text record and
stream contract](TEXT_STREAM_MODEL.md).

It verifies supported text pipelines, `>` and `>>`, stdout/stderr separation,
final-stage result statuses, fatal upstream failures, deterministic primary
diagnostics, normal upstream stop, and Ctrl+C exit `130`.

Required cases include `cat app.log | grep TODO | head -n 1`, `find src -type f -name "*.ts" | wc -l`, `grep NOTHING app.log`, `grep NOTHING app.log | head -n 5`, `cat missing.txt | head -n 5`, invalid UTF-8 before and after early `head`, both output redirect modes, final unterminated input, bounded slow-consumer flow, and `tail -f app.log` cancellation.

## D. Shell transport suite

Runs the same P0 fixtures through `cmd` and Windows PowerShell. It validates current filesystem directory, `PATH`, and UTF-8 inheritance; Familiar on and off; native command pass-through; cancellation forwarding; PowerShell FileSystem location acceptance; and the non-FileSystem provider guard.

The transport matrix also proves one-shot broker consumption for prepared
Reject, Execute, and Control variants, their stdout/stderr/exit behavior, and
rejection of a repeated or mismatched ID.

The guard must demonstrate that `Set-Location HKLM:\` followed by a P0 file command fails clearly instead of using an older inherited directory.

The application-launch matrix follows the [CLI launch
contract](CLI_LAUNCH_CONTRACT.md): public grammar, cwd/environment/token
inheritance, same-binary protected GUI role, `Ready`/`Failed` acknowledgement,
independent child lifetime, timeout/Ctrl+C, no orphan, and direct internal-mode
rejection are exercised from both shells.

## E. Native preservation suite

With Familiar on, native PowerShell cmdlets, cmd built-ins, shell variables, and state commands remain raw input. With Familiar off, every input is raw pass-through, including P0-looking unsupported syntax.

## F. Terminal submission and session suite

The exact automated, integration, and boundary-spike matrix follows the
[terminal submission and session contract](TERMINAL_SESSION_CONTRACT.md). It
covers validated prompt markers and session states; foreground interactive
pass-through; Unicode/IME editing; completion and unknown-edit fallback; raw
visible recall versus native history; single- and multiline paste confirmation;
confirmed nested `cmd`/PowerShell transitions; fixed-invocation replacement;
session restart; and `Ctrl+C`.

Required negative cases prove that prompt-looking output, stale or malformed
markers, Tab or history search, a multiline paste, and input while a foreground
program is active cannot reach `prepare_submission`.

## G. Manual application smoke suite

Manual checks cover UI and PTY behavior unsuitable for reliable automation: startup, fonts, focus, resize, editing, raw history, paste reliability fallback, Ctrl+C during `tail -f`, shell switching, Korean text and paths, diagnostic visibility under redirection, and session restart isolation.

## Acceptance gate

P0 is not complete until all of the following pass:

```text
[ ] A-C automated suites
[ ] D cmd and PowerShell matrix
[ ] supported-Windows update canary
[ ] E native preservation regression
[ ] F terminal submission/session matrix
[ ] G manual smoke suite
[ ] performance budget and regression suite
[ ] documentation matches observed behavior
[ ] implementation-gate re-review is complete
[ ] user approves implementation and final acceptance
```

Compare output exactly only where a Wingman contract promises it. Do not freeze locale-specific diagnostics or behavior of raw native pass-through commands.
