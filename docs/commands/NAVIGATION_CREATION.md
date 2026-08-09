# Navigation and Creation Contract (P0)

This contract defines Wingman's small, Windows-native compatibility surface for
listing locations and creating files or directories. It is not a promise to
emulate every GNU Coreutils option or the Unix permission model.

Every path operand and write-safety check follows the shared
[Windows path contract](../WINDOWS_PATH_CONTRACT.md).
The modifying commands also follow the shared
[mutation execution contract](../MUTATION_EXECUTION_CONTRACT.md).
Generated `ls`, `pwd`, and `which` records and their final encoding follow the
shared [text record and stream contract](../TEXT_STREAM_MODEL.md).

## `ls` and `ll`

```text
ls [-a] [-l] [-h] [PATH]
ll [PATH]
```

- `ll` is exactly the `ls -l` alias.
- Short options may be combined; `ls -lah` is valid.
- With no `PATH`, list the current directory. A directory path lists its immediate children; a file path prints that item itself.
- The default output is one raw basename per line, suitable for a supported
  pipeline. Output uses case-insensitive Unicode ordinal basename order with a
  case-sensitive ordinal tiebreaker.
- `-a` includes items carrying Windows `Hidden` or `System` attributes. A leading `.` has no special hidden-file meaning.
- `-l` emits exactly `TYPE ATTRS SIZE MODIFIED NAME`. `TYPE` is `d` for a
  directory, `-` for a regular file, `l` for a reparse point, or `?` for another
  item. `ATTRS` is the fixed five-character `RASHC` mask (ReadOnly, Archive,
  System, Hidden, Compressed), replacing each absent letter with `-`.
- `SIZE` is an unsigned decimal byte count for a regular file and `-` otherwise.
  `MODIFIED` is last-write time truncated to whole seconds and rendered as
  `YYYY-MM-DDTHH:MM:SS±HH:MM` in the active Windows time zone. `NAME` is the raw
  basename; spaces are not quoted. No Unix owner, group, inode, or `rwx` fields
  are implied.
- `-h` is valid only with `-l`. It keeps `0B` through `1023B`; larger regular
  sizes use one decimal digit and `KiB`, `MiB`, `GiB`, `TiB`, `PiB`, or `EiB`,
  rounded half up with integer arithmetic. A value that rounds to `1024.0` is
  promoted to the next unit.
- Accepted relative, drive-absolute, and UNC paths may use either separator
  style where the shared path contract permits it.

P0 excludes recursion, wildcards, filters, sorting options, display-column customisation, and the rest of GNU `ls`.

## `pwd`

```text
pwd
```

- Takes no arguments or options.
- Prints the current working directory as an absolute native Windows path, such as `C:\Users\user\ProjectAgent\wingman`.
- It does not convert to a POSIX path and does not implement GNU `pwd -L/-P`.

## `clear`

```text
clear
```

- Takes no arguments or options.
- Clears the active terminal view. Scrollback retention and low-level ANSI cursor semantics are implementation details, not compatibility guarantees.

## `mkdir`

```text
mkdir [-p|--parents] PATH...
```

- Takes one or more explicit, non-wildcard paths.
- Without `-p`, an already-existing target directory is an error.
- With `-p`, missing ancestor directories are created; already-existing directories are successful no-ops.
- An existing file where a directory is needed is always an error.
- It never creates a Unix permission mode. `-m`, pipeline input, and wildcard expansion are outside P0.
- Wingman validates every operand before creating any directory, then processes
  operands left to right. An ordinary runtime failure may leave already-created
  directories (including a partial `-p` ancestor chain); later independent
  operands are still attempted and the final status is `1`.

## `touch`

```text
touch FILE...
```

- Takes one or more explicit, non-wildcard file paths.
- A missing target becomes an empty regular file. An existing regular file has its `LastWriteTime` updated to the current time.
- Parent directories are not created automatically.
- Targeting a directory is an error in P0. Timestamp-selection options such as `-a`, `-m`, `-d`, and `-r` are excluded.
- Pipeline input and wildcard expansion are outside P0.
- One UTC timestamp is captured for the request and applied to every operand.
  After global preflight, operands run left to right. Ordinary operational
  failures do not undo prior updates or creations and do not prevent later
  independent operands from being attempted; the final status is `1`.

## `which`

```text
which NAME
```

- Accepts exactly one filename component. A separator, drive colon, wildcard,
  control character, or invalid Windows filename makes the request syntax `2`.
- The runner snapshots `PATH` and `PATHEXT`. Search directories are the current
  filesystem directory followed by `PATH` components left to right; an empty
  component means the current directory, a relative component resolves from it,
  and one matching pair of enclosing quotes is removed. Percent-variable text
  is not expanded. Duplicate resolved directories are skipped case-insensitively.
- `PATHEXT` is split on `;`, normalized to leading-dot extensions, filtered to
  valid filename extensions, and de-duplicated case-insensitively in order. If
  it is absent or has no valid entry, use `.COM;.EXE;.BAT;.CMD`.
- If `NAME` has an extension, search that exact name only and require the
  extension to occur in `PATHEXT`. Otherwise append each `PATHEXT` entry in its
  listed order within each search directory. A match must be an existing
  non-directory file.
- Prints the first match as a normalized absolute Windows path. Inaccessible or
  invalid search directories are skipped; if no match exists, exit with result
  `1`, while a filesystem inspection failure that prevents a conclusive search
  is operational `1` with a diagnostic.
- It does not resolve Wingman compatibility commands, PowerShell aliases or functions, or shell built-ins such as `cd`.

## Errors and composition

`ls` output may feed a supported text pipeline. `pwd`, `clear`, `mkdir`, `touch`, and `which` do not accept pipeline input in P0.

- Success: exit code `0`.
- Missing path, inaccessible path, or filesystem failure: exit code `1`.
- Invalid syntax, unsupported option, wildcard path, or another rejected P0 shape: exit code `2`.

Each command keeps native Windows filesystem and ACL behavior. Read-only attributes, locks, and permissions are enforced by Windows; Wingman does not emulate Unix ownership or mode semantics.
