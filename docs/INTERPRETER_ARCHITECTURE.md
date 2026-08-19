# Common Interpreter Architecture

Status: current P0 architecture, implemented by the release candidate.

## Principle

> The shell owns state. Wingman owns the agreed Unix-command meaning.

PowerShell and `cmd` remain the active native shells. They own their current
location, environment, session features, and native command behavior. Wingman
only owns the bounded P0 compatibility grammar and the command contracts in
this repository.

## Decision flow

```text
submitted input line
  -> Familiar mode off?             pass through raw input
  -> native state command?          pass through raw input
  -> recognised P0 Unix candidate?  parse and validate
       -> valid                    execute common plan
       -> invalid claimed syntax    show a Wingman diagnostic
  -> otherwise                      pass through raw input
```

Native state commands include `cd`, `chdir`, `pushd`, `popd`, and PowerShell
`Set-Location`. Wingman does not translate them.

## Layers

1. **Input classifier**: decides raw pass-through, Wingman diagnostic, or
   common interpretation. It acts only on a reliably captured, submitted line.
2. **Constrained lexer and parser**: accepts the P0 one-line grammar only:
   words, single/double quotes, `--`, `|`, and one final `>` or `>>`.
3. **Command catalog and validator**: turns generic parsed words into a
   contract-valid command, rejects unsupported options and unsafe requests,
   and assigns the documented exit behavior.
4. **Common execution plan**: an unambiguous, shell-independent representation
   of the requested work.
5. **Wingman runner**: executes that plan with Windows filesystem, process,
   and ACL semantics, moves structured text records through bounded pipelines,
   writes output and diagnostics, and returns the planned exit code.

The parser does not implement Bash syntax. Command substitution, environment
expansion, glob expansion, `&&`, `||`, `;`, input/error redirection, and other
non-P0 constructs are outside it.

## Shell boundary

Rust owns the prompt/session tracker. Only a validated prompt plus an
allowlisted, mirrored edit sequence creates a reliable submitted line. The
frontend forwards opaque, session-tagged input bytes; Rust atomically chooses
native input or registers and writes a prepared-request invocation. While a
command or foreground program is running, all terminal input passes through.
The exact state and fallback rules are defined in the [terminal submission and
session contract](TERMINAL_SESSION_CONTRACT.md).

Rust retains every rejection, control response, and execution plan in session
memory; no plan is returned to the WebView. The runner is launched as a child
of the active shell so it inherits the actual current filesystem directory,
`PATH`, environment, and access token after native state commands such as `cd`.

Shell-specific code must not parse P0 options or define independent command
semantics. It may only transport a validated runner request safely. The
transport must not interpolate user paths or patterns into shell source; use a
versioned opaque request encoding.

## Required consistency

- The common plan and runner semantics are shell-independent. In P0, only the
  validated Windows PowerShell adapter intercepts Familiar input; `cmd` remains
  native pass-through and does not claim the P0 grammar.
- A claimed P0 command with unsupported syntax fails clearly; it is not partly
  converted or silently guessed.
- Raw native commands and native shell state commands remain available.
- P0 adds no frontend-managed command history. Native history remains available
  and may contain an internal runner invocation.
- Cancellation, output streaming, redirection, and errors are validated by
  tests at the runner boundary.

## Implementation status

The release candidate uses one Rust catalog, parser, execution-plan format, and
runner. The frontend no longer owns compatibility parsing, the writable legacy
PowerShell profile is not loaded, and `cmd` compatibility mappings are removed.
The packaged PowerShell adapter only proves editor readiness, clears the editor
buffer, and launches the fixed opaque request invocation.

See [the common interpreter data model](INTERPRETER_DATA_MODEL.md) for the
decision, parsing, execution-plan, and runner-request boundaries.
See [the input classification contract](INPUT_CLASSIFICATION.md) for the
ownership decision that precedes parsing.
Prompt evidence, Unicode-safe input mirroring, completion fallback, paste,
history, and shell transitions follow [the terminal submission and session
contract](TERMINAL_SESSION_CONTRACT.md).
See [the lexer contract](LEXER_CONTRACT.md) for the constrained token rules.
The historical implementation authorization is recorded in
[the implementation gate](IMPLEMENTATION_GATE.md).
Update verification and support policy are defined in [the compatibility
maintenance contract](COMPATIBILITY_MAINTENANCE.md).
Runner I/O, cancellation, and exit behavior are defined in [the runner
execution contract](RUNNER_EXECUTION_CONTRACT.md).
UTF-8 decoding, BOM/newline records, pipeline backpressure, short-circuit,
redirection sinks, and fatal priority follow [the text record and stream
contract](TEXT_STREAM_MODEL.md).
Privilege, WebView, transport, terminal-data, and update boundaries are defined
in [the security and trust model](SECURITY_MODEL.md).
Accepted path forms, runner-side resolution, identity, and reparse behavior are
defined in [the Windows path and filesystem contract](WINDOWS_PATH_CONTRACT.md).
User-visible latency, resource ceilings, baselines, and the renderer replacement
trigger are defined in [the performance budget](PERFORMANCE_BUDGET.md).
The completed legacy-to-runner transition is recorded in
[the migration plan](MIGRATION_PLAN.md).
Mutating-request preflight, ordering, staging, commit, and partial-result rules
are defined in [the mutation execution contract](MUTATION_EXECUTION_CONTRACT.md).
