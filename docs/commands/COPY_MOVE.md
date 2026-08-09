# `cp` and `mv` Command Contract (P0)

Status: accepted MVP scope.

Korean version: [COPY_MOVE.ko.md](COPY_MOVE.ko.md)

Every source, destination, identity, containment, and reparse check follows the
shared [Windows path contract](../WINDOWS_PATH_CONTRACT.md).
Global preflight, staging, commit, partial-result, and cancellation behavior
follows the shared [mutation execution contract](../MUTATION_EXECUTION_CONTRACT.md).

## Supported syntax

```text
cp [OPTIONS] SOURCE DESTINATION
mv [OPTIONS] SOURCE DESTINATION
```

| Command | Supported options |
| --- | --- |
| `cp` | `-r`, `-R`, `--recursive`, `-f`, `--force`, `-n`, `--no-clobber`, `--` |
| `mv` | `-f`, `--force`, `-n`, `--no-clobber`, `--` |

Each operation accepts exactly one source and one destination. Pipeline input and
wildcard paths containing `*` or `?` are not supported. A directory source for
`cp` requires a recursive option; `mv` moves files or directories without a
recursive option. `-f` and `-n` cannot be combined.

## Destination rules

- A new destination path becomes the new file or directory name.
- When the destination is an existing directory, Wingman uses
  `DESTINATION\basename(SOURCE)` as the effective destination.
- If that effective destination already exists as a directory, Wingman rejects
  the operation instead of merging directory trees.
- Missing parent directories are an error. Use `mkdir -p` first.

## Overwrite and platform rules

- By default, an existing destination file is overwritten only at the commit
  point after a sibling staging item has been copied, flushed, and rechecked.
- `-n` skips an existing destination before staging and succeeds without
  changing it.
- `-f` attempts to replace a read-only or hidden destination item.
- `-f` does not bypass Windows ACLs, locked files, encryption, or volume
  constraints.
- A same-volume `mv` commits with a direct rename or replace. A cross-volume
  `mv` stages and commits a copy, then removes the source. If cancellation or a
  source-removal failure happens after commit, both source and destination may
  remain; Wingman does not delete the committed destination as rollback.

Before the first filesystem change, Wingman validates the complete source and
effective destination. Any known safety rejection exits `2` without mutation;
inability to establish required identity or traversal safety exits `1` without
mutation. A pre-commit copy failure leaves an old destination untouched and
removes staging data on a best-effort basis.

## Required rejection rules

Wingman must reject the following before executing either command:

- source and effective destination resolve to the same path
- a recursive copy destination resolves inside the source directory
- a move destination resolves inside the source directory
- wildcard paths, pipeline input, or more than two operands
- combined `-f` and `-n`
- a source or recursive traversal containing a symbolic link, junction, or
  other reparse point

## Exit rules

- Exit `0` after a successful copy or move.
- `-n` exits `0` when it skips an existing destination.
- Missing source, access denied, destination conflict, or a locked file exits
  `1`.
- Invalid syntax, unsupported options, or a rejection-rule violation exits `2`.
- Cancellation exits `130`; documented post-commit state may remain.

## Required examples

```text
cp app.json backup.json
cp -r src backup
cp -n config.json backup\config.json
mv old-name.txt new-name.txt
mv build dist
```
