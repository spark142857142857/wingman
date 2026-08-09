# Common Interpreter Data Model (Draft)

Status: accepted design direction; names may change during implementation, but
the ownership and data boundaries are intentional.

## Semantic ownership decision

```text
OwnershipDecision = PassThrough | Reject | Execute
```

- `PassThrough` means the original line belongs to the active shell.
- `Reject` means Wingman owns the P0-looking line but its syntax, options, or
  safety shape are outside contract.
- `Execute` means Wingman owns the line and produced a fully validated,
  shell-independent execution plan.

This is an internal semantic result, not the value returned to the WebView.
Reserved Familiar controls are detected before this ownership decision.

## Frontend decision

```text
FrontendDecisionV1 {
  session_id,
  command_sequence,
  decision:
      PassThrough     { raw_line }
    | InvokePrepared  { request_id, display_line }
}
```

- The envelope IDs bind the result to one active prompt and submission. A stale
  or mismatched result is discarded and cannot replace a later editor buffer.
- `PassThrough` returns the authoritative original line for unchanged shell
  submission. It creates no broker entry.
- `Reject`, `Execute`, and a recognised Familiar control become a prepared
  request stored in Rust session memory. The frontend receives only its short,
  unpredictable `request_id` and the original `display_line`.
- `display_line` is retained for Wingman's visible recall history. The fixed
  internal runner invocation must not replace it.

The [terminal submission and session contract](TERMINAL_SESSION_CONTRACT.md)
is a precondition to this decision. The WebView cannot declare prompt or line
reliability. Rust binds the decision to the exact mirrored line and sequence.
On `PassThrough`, the native editor already contains `raw_line`, so the
frontend treats it as a consistency value and forwards Enter only; it does not
resend the text.

Other than the session/sequence envelope and one-shot ID, no parsed command,
diagnostic payload, path, pattern, execution plan, serialized request, broker
endpoint, or request secret crosses the Rust-to-WebView decision boundary.

## Parsing model

```text
ParsedLine {
  stages: ParsedCommand[],
  redirect: Redirect | null
}

ParsedCommand {
  name: string,
  arguments: string[]
}

Redirect {
  mode: Overwrite | Append,
  path: string
}
```

The parser records words, quotes, pipeline boundaries, and the final output
redirection only. It does not assign command meaning, expand a glob or an
environment variable, or interpret general shell syntax.

## Validated path model

```text
ValidatedPathSpec {
  original: string,
  kind: Relative | DriveAbsolute | UncAbsolute,
  components: string[]
}

ResolvedPath {
  absolute_native: string,
  identity: FileIdentity | null
}
```

The host catalog converts path operands into `ValidatedPathSpec` under the
[Windows path contract](WINDOWS_PATH_CONTRACT.md). It validates shape but does
not resolve a relative path from the host process. The runner revalidates the
specification, inherits the active shell's filesystem cwd, and creates
`ResolvedPath` immediately before operation-specific checks. `FileIdentity` is
acquired only when the filesystem object exists and an identity check is needed.

Patterns are separate command-specific values, never `ValidatedPathSpec`.
Neither `ResolvedPath` nor `FileIdentity` is serialized in `ExecutionPlan` or
returned to the WebView.

```text
ValidatedRedirect {
  mode: Overwrite | Append,
  path: ValidatedPathSpec
}
```

The catalog converts the parser's raw `Redirect` into this validated form.

## Validated execution model

```text
ExecutionPlan {
  stages: StagePlan[],
  redirect: ValidatedRedirect | null
}
```

`StagePlan` is a tagged command-specific type rather than a generic command
string. Representative forms are:

```text
ReadTextFiles { paths, number_lines }
HeadLines { count, path }
TailLines { count, path }
CountLines { path }
ListDirectory { path, include_hidden, long_format, human_sizes }
SearchText {
  pattern, paths, source,
  ignore_case, line_numbers, invert, fixed_string, recursive
}
FindPaths { start_path, kind, name_pattern, case_mode, min_depth, max_depth }
RemovePaths { paths, recursive, force }
SortLines { reverse, numeric, unique, source }
UniqueLines { count, duplicates_only, unique_only, source }
```

The command catalog creates these values and applies every command contract's
option, source, safety, and exit-code checks before the runner sees them.
Environment-dependent path, identity, and reparse checks remain runner work.

## Pipeline compatibility

Runtime text edges carry bounded `RecordFrame { text, terminated }` values,
not byte chunks or shell objects. The [text record and stream
contract](TEXT_STREAM_MODEL.md) owns decoding, framing, final encoding,
backpressure, short-circuit, and outcome priority; command metadata only says
whether a stage may connect to those edges.

Catalog metadata declares whether a command can consume text from a preceding
stage and whether it emits text for a following stage.

| Group | Text input | Text output |
| --- | --- | --- |
| `cat`, `ls`, `find` | no | yes |
| `grep`, `head`, `tail`, `wc`, `sort`, `uniq` | contract-specific | yes |
| `mkdir`, `touch`, `cp`, `mv`, `rm`, `clear`, `which` | no | usually no |

Validation rejects impossible or unpromised combinations before execution, for
example `rm temp | grep error` or `grep TODO app.txt | mkdir logs`.

## Prepared runner request

```text
PreparedRequestV1 {
  protocol: "wingman.run",
  version: 1,
  kind:
      Reject  { diagnostic, exit_code: 2 }
    | Execute { plan: ExecutionPlan }
    | Control { response, exit_code }
}
```

Rust stores this value under the request ID. The active shell receives only a
fixed runner invocation and that ID. After the runner connects, the broker
atomically consumes the entry and serializes `PreparedRequestV1` over the local
session pipe. The frontend never transports or serializes this value.

For a Familiar control, the Rust host applies the validated application-state
change and prepares only its response and exit status for shell-visible output.
For a rejection, the runner prints the prepared diagnostic and returns `2`
without executing a plan. For execution, the runner validates the plan again.

The request deliberately has no current-working-directory field. The runner is
started as a child of the active shell and inherits its real current filesystem
directory, environment, `PATH`, and access token.
