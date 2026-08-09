# Text Record and Stream Contract (Draft)

Status: accepted design direction for closing consolidated finding C4. This
document does not authorize implementation.

Korean version: [TEXT_STREAM_MODEL.ko.md](TEXT_STREAM_MODEL.ko.md)

## Scope and authority

This is the single P0 authority for file decoding, logical records, internal
pipeline transport, final newline behavior, output encoding, redirection sink
order, backpressure, downstream short-circuit, fatal status propagation, and
partial output.

Every P0 text producer and consumer uses this model. There is no command-specific
raw-byte bypass, PowerShell object stream, native shell pipe, or second line
reader.

## Logical record model

```text
RecordFrame {
  text: UnicodeString,
  terminated: bool
}

TextStream = ordered RecordFrame sequence
```

`text` never contains the recognized line-ending bytes. At a file decoder,
`terminated` initially records whether that source record ended with LF or
CRLF. Within a logical `TextStream`, it means that a logical line boundary
follows this record.

The stream invariant is:

- only the final record may have `terminated: false`;
- when a producer or transform discovers a later output record, it promotes a
  pending preceding `false` record to `true` before emission;
- an empty stream has no termination state.

One-record lookahead is sufficient. This keeps downstream `wc -l` and the final
encoder consistent after filtering or multi-source output while still
preserving whether the actual final emitted record had a terminator.

## UTF-8 decoder

Every explicit text file uses one streaming UTF-8 decoder.

- Valid UTF-8 split across arbitrary read boundaries is buffered until a whole
  Unicode scalar value is available.
- Invalid, overlong, surrogate, out-of-range, or incomplete-at-EOF UTF-8 is an
  operational input failure with exit `1`.
- No replacement-character repair is performed for P0 file input. The decoder
  reports the source and byte offset without copying raw source bytes into the
  diagnostic.
- A NUL byte is valid UTF-8 but outside Wingman P0 text semantics; encountering
  it is an operational text-input failure with exit `1`.
- UTF-16, UTF-32, ANSI, OEM, and locale-code-page files are outside P0. Their
  bytes are not guessed or transcoded.
- Other Unicode control characters remain text data. Their terminal effects
  are constrained by the terminal security model rather than silently removed.

An early downstream stop means unread bytes are not decoded or validated. A
malformed sequence observed before the stop remains fatal; malformed data in an
unread suffix has no outcome because P0 did not inspect it.

## BOM policy

At byte offset zero of each explicit file, one UTF-8 BOM (`EF BB BF`) is accepted
as an encoding signature and removed before record framing. It is not a text
record and is never counted, matched, sorted, or emitted.

- U+FEFF or the same bytes anywhere else are ordinary text.
- Each file in a multi-file command gets its own offset-zero BOM check.
- Internal pipelines carry Unicode records and have no BOM concept.
- Terminal and redirected P0 output never add a BOM.
- `>>` never inserts a second BOM into an existing file.

## Newline framing

P0 recognizes LF and CRLF as line endings. A CR immediately before LF belongs
to that CRLF ending and is removed from `text`. A lone CR is ordinary text.

```text
input bytes       records
empty             []
a                 [{ text: "a", terminated: false }]
a\n               [{ text: "a", terminated: true }]
a\r\nb            [{ "a", true }, { "b", false }]
\n                [{ text: "", terminated: true }]
a\n\n             [{ "a", true }, { "", true }]
```

A trailing terminator does not create a phantom record after it. Blank lines
are real empty records created by their own terminator. Record numbering counts
every emitted record, including an unterminated final record. `wc -l` is the
exception: it counts input frames whose `terminated` flag is true.

## File and generated sources

`cat FILE...` decodes each file independently for UTF-8 and BOM validation but
concatenates their decoded character streams before newline framing. Therefore,
an unterminated suffix of one file joins the prefix of the next file exactly as
text concatenation requires. `cat -n` numbers the resulting records continuously.

Commands that inspect files independently, such as multi-file or recursive
`grep`, do not merge a record across file boundaries. Prefixes such as
`PATH:LINE:` modify `text`. If a later selected result follows an unterminated
record from an earlier file, the earlier pending result is promoted to
terminated so both remain distinct logical records.

Generated record sources such as `ls`, `find`, `which`, Familiar control
responses, and `wc -l` mark every generated output record as
terminated. Diagnostics remain stderr and never enter a P0 stdout pipeline.

## Transform rules

- Streaming selection or mapping commands (`cat -n`, `grep`, `head`, non-follow
  `tail`) preserve a selected input frame's `terminated` value unless a later
  output frame requires promotion under the stream invariant.
- `head -n 0` emits an empty stream and requests normal upstream stop without
  reading payload records.
- `wc -l` counts only terminated input frames and emits one generated terminated
  count record.
- `sort` materializes bounded input. After reordering, it marks every output
  record except the logical final one as terminated and assigns the input
  stream's final-record termination state to that final output record.
- `sort -u` applies the same final-state rule after deduplication.
- `uniq` preserves order. An emitted group uses the termination state of the
  last input frame in that group; if later groups are filtered out, that state
  determines the final output newline.
- A command producing no records emits an empty stream regardless of the input
  stream's final termination state.

Commands may buffer only what their contract requires. Streaming commands do
not materialize the complete input merely to determine final newline behavior;
one pending output record is sufficient.

## Final encoding and sinks

The internal pipeline carries invariant-valid Unicode `RecordFrame` values,
not UTF-8 chunks. Only the final stdout sink encodes records.

- Normal P0 stdout bytes are UTF-8 without BOM.
- Every frame with `terminated: true` receives CRLF; a final `false` frame does
  not. The sink rejects an impossible nonfinal `false` frame as an internal
  pipeline failure rather than inventing bytes.
- No input newline byte style is preserved; LF and CRLF both become CRLF at the
  final terminal or file sink.
- The encoder never emits an incomplete Unicode scalar during normal operation.
- A low-level write failure may leave a byte prefix, including a partial UTF-8
  sequence, in a redirected file. P0 reports failure but cannot promise rollback.
- stderr diagnostics use bounded UTF-8/CRLF output but are not `TextStream`
  records and never follow stdout redirection.

Terminal rendering then travels through the normal Windows pseudoconsole path.
P0 does not double-encode text through a locale code page.

## Redirection preparation and open order

The runner completes these steps before any pipeline task emits data:

1. validate the complete plan, command grammar, path shapes, safety rules, and
   redirection/input identity constraints;
2. resolve and attempt to open every explicit regular-file input needed at
   startup, left to right;
3. open the final stdout sink;
4. start stage tasks and record flow.

If any explicit input cannot be opened, all opened input handles are closed,
diagnostics retain operand order, no stage starts, and the redirection target is
untouched. If the output sink cannot be opened, no stage starts. Directory
traversal and data decoding can still fail after the sink is open.

For `>`, step 3 creates or truncates the target. For `>>`, it creates or seeks
to the existing end. Append does not inspect, transcode, or repair existing
bytes; the appended segment alone is guaranteed to follow this UTF-8/no-BOM
encoder. If the existing file lacks a final newline, Wingman inserts no hidden
separator before the first appended byte.

`head -n 0 FILE > out.txt` still opens `FILE` first and then creates or truncates
`out.txt`, even though no payload record is read. A later operational failure or
cancellation may leave an empty or partial target. Atomic output replacement is
not a P0 promise.

Multi-source commands consume sources left to right. After a runtime read or
decode fault, `cat` and non-recursive `grep` stop that source and continue later
independent operands; recursive `grep` continues later files in traversal order.
The first fault in operand/traversal order is primary, final status is `1`, and
already emitted stdout remains. Cancellation, fatal sink failure, or downstream
normal stop prevents new sources from starting.

## Bounded pipeline and backpressure

Every adjacent stage uses a bounded record channel. Capacity and byte ceilings
are fixed during implementation review and measured under the performance
contract.

- The initial P0 limit for one decoded record is 1 MiB of source UTF-8 bytes,
  excluding the recognized LF byte. Exceeding it is an operational input
  failure; it is not truncation. This ceiling may be lowered after release-build
  calibration, but increasing it requires a contract and memory-budget review.
- A full downstream channel pauses the producer rather than growing memory.
- The pending-record encoder, decoder fragments, individual record length,
  stage count, `tail` buffer, and `sort` materialization all have explicit
  bounds.
- Finite `tail` uses a non-preallocated ring capped at 65,536 records and
  16 MiB of retained record text. Bound failure clears the ring and emits no
  tail records.
- `sort` materialization is capped at 262,144 records and 64 MiB of retained
  record text. Bound failure clears the materialized input and emits no sorted
  records.
- Exceeding an input-data or materialization bound is an operational failure
  with exit `1`; it is never truncation or partial reinterpretation.
- Blocking reads, writes, traversal, waits, and channel operations observe the
  shared cancellation token.
- A streaming source cannot busy-poll merely because its downstream consumer
  is slow.

`sort` validates and materializes its bounded complete input before it emits
sorted records. Therefore a numeric-data or materialization failure from `sort`
does not emit partial sorted stdout, although an already opened `>` target may
be empty.

## Normal short-circuit

A stage such as `head` may complete after consuming only a prefix. It sends a
normal stop signal upstream and closes only its incoming flow after its required
records are accepted.

- Upstream observes normal stop and exits without a broken-pipe diagnostic.
- Normal stop is not cancellation and does not produce exit `130`.
- Data not read after acknowledged stop is outside the operation and is not
  decoded, traversed, or validated.
- An operational failure already observed before the stop acknowledgement
  remains fatal and dominates downstream success.
- Synthetic closed-channel errors caused solely by normal stop are suppressed;
  genuine source, decoder, or sink errors are not.

Thus `cat huge.log | head -n 1` can stop reading after the first complete record.
An invalid UTF-8 sequence before that record fails; one in an unread suffix does
not.

## Outcome and diagnostic priority

```text
PreExecution = ValidationFailure(exit 2) | Ready

StageOutcome =
    Success(exit 0)
  | Result(exit 1)          # for example grep: no match
  | StoppedNormally
  | OperationalFailure(exit 1, diagnostic)
  | Cancelled(exit 130)
```

The runner publishes one pipeline status in this order:

1. a pre-execution syntax, safety, request, or plan failure exits `2` and starts
   no stage;
2. a user cancellation accepted before terminal completion exits `130`;
3. any genuine source, decoder, stage, redirection-write, or filesystem
   operational failure exits `1` and dominates a later stage's success;
4. otherwise the final stage's result code wins; upstream `Result(1)` is not a
   fatal failure and does not override a successful final stage;
5. `StoppedNormally` created by downstream short-circuit has no failure status.

If multiple operational failures occur, the primary diagnostic is selected
deterministically by lowest pipeline stage index and then source-operand order.
Additional diagnostics, if retained, follow the same bounded stable order.
Shutdown artifacts do not replace the primary cause.

Consequently, `grep NOTHING file` exits `1`; `grep NOTHING file | head -n 5`
may exit `0`; and `cat missing | head -n 5` exits `1`.

## `tail -f` record behavior

Follow mode remains record-based.

- The initial snapshot and appended bytes use the same streaming UTF-8 decoder.
- A current unterminated suffix is buffered and not emitted until LF completes
  its record. Later appended text extends that same pending record.
- `Ctrl+C` does not flush a still-unterminated pending record.
- Rotation tracking and byte-fragment streaming remain outside P0.
- Decode, NUL, access, truncation behavior selected by the command contract, and
  resource failures are operational exit `1`; user cancellation is `130`.

Non-follow `tail` and other finite readers do emit a final unterminated record
at EOF. The follow-mode buffering rule avoids showing one logical line twice or
inventing a record boundary while the file is still open for append.

## Partial output contract

Streaming stages may have emitted complete records before a later operational
failure or cancellation. Those records remain visible or redirected. Wingman
does not print success after a fatal outcome and does not roll back terminal
output.

The record encoder never deliberately emits half a record, but operating-system
write failure can leave a byte prefix in a file. Diagnostics state that output
may be partial. Mutation commands follow their separate filesystem partial-work
rules; this contract covers stdout data only.

## Required validation matrix

Tests cover at least:

1. UTF-8 scalar values split at every byte boundary; invalid, overlong,
   surrogate, out-of-range, incomplete EOF, NUL, UTF-16 BOM, and ANSI input;
2. no BOM, one initial UTF-8 BOM, BOM split across reads, BOM-only file, and
   U+FEFF later in a file or at the start of each multi-file operand;
3. empty, LF, CRLF, mixed LF/CRLF, lone CR, blank lines, final terminated, and
   final unterminated input using the examples in this contract;
4. `cat` multi-file boundary joining, `cat -n`, `grep` selection/prefixes and
   multi-source unterminated promotion, `head`, finite `tail`, `wc -l`, `sort`,
   `sort -u`, and every `uniq` filter;
5. empty output and final-newline state through two- and three-stage pipelines;
6. `>`, `>>`, existing unterminated append target, BOM target, missing input,
   output-open failure, same-file rejection, disk/write failure, and partial
   output;
7. one-record channel capacity, slow consumer, long-record limit, bounded sort,
   cancellation while blocked, and absence of busy polling;
8. `head -n 0`, early `head`, invalid data before and after the stop boundary,
   suppressed broken pipe, and fatal-before-stop priority;
9. upstream result status versus fatal failure, final-stage status, multiple
   fatal ordering, sink failure, cancellation race, and exit `0/1/2/130`;
10. `tail -f` initial complete records, pending unterminated suffix, later
    completion, split UTF-8 append, slow append, and `Ctrl+C` without flush.

## Standards references

RFC 3629 defines valid UTF-8 byte sequences and requires decoders to protect
against invalid forms:
[RFC 3629](https://www.rfc-editor.org/info/rfc3629/).

The Unicode Consortium documents the optional UTF-8 BOM as an encoding
signature rather than a byte-order requirement:
[Unicode BOM FAQ](https://unicode.org/faq/utf_bom.html).

Microsoft documents that the Windows pseudoconsole channel uses UTF-8:
[Pseudoconsoles](https://learn.microsoft.com/en-us/windows/console/pseudoconsoles).
