# Windows Path and Filesystem Contract (Draft)

Status: accepted P0 design direction. This document closes consolidated-review
item C2. It does not authorize implementation.

Korean version: [WINDOWS_PATH_CONTRACT.ko.md](WINDOWS_PATH_CONTRACT.ko.md)

## Scope and principle

This contract is the single path authority for P0 command operands, final
stdout redirection, CLI starting directories, validation, execution, and tests.
Individual command contracts may further restrict a valid path, but may not
accept a path form rejected here or weaken these safety rules.

> Parse the user's path once, resolve it against the runner's real filesystem
> location once, and use Windows object identity for safety when strings are
> not enough.

Wingman is not a filesystem sandbox. An accepted path can reach anything the
current Windows access token may access. These rules prevent ambiguity and
accidental target broadening; they do not isolate the user from a hostile
same-user process racing filesystem changes.

## Path values are not patterns

`PathValue` and `PatternValue` are different validated types.

- Every file, directory, source, destination, and redirection operand is a
  `PathValue` and never performs wildcard expansion.
- `*` and `?` are rejected in a `PathValue`.
- A command-specific pattern such as `find -name "*.ts"` is a `PatternValue`.
  Its grammar belongs to that command and it is never resolved as a path.
- Quotes group a lexer word and are removed before path validation. They are
  not part of the filename.

## Accepted user path forms

| Kind | Examples | Rule |
| --- | --- | --- |
| Relative | `file.txt`, `src\main.ts`, `./src`, `.\src`, `..\logs` | Resolved from the runner's inherited filesystem cwd. Both separator styles are accepted. |
| Drive-absolute | `C:\work\file.txt`, `C:/work/file.txt` | Drive letter plus separator is required. Drive letters are case-insensitive. |
| UNC-absolute | `\\server\share`, `\\server\share\folder` | The prefix must use two backslashes and include non-empty server and share components. Later separators may use either style. |

`.` and `..` components are accepted and resolved lexically. A drive root or
UNC share root is a valid read-only operand; destructive command contracts add
their own root protection.

`~`, `$HOME`, and `%USERPROFILE%` have no expansion meaning. If otherwise valid,
they are literal filename components.

## Rejected user path forms

| Form | Examples | Reason |
| --- | --- | --- |
| Empty | `""` | There is no target. |
| Drive-relative | `C:`, `C:temp\file.txt` | Meaning depends on a per-drive shell directory and is not stable across the runner boundary. |
| Root-relative | `\Windows`, `/home/user` | Meaning depends on the current drive and can be confused with a Linux absolute path. |
| Slash-prefixed UNC | `//server/share` | P0 requires explicit Windows UNC spelling with `\\`. |
| Device or NT namespace | `\\?\...`, `\\.\...`, `\??\...`, `\\?\GLOBALROOT...`, volume-GUID paths | These bypass normal Win32 path interpretation and complicate root and device safety. |
| Alternate data stream | `file.txt:secret` | P0 operates on ordinary files and directories only. The drive-letter colon is the sole accepted colon. |
| Wildcard path | `*.log`, `src\?.ts` | P0 does not expand path operands. |
| Invalid/control characters | NUL, U+0001-U+001F, `<`, `>`, `"`, `|` | Not an ordinary Win32 filename and may collide with terminal grammar. |
| Ambiguous component | `name.`, `name ` | Win32 normalization can address a different name than the displayed input. |
| Reserved device component | `CON`, `NUL.txt`, `COM1`, `LPT9` | Win32 may interpret it as a device rather than a file. |

Reserved device checks are case-insensitive and apply to the component before
its first dot. P0 rejects `CON`, `PRN`, `AUX`, `NUL`, `CONIN$`, `CONOUT$`,
`COM1`-`COM9`, and `LPT1`-`LPT9` in any component.

The resolved absolute path is limited to 4096 UTF-16 code units in P0. A longer
value is rejected as unsupported syntax. Wingman may use an internal long-path
Win32 representation after validation, but a user-supplied `\\?\` prefix is
never accepted or shown.

## Classification and resolution

Path processing is ordered and performed by the same common Rust library at two
trust boundaries:

```text
host preparation:
lexer word
  -> classify Relative | DriveAbsolute | UncAbsolute, or reject
  -> validate characters, components, reserved names, wildcards, and length
  -> ValidatedPathSpec stored inside ExecutionPlan

runner execution:
  -> defensively validate ValidatedPathSpec again
  -> obtain runner filesystem cwd and location kind
  -> convert accepted `/` separators to `\`
  -> prepend cwd for Relative
  -> fold repeated separators, `.`, and `..` lexically
  -> reject traversal above a drive root or UNC share root
  -> ResolvedPath with absolute native spelling
  -> operation-specific filesystem and identity checks
```

The host never guesses the shell's current directory and never stores an
absolute path derived from the host process. `ExecutionPlan` contains validated
path syntax only. The runner creates `ResolvedPath` after it has inherited the
active shell's actual environment and cwd. Resolved paths and file identities
are never returned to the WebView.

Runner lexical resolution does not open the filesystem and does not dereference a
symbolic link, junction, mount point, or other reparse point. It therefore also
works for a missing destination leaf. Filesystem inspection is a separate step.

Unicode normalization is not performed. The original user spelling is retained
for history and diagnostics; the resolved spelling is used for execution.
Forward slashes are only a user-input convenience and output uses native
backslashes unless an individual command contract says otherwise.

## PowerShell and cmd location

- In `cmd`, a relative path uses the runner process's inherited current
  directory. Drive-relative operands remain rejected even if cmd tracks a
  directory for that drive.
- In PowerShell, the transport shim must prove the current location belongs to
  the FileSystem provider before invoking the runner.
- A non-filesystem provider location never falls back to an older process cwd.
  A P0 filesystem request fails with the documented location error.
- The path string itself is never interpolated into the shell command used to
  invoke the runner.

## Comparison and object identity

String comparison alone is not a sufficient safety decision on Windows.

- Lexical safety comparison is ordinal and case-insensitive, with separators
  normalized. It may conservatively reject aliases on a case-sensitive
  directory.
- For an existing object, operations that must detect the same source and
  destination use volume/file identity obtained from an opened handle.
- Two different hard-link names with the same identity are the same file for
  `cp`, `mv`, and redirection alias checks.
- `rm` of one ordinary hard-link path removes that named link only.
- When an output leaf does not exist, Wingman validates the existing parent and
  rechecks the created/opened leaf before writing.

## Reparse-point policy

Reparse points include symbolic links, junctions, mount points, and other
Windows link-like objects.

| Operation class | P0 policy |
| --- | --- |
| Explicit non-recursive read (`cat`, `head`, file `grep`, `ls PATH`) | May follow the explicitly supplied path under normal Windows access rules. Wingman does not claim sandbox confinement. |
| Recursive read (`find`, `grep -r`) | Never descends into a reparse point, including when one is encountered below the start path. An explicit reparse start is not traversed. |
| `cp`/`mv` | Rejects a reparse source, a reparse item discovered in a recursive source, and any reparse ancestor or existing destination involved in the operation. |
| `mkdir`, `touch`, and output redirection | Rejects an existing reparse target or reparse ancestor before writing. |
| `rm` | Requires non-reparse ancestors. If the explicit leaf is a reparse point, removes the link itself and never its target. Recursive traversal never follows reparse points. |
| CLI starting directory | May follow an explicitly supplied existing directory path; it performs no mutation. |

If Wingman cannot determine whether a write or destructive path crosses a
reparse point, it fails without performing that operation.

## Root, ancestry, and containment

Operation-specific checks use the resolved path plus object identity where
available.

- Recursive `rm` rejects drive roots, UNC share roots, the current filesystem
  directory, and every ancestor of that directory.
- A recursive `cp` destination and an `mv` destination may not be the source,
  inside the source, or the same object through another spelling.
- A redirection target may not identify any file opened as an input source for
  the same execution plan.
- UNC server-only paths are never valid. A UNC share root is the containment
  root and `..` cannot escape it.
- Mount-point and reparse aliases do not weaken these checks.

All operand shapes and determinable safety conditions are validated before the
first mutation. Recursive operations recheck identities and reparse state while
walking. If the filesystem changes between checks and safe identity cannot be
maintained, Wingman aborts with an operational failure rather than following a
new target.

The exact whole-request preflight boundary, deterministic traversal, staging
and commit points, partial results, cancellation, and exit aggregation follow
the [mutation execution contract](MUTATION_EXECUTION_CONTRACT.md).

## Errors

- Invalid or unsupported path shape, wildcard path, reserved name, length
  overflow, traversal above root, or a known safety-rule violation exits `2`.
- A valid path that is missing, inaccessible, locked, offline, unavailable, or
  rejected by the actual filesystem exits `1`, subject to command-specific
  result rules such as `rm -f` on a missing leaf.
- Failure to inspect identity or reparse state for a write/destructive operation
  is a safe operational failure (`1`), not permission to continue.
- Diagnostics name the operand and rule without printing secrets, internal
  namespace rewrites, or broker data.

## Required validation matrix

Tests cover at least:

```text
accepted:
  file.txt
  .\src\main.ts
  ../src/main.ts
  C:\work\한글 파일.txt
  C:/work/project
  \\server\share\folder

rejected as shape/safety exit 2:
  C:relative.txt
  \root-relative.txt
  /home/user/file
  //server/share/file
  \\?\C:\file.txt
  \\.\PhysicalDrive0
  file.txt:stream
  *.log
  folder\name.
  folder\NUL.txt
```

The suite also creates same-file hard links, file and directory symbolic links,
junctions, a destination-inside-source case, drive/UNC roots where safely
available, Korean and case-variant names, missing leaves, locked files, denied
access, redirection aliasing, and a controlled path-change race. Destructive
fixtures remain inside a verified disposable test root.

## Implementation status note (2026-08-09)

The `runner_io` foundation now opens each output ancestor relative to the
already verified parent directory handle, with reparse processing disabled, and
opens the leaf relative to the pinned final parent. A controlled test replaces
the pathname with a junction after the parent handle is pinned and verifies that
the alternate target is never followed. Existing reparse leaves and ancestors
are rejected before truncation, and a missing leaf below a reparse ancestor is
not created.

The production sidecar now connects this primitive to the validated `cat`,
`head`, finite `tail -n N`, `wc -l`, `grep`, and `uniq` record stream for `>` and `>>`. Integration tests cover input-before-
output ordering, overwrite and append, hard-link same-file rejection, reparse
rejection, output-open failure, partial runtime output, and cancellation during
an actual redirected runner process. A production PowerShell/ConPTY vertical
test now also exercises Unicode-path `cat | head >`, `wc -l >`, `tail -n 1 >`, Unicode `grep -n >`, and `uniq -c >` submissions through this
primitive. Familiar-off, uncertain, explicit-native, and `cmd` input remain
outside this interception path.

## Research basis

Windows distinguishes fully qualified, root-relative, drive-relative, UNC, and
device namespace paths; this contract intentionally supports only the bounded
subset above. See [Microsoft: Naming Files, Paths, and Namespaces](https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file).
