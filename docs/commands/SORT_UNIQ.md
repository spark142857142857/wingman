# `sort` and `uniq` Command Contract (P0)

Status: accepted MVP scope.

Korean version: [SORT_UNIQ.ko.md](SORT_UNIQ.ko.md)

Implementation status (2026-08-09): `sort` and `uniq` are published in the
Reliable PowerShell Familiar path.

Every file operand follows the shared
[Windows path contract](../WINDOWS_PATH_CONTRACT.md).
Text decoding, record termination, bounded materialization, and output follow
the shared [text record and stream contract](../TEXT_STREAM_MODEL.md).

## `sort`

```text
sort [OPTIONS] FILE
sort [OPTIONS] <pipeline input>
```

| Option | Meaning |
| --- | --- |
| `-r`, `--reverse` | Reverse the final sort order. |
| `-n`, `--numeric-sort` | Sort simple decimal numbers numerically. |
| `-u`, `--unique` | Keep one occurrence of each identical line. |

- Exactly one file or one pipeline input is accepted.
- Materialization is capped at 262,144 records and 64 MiB of record text.
  Exceeding either bound exits `1` without sorted stdout.
- Default comparison is case-sensitive Unicode ordinal comparison of the entire
  text line, independent of the active shell's locale defaults.
- With `-n`, ASCII space and tab are trimmed only for parsing. A blank trimmed
  line has numeric value zero; every other line must match
  `[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)`. Exponents, `NaN`, infinity, commas,
  Unicode digits, and embedded whitespace are invalid runtime data.
- Numeric comparison uses exact sign/coefficient/scale arithmetic, never a
  floating-point parse. Leading/trailing zeroes do not change value and every
  spelling of zero compares equal. Equal numeric values retain input order,
  including under `-r`. Invalid numeric data exits `1` with no partial sorted
  stdout.
- `-u` compares complete lines case-sensitively.
- `-n -u` still removes only text-identical lines; numerically equal spellings
  such as `1`, `1.0`, and `+01` remain separate stable records.
- Keys, field separators, human and version numeric modes, case folding, locale
  controls, output files, and multi-file input are out of scope.

## `uniq`

```text
uniq [OPTIONS] FILE
uniq [OPTIONS] <pipeline input>
```

| Option | Meaning |
| --- | --- |
| `-c`, `--count` | Prefix each emitted group with its count as `COUNT LINE`. |
| `-d`, `--repeated` | Emit only groups with two or more adjacent occurrences. |
| `-u`, `--unique` | Emit only groups with one occurrence. |

- Exactly one file or one pipeline input is accepted.
- Default `uniq` keeps only the first line in each group of adjacent identical
  lines. It does not remove non-adjacent duplicates.
- Comparisons are case-sensitive and apply to the whole line.
- `-c` may be combined with either `-d` or `-u`; `-d` and `-u` together are an
  error.
- Case-insensitive matching, skipped fields or characters, all-repeated output,
  output-file arguments, and other options are out of scope.

## Shared rules

- Wildcard paths and file-path plus pipeline-input combinations are rejected.
- File, access, decode, NUL, data, or materialization failures exit `1`; invalid
  syntax or input-source shape exits `2`; normal completion exits `0`.

## Required examples

```text
sort names.txt
grep ERROR app.log | sort
sort -r names.txt
sort -n numbers.txt
find src -type f | sort -u
sort names.txt | uniq
sort names.txt | uniq -c
sort names.txt | uniq -d
sort names.txt | uniq -u
```
