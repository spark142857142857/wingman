# Implementation Gate

Status: binding project decision.

No implementation, migration, or production-code modification of the common
interpreter begins merely because its planning documents are complete.

Before implementation starts, Wingman must:

1. Re-review the accumulated product contract, command contracts, architecture,
   data model, input classification, lexer contract, shared path/filesystem,
   terminal session, text stream, mutation execution, CLI launch, and
   prototype/target boundary contracts, security model, performance budget,
   maintenance plan, and test plan as one coherent proposal.
2. Identify any contradictions, missing command behavior, safety gaps, and
   migration risks in that review.
3. Present the consolidated implementation plan to the user.
4. Receive the user's explicit approval to begin implementation.

Until step 4, documentation and read-only research are allowed; implementation
code, compatibility refactors, and behavior-changing tests are not.

The current review result and required pre-implementation corrections are in
[the consolidated plan review](CONSOLIDATED_PLAN_REVIEW.md).
