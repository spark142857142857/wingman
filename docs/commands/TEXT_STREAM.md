# `cat`, `head`, `tail`, and `wc` Command Contract (P0)

Status: accepted MVP scope.

Korean version: [TEXT_STREAM.ko.md](TEXT_STREAM.ko.md)

Every file operand follows the shared
[Windows path contract](../WINDOWS_PATH_CONTRACT.md).
All decoding, BOM, newline, final-terminator, pipeline, and output behavior
follows the shared [text record and stream
contract](../TEXT_STREAM_MODEL.md).

## `cat`

```text
cat [-n | --number] FILE...
```

- One or more explicit text files are written in argument order.
- Files are decoded separately for UTF-8/BOM validation, then concatenated
  before record framing. An unterminated suffix therefore joins the next file's
  prefix.
- `-n` and `--number` number every output line, including blank lines, with a
  continuous count across files.
- `cat` is a source command and cannot receive pipeline input.
- Startup-open failures are collected in operand order before any redirected
  target is opened, and no stage starts. During streaming, a read/decode failure
  stops that source at the fault, records operational status `1`, and continues
  with later independent operands unless cancellation or downstream normal stop
  intervenes. Already emitted text is not rolled back.
- Interactive standard-input reading, binary byte semantics, and options such
  as `-A`, `-b`, and `-s` are out of scope.

## `head`

```text
head [-n N] FILE
head [-n N] <pipeline input>
```

- The default count is 10 lines. `N` must be a non-negative integer.
- Exactly one file or one pipeline input is accepted.
- `head -n 0` succeeds without output.
- Byte counts, headers, and the obsolete `-5` form are out of scope.

## `tail`

```text
tail [-n N] FILE
tail [-n N] <pipeline input>
tail [-n N] [-f | --follow] FILE
```

- The default count is 10 lines. `N` must be a non-negative integer.
- Finite mode retains at most 65,536 records and 16 MiB of record text. It
  exits `1` without tail output if either materialization bound is exceeded.
- `tail -n 0` opens the explicit input but does not decode its payload.
- `-f` and `--follow` require exactly one file. Wingman prints the current last
  N lines and then prints appended lines until the user interrupts with `Ctrl+C`.
- In follow mode, a current unterminated suffix remains pending until an
  appended LF completes it. `Ctrl+C` does not flush that pending fragment.
- If the open file is observed to shrink below the consumed offset, follow mode
  reports an operational failure instead of seeking or reopening it.
- File rotation tracking, `-F`, byte counts, reverse output, and `+N` syntax
  are out of scope.

## `wc`

```text
wc -l FILE
wc -l <pipeline input>
```

- P0 supports only `-l` and `--lines`.
- Exactly one file or one pipeline input is accepted.
- Output is the count of terminated input record frames only. A final non-empty
  line with no LF/CRLF is not counted, matching `wc -l` semantics.
- Bare `wc`, word, byte, character, maximum-line-length, and multi-file totals
  are out of scope.

## Shared rules

- Wildcard paths and file-path plus pipeline-input combinations are rejected.
- P0 guarantees text-line behavior only, not binary data or byte-exact encoding
  behavior.
- Invalid UTF-8, NUL, a record/resource limit, file-not-found, access, or other
  runtime input failure exits `1`; invalid syntax or input-source shape exits
  `2`; normal completion exits `0`; cancelled follow exits `130`.

## Required examples

```text
cat README.md
cat package.json tsconfig.json
cat -n app.log | grep ERROR
head -n 20 app.log
cat app.log | grep ERROR | head -n 10
tail -f server.log
grep ERROR app.log | tail -n 5
wc -l README.md
find src -type f -name "*.ts" | wc -l
```
