# `rm` Command Contract (P0)

Status: accepted MVP scope.

Korean version: [RM.ko.md](RM.ko.md)

Every target, root, ancestry, identity, and reparse check follows the shared
[Windows path contract](../WINDOWS_PATH_CONTRACT.md).
Whole-request preflight, traversal order, partial deletion, and cancellation
follow the shared [mutation execution contract](../MUTATION_EXECUTION_CONTRACT.md).

## Supported syntax

```text
rm [OPTIONS] PATH...
```

| Option | Meaning |
| --- | --- |
| `-r`, `-R`, `--recursive` | Remove a directory and its contents recursively. |
| `-f`, `--force` | Ignore missing paths and attempt removal of read-only or hidden items. |
| `--` | Stop option parsing. |

Short options may be combined. `rm FILE` removes files only; a directory
requires `-r`, `-R`, or `--recursive`. Pipeline input is never accepted.

Removal is permanent and does not use the Windows Recycle Bin. `-f` does not
bypass Windows ACLs or remove a file held open by another process.

## Required safety rules

Every target must be an explicit, non-wildcard path. `*` and `?` are rejected,
as are `--no-preserve-root` and every other unsupported option.

For a recursive request, Wingman must reject any target that resolves to:

- a Windows drive root, such as `C:\` or `C:/`
- a UNC share root, such as `\\server\share\`
- the current working directory
- an ancestor of the current working directory

The same protection applies after path normalisation, so `.` and `..` cannot
bypass it. When a symbolic link, junction, or other reparse point is an
explicit target, Wingman removes the link itself and never recursively follows
its target.

Every supplied target and every recursive tree must pass global preflight before
the first deletion. One known safety violation exits `2` and deletes nothing;
if required identity, ancestry, or reparse safety cannot be established, the
request exits `1` and deletes nothing.

## Exit rules

Recursive removal preflight is bounded to 100,000 entries and a maximum depth
of 256 below each target root. Exceeding either bound is a resource-safety
rejection with exit `2` and no deletion.

- Exit `0` only when all requested targets are successfully removed.
- Without `-f`, a missing path, access-denied error, or in-use file exits `1`.
- With `-f`, a missing path is successful; ACL and in-use-file errors still
  exit `1`.
- Invalid syntax, no target, unsupported options, wildcard paths, and safety
  rule violations exit `2`.
- After successful global preflight, top-level targets run left to right and a
  recursive tree is removed child before parent in the shared deterministic
  order. An ordinary runtime failure may leave partial deletion and Wingman may
  continue with later independent targets while the safety state remains valid.
  A safety-recheck mismatch stops further deletion. Any such failure exits `1`.
- Cancellation exits `130` and does not roll back completed deletion.

## Explicitly out of scope

```text
rm -i file.txt
rm -I dir
rm -d empty-dir
rm -v file.txt
rm --one-file-system dir
rm --no-preserve-root /
rm *.log
find . | rm
```

## Required examples

```text
rm notes.txt
rm -r temp
rm -rf dist cache
rm -- -file.txt
```
