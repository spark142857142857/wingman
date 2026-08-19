# Runner Execution Contract (Draft)

Status: accepted design direction for P0 execution; implementation remains gated.

## Shell boundary and filesystem location

The runner executes only from a real Windows filesystem location.

- In `cmd`, the child runner uses the shell's current Windows directory.
- In PowerShell, the transport shim checks that `Get-Location` is a FileSystem
  provider location before starting the runner.
- At a non-filesystem PowerShell location such as `HKLM:\`, the shim does not
  start the runner. It emits a clear filesystem-location diagnostic and exits
  with code `1`.

This guard prevents a native child process from accidentally inheriting an
older process directory while PowerShell is located in a non-filesystem
provider.

The host stores only validated path syntax. The runner revalidates and resolves
it from this inherited location under the
[Windows path and filesystem contract](WINDOWS_PATH_CONTRACT.md); host-process
cwd and frontend-provided absolute paths are never substituted.

## Request execution

```text
shell transport
  -> one-shot request ID
  -> broker returns PreparedRequestV1
  -> protocol-version check
  -> Reject: print prepared diagnostic and exit 2
  -> Control: print prepared response and return its prepared status
  -> Execute: defensive plan validation
       -> inherited cwd, environment, PATH, and access token
       -> P0 execution
  -> stdout, stderr, and exit code
```

The runner validates the prepared request independently of both the frontend
and the Rust host's earlier validation. Invalid request shape or protocol
mismatch exits `2`; actual environment and filesystem failures exit `1` unless
an individual command contract explicitly says otherwise. The runner never
accepts a plan directly from the frontend or a command-line argument.

## Text stream

P0 uses the structured `RecordFrame { text, terminated }` pipeline in the
[text record and stream contract](TEXT_STREAM_MODEL.md), never a PowerShell
object stream, native shell pipe, or raw command-specific byte bypass. One
streaming decoder handles split UTF-8, optional source BOM, LF/CRLF framing,
invalid input, final unterminated records, and record bounds. `cat` streams
decoded records; `sort` materializes only its bounded logical input.

Only the final sink encodes BOM-free UTF-8 and CRLF. P0 is text-only and does
not promise binary copying or byte-for-byte input newline preservation.

## Output and redirection

- Normal data is stdout; Wingman diagnostics are stderr.
- `>` creates or truncates its final target. `>>` creates or appends to it.
- Only stdout is redirected. Diagnostics remain visible in the terminal.
- Missing parent directories, a directory target, or another output-open
  failure exits `1`.
- Output is streamed. Runtime failure or cancellation may leave partial output
  in the target; atomic replacement is not a P0 promise.
- Explicit regular-file inputs are opened before the output sink; all syntax,
  safety, and same-file checks happen before either can mutate the target. The
  exact order and append behavior follow the text stream contract.

## Pipeline exit status

P0 distinguishes a result state, such as `grep` finding no match, from a fatal
operational failure.

```text
syntax, safety, or request validation failure -> 2
fatal filesystem, access, or decoding failure -> 1
user cancellation                              -> 130
otherwise                                      -> final stage's exit code
```

Thus `grep NOTHING app.log` exits `1`, while `grep NOTHING app.log | head -n 5`
may exit `0` because the final `head` stage completed successfully. A fatal
upstream failure always dominates a later stage's success. Normal downstream
short-circuit is not fatal, suppresses its synthetic broken-pipe artifacts, and
does not require unread suffix data to be decoded. Exact result, cancellation,
fatal, and diagnostic ordering follows the text stream contract.

## Implemented read-only and redirection vertical slice (2026-08-10)

`which NAME` is also owned by the common catalog and runner when Familiar input
is reliable. It searches the runner's current filesystem directory first and
then its inherited `PATH` snapshot, applies a sanitized and deduplicated
`PATHEXT` snapshot (or the documented default), skips duplicate search
directories case-insensitively, and emits the first normalized absolute
non-directory match. It does not invoke a shell or report shell aliases,
functions, built-ins, or Wingman compatibility commands. No match is result
status `1` without a diagnostic; invalid names are rejected before execution.

`clear` is a standalone validated terminal operation. The runner emits only its
fixed clear-screen and cursor-home sequence; arguments, pipelines, and
redirection are rejected, and prepared control text cannot inject terminal
escape characters.

`ls` and its exact long-form alias `ll` are generated-record sources in the
same ordered text engine. Directory children or one explicit file are collected
before output mutation, bounded to 262,144 entries and 64 MiB of filename text,
and sorted with Windows Unicode ordinal ignore-case order plus an ordinal
case-sensitive tiebreaker. `-a` follows Windows Hidden/System attributes;
`-l` emits the pinned `TYPE ATTRS SIZE MODIFIED NAME` shape; and `-h` performs
integer half-up IEC size formatting only with `-l`. Explicit non-recursive
paths follow normal Windows access rules, while discovered reparse entries are
reported as type `l`. Generated records can feed every supported ordered text
stage and the existing reparse-safe final redirection sink.

`find` is a second generated-record source. It evaluates the explicit start at
depth zero, walks depth-first pre-order with the same Windows ordinal child
ordering as `ls`, includes hidden entries, and never descends into a reparse
entry or a reparse start. The dedicated bounded Unicode glob matcher applies
`-name`/`-iname` to the complete basename, while `-type`, `-mindepth`, and
`-maxdepth` are evaluated by the traversal. Traversal is capped at 100,000
visited entries and depth 256; cancellation and resource failure stop without
continuing into new filesystem objects. Relative display paths stay relative,
use native separators, and preserve the special `.`/`.\child` shape. Find
records feed the same ordered stages and safe final redirection; an empty
successful search remains status `0`.

Non-recursive text stages execute strictly from left to right in their declared
plan order. Supported stages may be repeated and recombined, including repeated
`grep`, `sort`, `uniq`, and finite `tail`, plus `head`/`tail` output feeding
later filters or materializing stages. A downstream `head` still short-circuits
unrequested upstream input, while fatal source failure continues to dominate
the final stage status. One ordered stage engine owns these semantics; the
runner no longer flattens a plan into command-specific ordering flags.

Recursive `grep` enumerates one sorted directory frame at a time and opens each
discovered file only when its turn is reached. It does not prebuild a complete
file list, so downstream `head` stops before later subdirectories are inspected.
Explicit root directory handles are opened before redirection. The output is
then opened without mutation, checked against an existing target inside any
root, and committed before traversal output begins. A multiply-linked target
receives an identity-only preflight so a true input alias is rejected without
truncating an unrelated multiply-linked output. A newly created target inside a
root is excluded from traversal by its pinned file identity.

The production sidecar now executes validated `clear`, `which`, `ls`/`ll`, `find`, `cat`, `head`, finite `tail -n N`, single-file `tail -f`, `wc -l`, `grep`, `sort`, and `uniq` plans through a
writer-based streaming entry point, either to normal stdout or to a final `>` or
`>>` file sink. It opens every explicit input before the output, resolves and
opens redirected output through the pinned-parent/reparse-safe primitive,
checks file identity before overwrite truncation, decodes each file with the
shared bounded UTF-8 reader, preserves multi-file concatenation and BOM rules,
supports continuous `cat -n` numbering, and keeps only the final sink's one
pending record.

`wc -l` consumes the same bounded record stream without materializing it and
counts only frames whose input terminator was present. A final unterminated
record is therefore not counted. It supports exactly one file or one supported
pipeline input and emits one generated terminated count record.

Finite `tail` retains only the last requested records. It does not preallocate
from `N`; the retained ring is capped at 65,536 records and 16 MiB of record
text. Exceeding either bound emits no tail data and exits `1`. `tail -n 0`
opens its explicit input but does not decode the payload. `tail -f` and
`--follow` accept exactly one file, retain the same bounded initial suffix, then
poll for appended bytes at a bounded interval. Complete records are flushed as
they arrive; an unterminated suffix remains in the shared UTF-8 decoder until a
later LF completes it. Cancellation discards that suffix and exits `130`.
Observed truncation is an operational failure, and file rotation is not tracked.

`uniq` keeps one bounded adjacent group in memory and compares complete lines
case-sensitively. It supports `-c`, `-d`, and `-u`, preserves the final member's
termination state, and composes with downstream `head`, finite `tail`, `wc -l`,
and safe redirection. Recursive `grep -r` now feeds this same ordered stage.

`sort` validates and materializes its complete logical input before emitting.
It is capped at 262,144 records and 64 MiB of record text, uses Unicode ordinal
ordering by default, and implements `-n` with exact decimal sign/coefficient/
scale comparison rather than floating point. Numeric ties remain stable under
`-r`; `-u` removes only text-identical records. Decode, numeric-data, and bound
failures emit no sorted stdout. Recursive `grep -r` now feeds this same ordered
stage, including repeated downstream filters and materializing stages.

`head` stops the upstream reader after the required prefix. An invalid UTF-8
suffix already buffered by the OS but not requested by the record reader is not
decoded and does not fail the command. Runtime failure in one `cat` source keeps
completed output, records exit `1`, and continues later independent files.
Redirected output uses the same BOM-free UTF-8/CRLF encoder. Append introduces
no BOM or separator, diagnostics remain on stderr, and a runtime failure or
cancellation can leave an empty or partial target as specified above.

This slice is reachable through typed runner requests and the actual
`wingman-runner` process. `clear`, `which`, `ls`/`ll`, `find`, `cat`, `head`, finite `tail -n N`, single-file `tail -f`, `wc -l`, `grep`, `sort`, `uniq`, `mkdir`, `touch`, `cp`, `mv`, and `rm` are now classified and published when
Familiar is on and the production PowerShell editor cycle is Reliable at a
FileSystem location. The shared lexer, parser, and catalog either build one
typed plan or prepare a deterministic exit-`2` rejection; explicit `.exe`
names, native-first pipelines, Familiar off, and uncertain input remain native
pass-through. A real PowerShell/ConPTY test covers Familiar activation, OOB
readiness, Unicode paths, `cat | head >`, `wc -l >`, `tail -n 1 >`, Unicode `grep -n >`, `uniq -c >`, `sort -n >`, the request broker, the sidecar, and
the next readiness cycle. `cmd` remains native pass-through because it has no
proved editor-readiness adapter.

## Cancellation

Ctrl+C sends cancellation to the runner. It stops recursive traversal,
streaming, waits, and `tail -f`; closes its output handles; and exits `130`.
Already completed copy, move, or remove work is not rolled back. Redirected
output may remain partially written.

The production sidecar installs a Windows console control handler before
execution and maps both `CTRL_C_EVENT` and `CTRL_BREAK_EVENT` to one shared
cancellation token. Read-only execution checks that token before input open,
between record reads, around sink emission, and before diagnostics. Accepted
cancellation wins over a simultaneous operational I/O failure, discards the
sink's uncommitted pending record, preserves completed stdout, and exits `130`
without a cancellation diagnostic. A process-level test launches the real
sidecar with `CREATE_NEW_PROCESS_GROUP`, waits for streaming to begin, sends a
group-scoped `CTRL_BREAK_EVENT`, and verifies partial output plus exit `130`.

Mutating requests use the preflight, staging cleanup, commit boundary, and
post-commit cancellation rules in the
[mutation execution contract](MUTATION_EXECUTION_CONTRACT.md).

## Runner boundary

The runner directly implements only validated P0 operations. It does not
reconstruct shell source, reinvoke `cmd` or PowerShell to implement a command,
reparse Bash syntax, or execute native commands as intermediate pipeline
stages.
