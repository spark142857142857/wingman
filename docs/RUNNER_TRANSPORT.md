# Runner Transport Contract

Status: current P0 transport contract, implemented by the release candidate.

## Decision

Use a local, session-scoped named-pipe broker and a short unpredictable request
ID. Do not embed raw user input or a serialized plan in shell source, command-line
arguments, or per-request environment variables. A one-shot protected request
file is a fallback, not the preferred transport.

## Flow

```text
reliable raw input
  -> Rust prepare_submission(session_id, command_sequence, raw_line)
  -> semantic PassThrough | Reject | Execute, or reserved Control
  -> PassThrough: return authoritative raw line; store nothing
  -> otherwise: store PreparedRequestV1 in Rust session memory
  -> return the matching session/sequence envelope with InvokePrepared,
     display line, and unpredictable request ID
  -> active shell launches wingman-runner with that ID
  -> runner connects to the local broker
  -> broker consumes and removes the request once
  -> broker sends PreparedRequestV1 over the local pipe
  -> runner revalidates and executes, prints rejection, or prints control response
```

Only a safe fixed runner path, request ID, and fixed location-kind flag appear
in shell source. User paths, patterns, diagnostics, controls, and execution plans
remain in Rust until they travel through the broker pipe. They never cross the
WebView boundary or appear in a command-line argument.

Every decision is bound to the active session and command sequence; stale or
mismatched results are discarded. `PassThrough` creates no request ID.
`Reject`, `Execute`, and `Control` all use
the same prepared-request path so the native shell receives the runner's stdout,
stderr, and exit status consistently.

## Broker lifecycle and security

- One local broker exists per Wingman session.
- Pipe naming includes a session-specific random component and local-session scope.
- Access is restricted to the current login session and user; remote access is denied.
- Request IDs are unpredictable, one-shot, and expire after a short timeout.
- Shell restart or Wingman shutdown invalidates all outstanding session requests.
- Unknown, expired, repeated, or protocol-incompatible requests never execute.
- The runner validates the received protocol and prepared kind again. It
  defensively validates an execution plan before filesystem access.

## Implemented defensive validation (2026-08-09)

The runner boundary now rejects unknown nested fields and revalidates every
typed field before dispatch. The shared host/runner limits are:

- 64 KiB maximum serialized prepared request;
- 16 pipeline stages and 128 total path operands, including redirection;
- 4 KiB prepared diagnostic and 256-byte control response;
- non-empty diagnostic/control text with no terminal control characters;
- fixed status `2` for Reject and `0` for Control;
- a maximum `head` count of `4,294,967,295`;
- exact reconstruction of every serialized `ValidatedPathSpecV1`; and
- only catalog-valid source and downstream stage shapes.

The runner performs the validation both after decoding and again at the direct
execution entry point. A rejected request emits one fixed bounded diagnostic
and never echoes request contents. The test-only environment-probe execution
variant has been removed; process-boundary inheritance is tested with the real
working-directory operation instead.

This completes request validation. Typed `cat`/`head`/finite `tail -n N`/single-file `tail -f`/`wc -l`/`grep` plans now use the
production streaming runner, and typed `>`/`>>` plans use the same record stream
through the safe prepared file sink. Reliable Familiar-on PowerShell input now
classifies `cat`, `head`, finite `tail -n N`, single-file `tail -f`, `wc -l`, and `grep` through the shared lexer/parser/catalog and sends
only the opaque prepared request ID through the fixed editor replacement.
The sidecar's shared cancellation token and Windows console control handler
cover both terminal and redirected sinks; an actual process-group test cancels
the real sidecar during redirected streaming. A PowerShell/ConPTY vertical test
also proves Unicode-path redirection and the next OOB readiness cycle. `cmd`
remains outside interception until it has a proved editor adapter.

## Location metadata

`cmd` invokes the runner with a fixed filesystem location kind. A minimal
PowerShell transport shim reports either `filesystem` or `non-filesystem`,
without interpolating the provider path into shell source. The runner applies
the plan's filesystem requirements after receiving it.

## Packaging

The application binary is `wingman.exe`; the dedicated sidecar is
`wingman-runner.exe`. Both are Cargo binary targets in the same package, so the
Tauri bundler installs the runner beside the application binary. It must not
also be declared in `bundle.externalBin`, which would duplicate the installed
file. The fixed PowerShell transport is compiled into `wingman.exe` and passed
as application-owned `-Command` source, so no writable `.ps1` support file or
process-wide execution-policy bypass is installed. Wingman exposes the
installed runner's application-controlled absolute path to child shell
sessions through a session environment variable.

## Presentation and history

The user's raw input is the authoritative mirrored consistency value before
replacement. P0 has no Wingman-owned frontend command history; native shell
history may contain the internal runner invocation. Native editor contents,
prompt synchronization, and the exact replacement operation follow the
[terminal submission and session contract](TERMINAL_SESSION_CONTRACT.md).

Hiding the invocation echo is a separate boundary spike. It may ship only if it
proves that it suppresses exactly the generated echo in PowerShell,
preserves the runner's first output and next prompt, survives Ctrl+C, and does
not corrupt shell line editing. The safe fallback is a short visible internal
invocation rather than a brittle PTY output filter.
