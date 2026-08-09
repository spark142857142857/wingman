# `grep` Command Contract (P0)

Status: accepted MVP scope.

Korean version: [GREP.ko.md](GREP.ko.md)

Every file and directory operand follows the shared
[Windows path contract](../WINDOWS_PATH_CONTRACT.md). A grep pattern is a
separate value and is not treated as a path.
Text decoding, record termination, pipeline transport, and final output follow
the shared [text record and stream contract](../TEXT_STREAM_MODEL.md).

## Supported syntax

```text
grep [OPTIONS] PATTERN FILE...
grep [OPTIONS] PATTERN <pipeline input>
```

Supported short and long options are:

| Option | Meaning |
| --- | --- |
| `-i`, `--ignore-case` | Ignore case while matching content. |
| `-n`, `--line-number` | Prefix matching output with a line number. |
| `-v`, `--invert-match` | Output non-matching lines instead. |
| `-F`, `--fixed-strings` | Treat the pattern as literal text. |
| `-r`, `--recursive` | Search files below an explicitly supplied directory. |
| `--` | Stop option parsing. |

Short options may be combined, for example `grep -in TODO app.log`.

## Input and output rules

- File paths and pipeline input are mutually exclusive. At least one is required.
- `-r` requires one or more explicit directory paths and cannot consume pipeline input.
- Top-level operands run left to right. Recursive search is depth-first; at
  each directory, entries use case-insensitive Unicode ordinal basename order
  with a case-sensitive ordinal tiebreaker. Reparse points are neither followed
  nor searched as files.
- For a single non-recursive file, output contains the selected text lines. For
  multiple files or recursive search, output is prefixed with `PATH:`. With
  `-n`, the prefix is `PATH:LINE:` (or `LINE:` for pipeline input). Numbering is
  one-based and restarts for each file; pipeline input has one one-based count.
- A displayed `PATH` uses the lexically normalized native form of its operand:
  separators become `\`, a relative operand remains relative, `.` descendants
  begin `.\`, and an absolute operand remains absolute. Recursive descendants
  append their discovered basenames to that displayed root.
- A match exits with status `0`; no match exits with result status `1`.
  Inaccessible paths, read/decode failures, NUL, and resource limits are
  operational status `1`; invalid syntax, pattern, or input shape is `2`.
- Startup opens, runtime reads, and diagnostics use operand/traversal order. A
  runtime read/decode error stops that file but later independent files are
  still searched. Matches may therefore accompany an operational final `1`.

## Pattern rules

The default pattern supports only the portable P0 regular-expression subset:

```text
.  *  ^  $  []  \
```

`-F` disables regular-expression interpretation. It should be used for an
exact string such as `C:\temp\file.txt`.

Matching is over Unicode scalar values in one logical record; newline is never
part of the pattern input. The grammar is exact:

- an ordinary scalar matches itself and `.` matches one scalar;
- unescaped `^` is valid only as the first pattern token and unescaped `$` only
  as the last; they anchor the record start and end;
- `*` repeats the immediately preceding literal, `.`, or bracket class zero or
  more times; a leading, repeated, or anchor-following `*` is invalid;
- `[abc]`, `[a-z]`, and `[^a-z]` are supported. A class must contain at least one
  member. A range has two scalar endpoints in ascending ordinal order. `-` is
  literal only first or last, and `^` negates only when first;
- `\` escapes exactly `.`, `*`, `^`, `$`, `[`, `]`, `\`, or `-`. A trailing
  escape or an escape of another scalar is invalid. Inside a class, `]`, `-`,
  `^`, and `\` may be escaped;
- grouping, alternation, `+`, `?`, braces, backreferences, named classes, and
  locale collation are not grammar and produce pattern exit `2`.

`-i` applies locale-independent Unicode simple case folding to scalar
comparisons, without multi-scalar expansions. `-F` treats every pattern scalar
literally but uses the same `-i` rule when requested. An empty pattern is valid
and matches every record.

P0 does not support extended or Perl regular expressions, multiple patterns,
context output, colour, glob expansion, include/exclude filters, or binary-file
handling. In particular, the following must produce an unsupported-syntax error
rather than a partial conversion:

```text
grep -E "foo|bar" app.log
grep -P "(?<=id=)\d+" app.log
grep -e foo -e bar app.log
grep -C 3 ERROR app.log
grep TODO *.txt
grep --include="*.ts" -r TODO src
```

`find -name "*.ts"` is different: that wildcard is a `find` argument, not
shell glob expansion, and is covered by the `find` contract.

## Required examples

```text
grep TODO app.log
grep -in TODO app.log
grep -F "C:\temp\file.txt" app.log
cat app.log | grep -n ERROR
grep -r "TODO" src
```
