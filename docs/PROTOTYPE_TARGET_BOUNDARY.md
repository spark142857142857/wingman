# Prototype and Target Boundary

Status: historical migration boundary. The code cutover is complete; final
manual, external-matrix, signing, and user-acceptance gates remain open.

Korean version: [PROTOTYPE_TARGET_BOUNDARY.ko.md](PROTOTYPE_TARGET_BOUNDARY.ko.md)

## Why this boundary exists

The checked-in application is now a P0 release candidate. Historical prototype
behavior remains in explicitly labelled README and test snapshots, but it is
not a current target promise. The implemented P0 contracts and current release
matrix now govern the candidate.

## Source-of-truth order

For the current common-interpreter candidate, conflicts are resolved in this order:

1. the [implementation gate](IMPLEMENTATION_GATE.md) and accepted consolidated
   review;
2. shared target contracts for path, terminal session, text stream, mutation,
   runner transport/execution, security, performance, and CLI launch;
3. P0 command contracts under `docs/commands/`;
4. the common-interpreter acceptance test plan;
5. current README and release/support material.

The historical sections of `README.md`, `docs/TEST_MATRIX.md`, and
`docs/MANUAL_SMOKE_TEST.md` remain prototype evidence. They do not override the
target contracts when they mention Windows 10, P1 commands such as `sed` or
`xargs`, input redirection, shell-specific mappings, or other behavior outside P0.

## Historical pre-implementation gate

- Product code and behavior-changing tests remain untouched.
- Planning may label and cross-link prototype and target documents.
- The final consolidated review must resolve C1-C10, align English/Korean copies,
  and receive explicit user approval under the implementation gate.
- Performance values remain proposed; no documentation edit may pretend that an
  unmeasured prototype or debug build passed them.

## Migration test separation

After explicit implementation approval, the contract suites were added beside
the legacy prototype evidence. Legacy expectations were not relabelled to make
new behavior appear green.

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
[x] accepted performance ceilings pass on the recorded local reference environment
[x] prototype-only mappings and writable temporary transport are removed
[x] README and support matrix distinguish current behavior from historical evidence
[x] installer exposes the contracted wingman command and protected runner
[x] English and Korean current-contract documentation agree
[ ] user explicitly approves final acceptance
```

The code and documentation cutover is complete. The first item remains open only
for the irreducible manual UI checks and external variants listed in the release
matrix; final release acceptance has not been claimed.
