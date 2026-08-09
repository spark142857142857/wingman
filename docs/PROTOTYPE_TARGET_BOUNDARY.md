# Prototype and Target Boundary

Status: binding documentation and migration boundary. This document does not
authorize implementation.

Korean version: [PROTOTYPE_TARGET_BOUNDARY.ko.md](PROTOTYPE_TARGET_BOUNDARY.ko.md)

## Why this boundary exists

The checked-in application is a prototype. The accumulated P0 contracts describe
the intended common-interpreter target. They coexist during planning and migration,
but a prototype behavior is not automatically a target promise and a target
contract is not a claim that the current code already implements it.

## Source-of-truth order

For the future common-interpreter release, conflicts are resolved in this order:

1. the [implementation gate](IMPLEMENTATION_GATE.md) and accepted consolidated
   review;
2. shared target contracts for path, terminal session, text stream, mutation,
   runner transport/execution, security, performance, and CLI launch;
3. P0 command contracts under `docs/commands/`;
4. the common-interpreter acceptance test plan;
5. README and release/support material after controlled cutover.

The current `README.md`, `docs/TEST_MATRIX.md`, `docs/MANUAL_SMOKE_TEST.md`, and
existing product tests are prototype evidence until cutover. They do not override
the target contracts when they mention Windows 10, P1 commands such as `sed` or
`xargs`, input redirection, shell-specific mappings, or other behavior outside P0.

## Before implementation approval

- Product code and behavior-changing tests remain untouched.
- Planning may label and cross-link prototype and target documents.
- The final consolidated review must resolve C1-C10, align English/Korean copies,
  and receive explicit user approval under the implementation gate.
- Performance values remain proposed; no documentation edit may pretend that an
  unmeasured prototype or debug build passed them.

## Migration test separation

After explicit implementation approval, add a new `contract-v1` suite beside the
legacy prototype suite. Do not rewrite legacy expectations in place to make new
behavior appear green.

- Legacy tests answer: “Did migration unexpectedly break the old prototype before
  the planned cutover?”
- Contract-v1 tests answer: “Does the target P0 contract pass through the common
  Rust core, runner, both shell transports, and application boundary?”
- Expected intentional differences are recorded in a migration ledger with the
  target contract and cutover phase that owns them.

Legacy compatibility mappings and their tests are removed only after the target
suite passes and the controlled cutover is approved. Historical test evidence may
be archived; it is not relabeled as target acceptance.

## One-time performance calibration

During Phase 1 boundary spikes, measure an optimized release build on the reference
machine using the performance contract. One calibration proposal may adjust the
currently provisional targets or ceilings using recorded raw data and an explained
reason. The user reviews that proposal together with the consolidated implementation
plan. Once accepted, the values are frozen for P0 acceptance; later changes require
an explicit performance-contract decision, not a quiet test relaxation.

## Controlled cutover checklist

Cutover occurs only when all are true:

```text
[ ] contract-v1 automated, shell, application, security, and manual suites pass
[ ] accepted performance ceilings pass on the recorded reference environment
[ ] prototype-only mappings and writable temporary transport are removed
[ ] README and support matrix describe only observed target behavior
[ ] installer exposes the contracted wingman command and protected runner
[ ] English and Korean user documentation agree
[ ] user explicitly approves final acceptance
```

At cutover, README becomes the public target summary and links to the detailed
contracts. Until then its prototype banner must remain visible.
