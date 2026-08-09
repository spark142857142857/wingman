# `find` Command Contract (P0)

Status: accepted MVP scope.

Korean version: [FIND.ko.md](FIND.ko.md)

The start operand follows the shared
[Windows path contract](../WINDOWS_PATH_CONTRACT.md). `-name` and `-iname`
values are command patterns, not paths.
Generated path records and their final encoding follow the shared
[text record and stream contract](../TEXT_STREAM_MODEL.md).

## Supported syntax

```text
find PATH
  [-type f|d]
  [-name PATTERN | -iname PATTERN]
  [-mindepth N]
  [-maxdepth N]
  [-print]
```

- `PATH` is one required start path; Wingman does not silently substitute `.`.
- `-type f` selects regular files and `-type d` selects directories.
- `-name` is case-sensitive and `-iname` is case-insensitive.
- `-mindepth` and `-maxdepth` require non-negative integers.
- `-print` is accepted as an explicit form of the default output action.
- Supported predicates may appear in any order, but each predicate may occur at
  most once. `-name` and `-iname` are mutually exclusive.

`PATTERN` matches only the discovered basename, not the whole path. It is a
Unicode-scalar wildcard grammar: `*` matches zero or more scalars, `?` matches
one, and bracket classes use the same member, range, negation, and escaping
rules as the P0 `grep` contract. `\` may escape `*`, `?`, `[`, `]`, `\`, `-`,
or `^`; any other or trailing escape is invalid. A path separator or `:` in the
pattern is invalid. An empty pattern is valid. `-iname` uses locale-independent
Unicode simple case folding without multi-scalar expansion.

## Path and output rules

- `.` and relative paths are supported. Relative paths may use `/` or `\` as
  separators, for example `./src` or `.\src`.
- Drive-absolute and UNC paths must satisfy the shared path contract. Linux or
  root-relative input such as `/home/user/project` is rejected, not translated.
- Each result is written on its own line in a lexically normalized native path
  form. Separators become `\`; a relative start remains relative; `.` is shown
  as `.` and its descendants as `.\name`; and a drive-absolute or UNC start
  remains absolute. No `\\?\` internal namespace is displayed.
- The start path is evaluated at depth `0`. Therefore `-maxdepth 0` can return
  the start path, while `-mindepth 1` excludes it.
- Hidden files and directories are included. A reparse entry may match when no
  `-type` is present, but it is neither a P0 regular file nor directory for
  `-type f|d` and is never traversed.
- Results are deterministic depth-first pre-order: evaluate the start, then at
  each directory visit children in case-insensitive Unicode ordinal basename
  order with a case-sensitive ordinal tiebreaker. Depth limits prevent descent
  but do not change sibling order.

## Pipeline and exit rules

`find` is a source command: it can feed a P0 pipeline but cannot receive
pipeline input.

```text
find src -type f -name "*.ts" | wc -l
find . -type f | grep "test"
```

A completed search exits with status `0`, even when it finds no results. A
missing or inaccessible start path exits with status `1`. Invalid syntax,
unsupported predicates, and invalid depth values exit with status `2`.

## Explicitly out of scope

```text
find . -size +10M
find . -mtime -7
find . -path "*/node_modules/*"
find . -regex ".*\\.ts"
find . -o -name "*.tsx"
find . -exec rm {} \;
find . -delete
```

P0 does not support additional metadata predicates, boolean expressions,
regular-expression or whole-path tests, or any action with side effects.

## Required examples

```text
find . -type f -name "*.ts"
find src -iname "*test*" -type f
find . -mindepth 1 -maxdepth 2 -type d
find "C:\work\project" -type f
find src -type f -name "*.ts" | wc -l
```
