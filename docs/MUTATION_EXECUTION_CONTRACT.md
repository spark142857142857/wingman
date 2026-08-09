# Mutation and Multi-Operand Execution Contract (Draft)

Status: accepted design direction for closing consolidated finding C5. This
document does not authorize implementation.

Korean version: [MUTATION_EXECUTION_CONTRACT.ko.md](MUTATION_EXECUTION_CONTRACT.ko.md)

## Scope and authority

This contract defines global preflight, operand order, safety failure,
operational continuation, staging, cancellation, diagnostics, and partial work
for P0 `mkdir`, `touch`, `cp`, `mv`, `rm`, and stdout redirection.

The Windows path contract remains authoritative for path forms, identity,
reparse points, roots, containment, and races. Command contracts define their
own valid operands and options. This document defines when validated operations
may begin and what remains after execution stops.

## Core distinction

```text
ValidationOrSafetyFailure -> exit 2, no mutation in the request
SafetyCannotBeEstablished -> exit 1, no mutation in the request
OperationalRuntimeFailure -> exit 1, documented partial work may remain
UserCancellation          -> exit 130, documented partial work may remain
Success                    -> exit 0
```

A request is the complete Wingman-owned submitted line, including every
operand and final redirection. Wingman never treats one unsafe operand as an
excuse to execute the others.

## Two-phase boundary

### Phase A: global preflight

Before the first mutation, the runner validates the whole request:

1. request/protocol/schema, command grammar, option combinations, operand
   count, and pipeline compatibility;
2. every `ValidatedPathSpec`, effective destination, redirection target, root,
   containment, and lexical alias rule;
3. existing-object identity, input/output alias, hard-link alias, ancestor,
   destination-inside-source, and reparse policy wherever required;
4. every recursive source or removal tree sufficiently to establish that no
   forbidden reparse item, root, current-directory ancestor, or unsafe boundary
   is present;
5. required source/destination types and startup file handles that are needed
   to make the operation safe.

A known grammar, shape, or safety violation exits `2` and mutates nothing. If
identity, reparse, ancestry, or recursive safety cannot be inspected because of
access, sharing, offline storage, race, or another operational condition, the
request exits `1` and mutates nothing. A changing filesystem is never permission
to continue with a guessed target.

Preflight may observe ordinary operational facts such as a missing target.
Their command-specific rules are recorded for Phase B, but they do not weaken
global safety. For example, `rm -f missing safe-file` may skip the missing
operand; an unsafe second operand still prevents every deletion.

### Phase B: ordered execution

After successful global preflight, operations execute in documented operand
order. Every identity and reparse assumption is rechecked immediately before a
commit, descent, delete, create, or open that can mutate.

- A runtime safety mismatch aborts remaining work with operational exit `1`.
- An ordinary per-operand operational failure is recorded and later independent
  operands continue when the command rules below permit it.
- Completed work is not represented as transactional or automatically rolled
  back.
- No new operand begins after cancellation.

## Stable order and diagnostics

Top-level operands run left to right as written. Recursive traversal uses
case-insensitive Unicode ordinal name order with a case-sensitive ordinal
tiebreaker, and removal visits children before their parent.

Diagnostics are emitted or reported in that same stable operand/traversal
order. The first operational failure is the primary diagnostic; additional
failures are bounded and retain order. A later cleanup or cancellation artifact
does not replace the primary cause.

Syntax/safety exit `2` may report all bounded preflight violations, but no
Phase B output claiming completed mutation is printed because execution never
started.

## `mkdir` multi-operand behavior

- Operands run left to right.
- Without `-p`, an existing directory is an operational failure `1` for that
  operand; later operands continue.
- With `-p`, existing directories are successful no-ops. Missing components are
  created from parent to child.
- If creation of a component fails, components already created for that operand
  remain. Wingman stops that operand and continues with the next top-level
  operand.
- An existing file where a directory is required, access denial, name collision,
  lock, or race is operational `1`.
- Reparse uncertainty discovered before Phase B prevents every mutation;
  reparse/identity change during Phase B stops all remaining work with `1`.

## `touch` multi-operand behavior

- The runner captures one UTC operation timestamp after preflight. Every
  successful operand uses that same timestamp.
- Operands run left to right. A missing leaf is created as an empty regular
  file; an existing regular file receives the captured `LastWriteTime`.
- An existing file's contents are never truncated or rewritten.
- Failure of one operand records `1` and later independent operands continue.
- A newly created file remains if setting its timestamp fails; that operand is
  still a failure.
- Directories, missing parents, reparse paths, access denial, locks, and races
  follow the shared path and operational rules.

## `cp` staging and commit

`cp` has one source and one effective destination. After preflight:

1. apply `-n` before staging; an existing destination is a successful no-op;
2. create an unpredictable staging sibling inside the verified destination
   parent, never in a global temporary directory;
3. copy and validate the complete file or recursive directory tree into staging;
4. flush and close required staging handles;
5. recheck source, parent, destination, identity, containment, and reparse
   assumptions;
6. commit staging to the effective destination with the narrowest available
   same-directory Windows rename/replace operation.

An uncommitted staging failure leaves the existing destination untouched.
Wingman removes staging best-effort; a cleanup failure is reported and may
leave a clearly application-owned staging item, but it is never treated as the
successful destination.

P0 recursive copy never merges directory trees. A destination directory that
would be replaced or merged remains a rejection/operational conflict under the
command contract. `-f` may clear replaceable read-only/hidden attributes on the
destination but never bypasses ACL, sharing, encryption, quota, or volume rules.

Once commit succeeds, `cp` is complete. A later diagnostic cleanup failure does
not remove the committed destination.

## `mv` commit and cross-volume partial state

For a same-volume move, Wingman uses a direct Windows rename/replace after final
identity and reparse rechecks. Success commits the destination and removes the
source as one filesystem operation where Windows provides that guarantee.

For a cross-volume move:

1. perform the same staged copy and destination commit as `cp`;
2. only after a successful destination commit, remove the source using the
   validated non-following removal rules;
3. if source removal fails or cancellation arrives after commit, exit `1` or
   `130` with both source and destination potentially present.

Wingman does not delete the committed destination to simulate rollback. Doing
so could lose the only complete copy after a concurrent source change. The
diagnostic states whether destination commit was confirmed and source removal
was incomplete.

`-n` skips before any copy or move. A failure before destination commit leaves
the source and old destination unchanged, apart from a possible reported
staging-cleanup artifact.

## `rm` all-target safety and partial deletion

All targets and recursive trees must pass global safety inspection before the
first deletion. One root, cwd/ancestor, forbidden path, reparse traversal,
unknown identity, or uninspectable safety boundary prevents every target from
being deleted.

After preflight:

- top-level targets run left to right;
- recursive targets use deterministic child-before-parent traversal;
- an explicit leaf reparse point deletes only the link object;
- `-f` changes only missing-target and replaceable attribute behavior; it does
  not suppress ACL, sharing, lock, race, or I/O failures;
- missing without `-f` records operational `1` and later safe targets continue;
- missing with `-f` is a successful no-op;
- a runtime failure within one target leaves already removed entries removed,
  stops that target where safety requires, and continues to a later independent
  target only if the failure did not invalidate global identity/reparse state;
- a runtime safety mismatch stops all remaining deletion.

Deletion is permanent, not Recycle Bin behavior. There is no undo log and no
rollback promise.

## Redirection mutation

Redirection follows the text stream open order and the path contract:

- all grammar, safety, identity, and explicit input-open checks complete first;
- `>` then creates/truncates and `>>` creates/appends before stage output starts;
- output-open failure starts no stage;
- later decoding, traversal, stage, sink, or cancellation failure may leave an
  empty or partial target;
- output aliasing any input identity is exit `2` with no target mutation;
- redirection is never staged as an atomic final replacement in P0.

## Cancellation

Cancellation is observed at traversal, copy, write, wait, and commit boundaries.

- No new top-level operand starts after accepted cancellation.
- `mkdir`, `touch`, and `rm` keep completed mutations.
- An uncommitted `cp`/cross-volume `mv` staging tree is cleaned best-effort; an
  already committed destination remains.
- Same-volume move is either observed before or after its filesystem commit; it
  is not split by Wingman.
- Exit is `130` when cancellation is accepted before terminal completion, even
  if shutdown produces secondary closed-handle errors.

## Exit aggregation

```text
global syntax or known safety violation       -> 2, no mutation
global safety inspection unavailable          -> 1, no mutation
accepted cancellation before completion       -> 130, partial work possible
one or more Phase B operational failures       -> 1, partial work possible
all operands successful or documented no-op   -> 0
```

For multi-operand commands, success requires every operand to succeed or be a
documented no-op such as `rm -f` missing, `mkdir -p` existing directory, or
`cp -n` skip. An operational failure is never hidden by a later success.

## Required validation matrix

Tests cover at least:

1. a safe operand plus a later syntax, wildcard, root, cwd/ancestor, same-file,
   destination-inside-source, or forbidden-reparse operand proving no mutation;
2. identity/reparse inspection unavailable before execution proving exit `1`
   and no mutation;
3. left-to-right `mkdir`, `touch`, and `rm` with first/middle/last operational
   failure, stable diagnostics, continuation, and final status;
4. `mkdir -p` partial component creation and collision; one captured `touch`
   timestamp; new-file timestamp failure and unchanged existing contents;
5. file and recursive `cp` staging success, copy failure, flush failure, cleanup
   failure, destination race, `-n`, `-f`, existing destination, and no merge;
6. same-volume atomic move where available; cross-volume copy failure,
   destination commit, source-delete failure, cancellation before/after commit,
   and both-copies diagnostic;
7. `rm` preflight over every target/tree, deterministic traversal, missing with
   and without `-f`, explicit reparse leaf, mid-tree ACL/lock/race failure,
   continuation versus global safety stop, and permanent partial deletion;
8. redirection missing input before target open, target-open failure, empty
   target after later failure, partial write, same-identity rejection, and
   cancellation;
9. cancellation at every stage boundary and exact `0/1/2/130` aggregation;
10. staging names, cleanup artifacts, and diagnostics containing no request
    secret or user-data copy beyond the necessary operand name.
