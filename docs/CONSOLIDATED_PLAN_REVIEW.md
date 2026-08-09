# Consolidated Plan Review

Status: completed top-to-bottom review with conditional approval of the design
direction. Implementation is **not** authorized by this document.

Korean version: [CONSOLIDATED_PLAN_REVIEW.ko.md](CONSOLIDATED_PLAN_REVIEW.ko.md)

## Outcome

The product direction is coherent and does not need a restart. Wingman's small
Unix-familiarity surface over native Windows shells, one Rust compatibility
core, structured execution plans, a dedicated runner, one-shot local transport,
and native pass-through boundary form a viable P0 architecture.

The review identified four contract blockers and six high-value corrections.
All ten are now closed at the documentation level. This still does not authorize
implementation: the user must re-review and explicitly approve the consolidated
plan, and the boundary spikes remain the first implementation phase.

The current prototype remains a useful migration baseline: its TypeScript,
PowerShell, cmd, layout, and Rust tests all passed on 2026-08-05. Those tests
prove the existing prototype is internally stable; they do not prove the new
common-interpreter contract.

## Decisions that remain accepted

- Product role: **Windows shell, Unix muscle memory**; not Linux, Bash, WSL, or
  a POSIX runtime.
- Target user: someone who already uses terminal commands and wants familiar
  Unix command habits in a native Windows environment.
- P0 scope: the documented bounded commands, whole-P0 text pipelines, and one
  final stdout redirection only.
- Native PowerShell, cmd, Windows administration, environment, and state
  commands remain available through raw pass-through.
- Familiar OFF is raw pass-through.
- One shared Rust classifier, lexer, validator, plan model, and command engine
  replace frontend cmd mappings and command-specific PowerShell functions.
- `wingman-runner.exe` directly executes validated P0 operations and does not
  implement them by constructing shell source.
- Tauri/WebView2 remains the initial UI, while the core remains renderer-neutral.
- Windows 11 24H2 or later x64, Windows PowerShell 5.1, and `cmd.exe` are the initial support
  matrix. PowerShell 7, Windows 10/Server, P1 commands, remote rule delivery,
  and a native-renderer rewrite remain outside P0.
- Security and correctness take priority over performance shortcuts.
- No implementation starts before the user re-reviews this corrected plan and
  gives explicit approval.

## Required contract corrections

| ID | Severity | Finding | Required resolution |
| --- | --- | --- | --- |
| C1 | Blocker | The data model says the frontend transports a serialized `RunnerRequestV1`; the later transport contract says Rust retains the plan and returns only a one-shot request ID. | Make the transport contract authoritative. Inside a session/command-sequence envelope, the frontend receives only `PassThrough { raw_line }` or `InvokePrepared { request_id, display_line }`. The broker alone provides `PreparedRequestV1 = Reject | Execute | Control` to the runner. |
| C2 | Blocker | There is no single Windows path contract. Drive-relative paths, root-relative paths, `/home`-looking input, UNC, device namespaces, alternate data streams, long paths, trailing dot/space behavior, case-only aliases, and reparse-point races can change targets or bypass root checks. | Add one shared path/filesystem contract used by every command, redirection, CLI path, validator, and test. P0 accepts ordinary relative paths, drive-absolute paths, and explicit UNC paths; rejects drive-relative and device/NT namespace forms, ADS, wildcards outside command patterns, and ambiguous names. Destructive safety uses handle/file-identity and non-following traversal rules, not string normalization alone. |
| C3 | Blocker | Reliable input capture and active-shell state are the architecture's most difficult boundary, but no dedicated contract defines supported editing, Unicode erasure, completion fallback, multiline paste, nested-shell transitions, or the state after an unknown escape/edit sequence. | Add a terminal submission/session contract. Interpret only a line reconstructed from known editing operations. Unknown edits and completion pass through. Support only documented standalone shell transitions; suspend interception when shell identity is unknown. The boundary spike must prove Unicode, wide characters, history recall, Ctrl+C, paste, and generated-invocation replacement in both shells before command migration. |
| C4 | Blocker | The runner contract permits `cat` to stream chunks directly but also promises UTF-8 decoding and CRLF serialization. It does not define BOM output, final unterminated lines, early `head` completion, upstream stop, or precise fatal-status propagation. | Define one text-record/stream model before commands. A streaming decoder handles split UTF-8 and an optional input BOM; newline state is explicit; no raw undecoded chunk bypasses it. Define short-circuit as normal upstream stop, fatal-error priority, redirection-open order, backpressure, and partial-output behavior. |
| C5 | High | Destructive and multi-target behavior is incomplete. It is unclear whether one unsafe `rm` target prevents all deletion, how partial `mkdir`/`touch`/`cp`/`mv` failures continue, and what happens when redirection aliases an input file. | Validate the entire syntax and every safety rule before the first mutation. Any safety violation exits `2` with no mutation. Runtime failures may leave documented partial work and exit `1`. Reject output redirection that resolves to the same file identity as an input. Keep `rm` last in implementation. |
| C6 | High | Several command contracts are not exact enough for one implementation and deterministic tests: `grep` regex/class rules and recursive traversal; `find` wildcard grammar and path presentation; `sort -n` number grammar and comparator; `ls -l/-h` columns and time/size formats; `which` PATH/PATHEXT resolution; and multi-source error ordering. | Close these semantics in the existing command documents. Prefer small deterministic rules over GNU breadth. Recursive result order may remain unspecified, but tests compare result sets and diagnostics/status deterministically. |
| C7 | High | The security document says history is session-memory-only and paste never executes by itself. Native PowerShell PSReadLine normally saves history incrementally, and the current prototype sends pasted line breaks as submissions. | Limit the promise to data owned by Wingman: Wingman adds no persistent command/output history in P0, while the native shell keeps its configured history behavior. Add one concise confirmation for a paste containing a line break before forwarding it. Raw P0 input remains Wingman's displayed recall entry; native shell history may contain the opaque internal invocation. |
| C8 | High | `wingman` CLI behavior is specified, but the process topology that lets a shell invocation return while a GUI window remains is not. Argument combination/order, GUI-subsystem behavior, error propagation, and same-binary relaunch are unresolved. | Make CLI/GUI handoff a boundary spike. Define the exact grammar and whether one binary self-spawns a detached internal GUI mode or a separate launcher is packaged. Preserve the public names `wingman.exe` and `wingman-runner.exe` unless the spike proves another signed internal binary is necessary. |
| C9 | High | The migration plan implements most commands before the full pipeline, redirection, and cancellation engine, although command semantics depend on those facilities. | Build the runner skeleton, streaming/pipeline engine, redirection, status propagation, resource limits, and cancellation with test stages before migrating real commands. Add read-only commands next, mutations later, and `rm` last. |
| C10 | Medium | Prototype and target documentation coexist. README still promises Windows 10, P1 commands, input redirection, and legacy mappings; the prototype test matrix intentionally tests behavior outside P0. Performance numbers are also uncalibrated. | Keep prototype tests as a migration baseline but replace their public promises at cutover. Add the new contract suites rather than mutating legacy expectations in place. Calibrate the performance budget once during the boundary-spike phase, then freeze it for P0 acceptance. |
| C11 | Blocker | The original plan assumed native `cmd` could provide the same trustworthy prompt and editor boundary as PowerShell. The 2026-08-08 spike proved that `PROMPT` cannot supply per-prompt sequence or nested depth, user prompt changes remove the marker, and no safe native buffer-replacement primitive was established. | Keep `cmd.exe` as a supported native terminal but make all P0 `cmd` input pass-through. Enable Familiar interception only through the packaged Windows PowerShell 5.1 PSReadLine adapter. Reconsider `cmd` Familiar only with a separately reviewed hook or Wingman-owned line editor. |

## Contract closure progress

- **C1 closed on 2026-08-06:** semantic ownership decisions are now separate
  from `FrontendDecisionV1`; Rust retains `PreparedRequestV1`, and the WebView
  receives only a session/sequence envelope plus raw pass-through or an opaque
  one-shot request ID and display line. Reject, Execute, and Control share the
  broker path.
- **C2 closed on 2026-08-06:** one shared Windows path contract now defines
  accepted forms, rejected namespaces, host `ValidatedPathSpec`, runner-side
  resolution, file identity, roots, hard links, and reparse policies.
- **C3 closed on 2026-08-06:** the terminal submission/session contract now
  requires validated prompt evidence, a conservative session state machine,
  Unicode-safe mirrored edits, permanent uncertainty after completion or an
  unknown edit, native foreground pass-through, and confirmed shell-stack
  transitions. Its boundary spike is mandatory before command migration.
- **C4 closed on 2026-08-06:** one `RecordFrame { text, terminated }` contract
  now owns streaming UTF-8/BOM decoding, LF/CRLF framing, final newline,
  command transforms, output encoding, redirection open order, bounded
  backpressure, normal short-circuit, fatal priority, `tail -f`, and partial
  output. Command-specific raw-byte bypass is forbidden.
- **C5 closed on 2026-08-06:** the mutation contract now separates whole-request
  no-mutation safety preflight from ordered operational work; fixes staging and
  commit for `cp`/`mv`, all-target preflight for `rm`, redirection identity,
  cancellation, partial state, diagnostics, and exit aggregation.
- **C6 closed on 2026-08-06:** command contracts now fix the P0 regex and glob
  grammars, Unicode folding, traversal and displayed paths, exact decimal sort,
  `ls -l/-h` fields and rounding, `which` resolution, and multi-source failure
  order. The acceptance plan names the corresponding deterministic fixtures.
- **C7 closed on 2026-08-06:** Wingman-owned recall is session-memory-only while
  native shell history keeps its configured behavior and may contain the opaque
  invocation. A line-breaking paste receives one Send/Cancel confirmation and
  then remains one native paste with no per-line Wingman classification.
- **C8 closed on 2026-08-06:** the public console launcher self-spawns the same
  signed `wingman.exe` in a protected internal GUI role and waits for a bounded
  two-way readiness handoff. The mandatory spike may reopen the contract before
  any separate internal GUI binary is introduced.
- **C9 closed on 2026-08-06:** migration now builds and tests the runner,
  `RecordFrame` pipeline, redirection, status priority, resource bounds, and
  cancellation before any real command; read-only commands precede mutations
  and `rm` remains last.
- **C10 closed on 2026-08-06:** README and legacy test documents are explicitly
  prototype snapshots, target authority is separate, contract-v1 tests are
  added beside legacy evidence after approval, cutover rules are fixed, and one
  Phase 1 performance calibration is frozen after user acceptance.

## Recommended minimal product resolutions

These defaults reduce P0 size without weakening its main value:

- Reserve only `familiar on`, `familiar off`, `familiar status`, and the `fam`
  short form. Drop the undocumented `compat` alias from the target contract.
- Keep clickable terminal links out of P0. URLs remain selectable/copyable text;
  this removes the current URL-opening capability and link-handler attack surface.
- Ship no remotely downloaded compatibility definitions in P0. Compatibility
  semantics change only through a signed Wingman release.
- Wingman itself stores no persistent command or output history in P0. It does
  not silently override the active shell's own history configuration.
- A paste containing a line break receives one compact send/cancel confirmation.
  Single-line paste remains immediate.
- Support in-session shell state only for reliably captured, documented
  standalone `cmd[.exe]`, `powershell[.exe]`, and matching `exit` transitions.
  Other ways of launching an interactive shell are outside P0 and require
  Familiar OFF or a fresh correctly selected session; Wingman does not claim it
  can detect every wrapper or alias.
- Treat the status-bar path as **last confirmed filesystem location**, never a
  guaranteed live provider path. A non-filesystem PowerShell location is shown
  as such and P0 filesystem commands fail through the provider guard.

## Corrected target flow

```text
xterm input
  -> reliable submission/session tracker
     -> uncertain or Familiar OFF: raw line + Enter to active shell
     -> reliable line: Rust prepare_submission(session_id, raw_line)
          -> PassThrough: raw line + Enter
          -> InvokePrepared: retain plan/diagnostic in Rust session memory
               -> replace the visible shell edit buffer with a fixed invocation
               -> shell invokes wingman-runner with one-shot request ID
               -> runner connects to session broker
               -> broker atomically consumes PreparedRequestV1
               -> runner revalidates and executes or prints rejection
               -> stdout/stderr/exit code return through the native shell PTY
```

The frontend never receives a path-bearing execution plan, parses P0 options,
or constructs a command-specific shell string. Shell-specific code is limited
to reliable input replacement, exact shell transitions, the PowerShell
filesystem-provider guard, and fixed runner transport.

## Corrected implementation sequence

Implementation may begin only after the required contract edits, a final
read-through, and explicit approval.

### Phase 0: contract closure

1. Align data model with request-ID transport.
2. Add shared Windows path/filesystem rules.
3. Add terminal submission/session and paste/history rules.
4. Complete text-stream/pipeline and command-detail semantics.
5. Align security, performance, CLI, README migration, and acceptance tests.

### Phase 1: boundary spikes

1. Package and launch the runner and resolve CLI/GUI process topology.
2. Prove named-pipe ACL, one-shot consumption, expiry, restart, and protocol
   mismatch behavior.
3. Prove cwd, environment, PATH, token, exit code, and non-filesystem provider
   behavior in both shells.
4. Prove reliable Unicode input replacement, raw displayed history, internal
   invocation echo behavior, paste policy, and Ctrl+C.
5. Record the first release-build startup, memory, input, and runner-dispatch
   performance baseline.

If a boundary spike fails, return to the contracts before implementing P0
commands.

### Phase 2: shared pure core

Implement and test path types, lexer, parser, classifier, command catalog,
typed plans, diagnostics, protocol validation, resource limits, and prepared
request storage without connecting destructive operations.

### Phase 3: runner engine

Implement broker client, defensive validation, text decoding, streaming stages,
pipeline short-circuit, fatal-status priority, stdout/stderr, redirection,
backpressure, cancellation, and deterministic test stages.

### Phase 4: read-only commands

Implement `pwd`, `which`, `ls`/`ll`, `clear`, `cat`, `head`, `tail`, `wc`,
`grep`, `find`, `sort`, and `uniq`, with exact contract and shell-transport tests.

### Phase 5: filesystem mutation

Implement `mkdir` and `touch`, then `cp` and `mv`. Connect `rm` only after path,
file-identity, reparse-point, root, ancestor, and race tests pass.

### Phase 6: controlled cutover

Run legacy and common-v1 comparison only behind an internal development flag.
After the P0 matrix passes, remove frontend cmd mappings, command-specific
PowerShell compatibility functions, writable temporary profiles, and the
temporary flag. Retire obsolete executable legacy tests without rewriting their
historical evidence as target acceptance. Update README and support claims at
the same cutover.

### Phase 7: release hardening

Apply CSP and final Tauri capability minimization, signed packaging, update and
uninstall checks, performance/endurance gates, Windows current/previous release
canaries, and final manual UI/PTY/security smoke tests.

## Acceptance gates

P0 is ready only when all are true:

```text
[x] C1-C11 contracts resolved and English/Korean documents aligned
[x] cmd spike completed; P0 narrowed to tested native pass-through
[ ] PowerShell 5.1 boundary matrix passes
[ ] pure, runner, filesystem, pipeline, transport, native-preservation suites pass
[ ] path, reparse, destructive, WebView, broker, paste, and elevation security tests pass
[ ] performance release ceilings pass on the reference machine
[ ] README, support matrix, installer, and observed behavior agree
[ ] no legacy compatibility parser or writable temporary profile remains
[ ] final consolidated review is presented
[ ] user explicitly approves implementation start and later P0 acceptance
```

## Evidence behind three corrections

- PowerShell's working location is not the same as the process current
  directory, which validates the non-filesystem provider guard and the need to
  test inherited cwd explicitly: [Microsoft about_Locations](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_locations).
- PSReadLine's default history style is `SaveIncrementally`, so Wingman cannot
  claim all shell history is memory-only without changing native behavior:
  [Microsoft Set-PSReadLineOption](https://learn.microsoft.com/en-us/powershell/module/PSReadline/set-psreadlineoption?view=powershell-5.1).
- Windows distinguishes fully qualified, root-relative, drive-relative, UNC,
  and device namespace paths, supporting one centralized path contract rather
  than per-command string checks: [Microsoft Naming Files, Paths, and Namespaces](https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file).

## Gate status

The top-to-bottom review, C1-C10 corrections, and final documentation consistency
pass completed on 2026-08-06. The pass checked 58 project Markdown files with no
broken local links, unmatched fences, or trailing whitespace and confirmed
English/Korean heading and fence parity for every target pair. Only the user's
consolidated-plan review remains. No production-code implementation,
compatibility refactor, boundary-spike code, or behavior-changing test work is
authorized until explicit approval.
