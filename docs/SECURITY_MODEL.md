# Security and Trust Model (Draft)

Status: proposed security contract for the planned common interpreter. This
document does not authorize implementation.

Korean version: [SECURITY_MODEL.ko.md](SECURITY_MODEL.ko.md)

## Security objective

Wingman is a terminal, not a sandbox. A native command intentionally submitted
by the user may read, modify, or remove anything allowed by the Windows access
token of the Wingman process. Wingman's responsibility is to avoid silently
increasing that authority, changing the command's target, exposing terminal
data through its UI boundary, or adding an execution path an attacker can use.

> A compatibility command receives the same Windows authority as the active
> native shell, and never more.

## Protected assets

- the user's files, credentials, environment variables, clipboard, and command
  intent
- terminal input, output, scrollback, and history
- the integrity of the interpreter, execution plan, runner, and installed files
- the current shell's working directory, environment, and exit status
- the integrity and confidentiality of an outstanding runner request

## Trust boundaries

```text
local bundled UI / xterm
  -> narrow Tauri invoke and event boundary
trusted Rust host and common interpreter
  -> session-local authenticated broker
trusted wingman-runner
  -> Windows filesystem under inherited user token

native PowerShell/cmd and their child processes
  -> user-directed but not trusted to define Wingman P0 semantics

remote content, terminal output, pasted text, paths, patterns, environment,
and compatibility updates
  -> untrusted data
```

The WebView necessarily has permission to send terminal input; compromise of
that UI could therefore type arbitrary native commands. Wingman reduces this
risk by loading only its bundled application UI, enforcing a restrictive
Content Security Policy, and exposing no unnecessary native capabilities.
Rust validation cannot make arbitrary native pass-through commands safe without
ceasing to be a terminal.

This model covers malformed input, remote-content compromise, command-building
injection, cross-user broker access, stale or replayed requests, and accidental
privilege or target broadening. It does not claim to isolate the user from
malware already running as the same Windows user, a compromised administrator
account, or a compromised operating system.

## Permission and elevation contract

- PowerShell, `cmd`, and `wingman-runner` inherit Wingman's current Windows
  access token. Familiar mode never changes that token.
- Wingman never bypasses UAC, stores administrator credentials, invokes
  `runas` for a compatibility command, or silently restarts elevated.
- If Wingman itself was explicitly launched elevated, the entire terminal
  session is elevated. The UI must display a persistent, unambiguous elevated
  indicator rather than a one-time warning.
- A native command may trigger its own normal Windows elevation flow. Wingman
  neither suppresses nor imitates that flow.
- `-f`, recursion, or any other P0 option never bypasses Windows ACLs, sharing
  violations, antivirus controls, or locked-file behavior.
- Familiar OFF and native pass-through use the same active shell token as P0.

## WebView and Tauri boundary

Production builds load only bundled local frontend assets. Remote navigation,
remote scripts, arbitrary iframes, development servers, and runtime-downloaded
UI code are forbidden.

The release configuration must:

- replace a null CSP with the narrowest CSP compatible with the bundled UI;
- allow only explicitly registered Tauri commands and events;
- expose no generic process-spawn, arbitrary filesystem, or unrestricted URL
  opening API to frontend JavaScript;
- validate input type, length, bounds, active session ID, and current state in
  every Rust command, even when TypeScript already checked them;
- attach session IDs to asynchronous PTY events and discard stale-session data;
- restrict clickable terminal links to an explicit user action and an approved
  scheme set, and never treat terminal output as trusted HTML;
- disable terminal escape features that can write the clipboard, launch a
  process, navigate the WebView, or invoke privileged app behavior without a
  separate user gesture.

External-link opening is not a general shell capability. If retained, it uses
a dedicated scheme allowlist and the operating system's external browser. URL
handling and xterm link behavior require their own security tests.

## Input and command-construction contract

- Native pass-through preserves the user's submitted input. Wingman does not
  append hidden shell syntax or reinterpret an unreliable reconstructed line.
- Prompt evidence, editing reliability, uncertain-input fallback, paste, and
  shell transitions follow the [terminal submission and session
  contract](TERMINAL_SESSION_CONTRACT.md). The WebView cannot assert a reliable
  prompt or line.
- Claimed P0 input becomes typed values and a validated execution plan. Paths,
  patterns, and text are never concatenated into PowerShell or `cmd` source.
- The WebView receives only an authoritative pass-through line or an opaque
  one-shot request ID and display line. Plans and prepared diagnostics remain
  in Rust and move only through the session broker.
- The internal shell invocation contains only a fixed installed runner path,
  fixed transport fields, and an unpredictable request ID as defined in the
  [runner transport contract](RUNNER_TRANSPORT.md).
- The runner implements validated P0 operations directly. It never invokes a
  shell to implement them or constructs a command string from plan data.
- Unsupported or ambiguous claimed syntax fails closed with exit code `2`. It
  is not partly translated and then passed through.
- Parsing, normalization, and filesystem use must not authorize one path and
  later operate on another. Reparse-point behavior follows each command
  contract, including the non-following deletion rule for `rm`.
- All path forms, runner-side resolution, object-identity comparisons, roots,
  and reparse behavior follow the shared
  [Windows path contract](WINDOWS_PATH_CONTRACT.md).

## Broker and runner contract

- One broker exists per Wingman session and accepts only the current user and
  local login session. Remote named-pipe access is denied.
- A separate owner-only local named pipe carries bounded editor-readiness
  frames. Its worker owns only a bounded inbox and stop state; it never owns or
  locks the application session, interpreter, PTY writer, or request broker.
- The readiness nonce and pipe name are removed from the PowerShell process
  environment as the first integration action, so later native children do not
  inherit them. Windows PowerShell loads the user profile before the current
  `-Command` integration bootstrap; that profile is therefore inside the P0
  trust boundary. A stronger profile-isolation policy requires a separately
  reviewed startup design.
- Readiness queue overflow, authenticated malformed input, replay, timeout,
  disconnect, or worker failure fail closed to native input. A late readiness
  frame never upgrades an editor cycle after any input was already forwarded.
- Request IDs are unpredictable bearer capabilities. They are short-lived,
  one-shot, consumed atomically, and invalidated on shell restart or app exit.
- A request has a protocol version, strict schema, bounded serialized size,
  and validated enum, length, and range fields. Unknown fields or versions do
  not execute by accident.
- The runner validates again and rejects unknown, expired, repeated, malformed,
  or session-mismatched requests before filesystem access.
- The runner is selected by an application-controlled absolute installed path,
  never by a `PATH` search.
- Packaged binaries and executable support files are signed and protected by
  normal installation-directory ACLs. Writable temporary scripts are not a
  release execution mechanism.
- Broker messages and diagnostics never echo the complete serialized plan,
  request secret, or environment.

Named-pipe ACLs and request IDs prevent unintended cross-session use and
replay. They do not promise isolation from a hostile process already running
with equivalent same-user authority.

Application launch uses the protected same-binary handoff in the
[CLI launch contract](CLI_LAUNCH_CONTRACT.md). Only the allowlisted handoff
handle is inherited; internal GUI role rejects missing, stale, replayed, or
parent-mismatched messages. Launch never elevates or copies path/environment
values into shell source or the child command line.

## Destructive operations and user intent

Wingman does not add confirmation dialogs to every native command. That would
change normal terminal behavior without creating a reliable security boundary.
Native commands remain the user's responsibility and pass through unchanged.

Wingman-owned P0 commands rely on narrow syntax and documented semantics:

- unsupported options and wildcards are rejected;
- conversion never broadens an explicit target set;
- `rm` applies its drive-root, share-root, current-directory, ancestor, and
  reparse-point rules before destructive execution;
- diagnostics do not disguise permanent deletion as Recycle Bin behavior;
- completed filesystem changes are not represented as transactional or
  automatically reversible.

The global no-mutation safety boundary, staged replacement, deterministic
ordering, partial failure, and cancellation behavior are binding in the
[mutation execution contract](MUTATION_EXECUTION_CONTRACT.md).

## Terminal data, history, clipboard, and logs

- Terminal input and output are sensitive by default. Production diagnostic
  logs do not record raw commands, PTY output, environment variables, working
  paths, clipboard contents, or serialized execution plans.
- Wingman-owned scrollback and visible recall are in memory for the current
  session only. Wingman does not override the active shell's configured native
  history, which may persist and may contain an opaque runner invocation. Any
  future persistent Wingman history is a separate opt-in feature with a visible
  location, retention control, and deletion control.
- Automatic secret redaction is not sufficient protection because arbitrary
  terminal data cannot be classified reliably.
- Copy requires an explicit user action on selected terminal text. Single-line
  paste inserts without submitting. Paste containing a line break is held for
  one Send/Cancel confirmation, then sent as one native paste without per-line
  Wingman classification.
- Crash reports and telemetry, if ever added, are opt-in and exclude raw
  terminal data and request contents.

## Updates and compatibility definitions

- Application and runner updates are authenticated, signed, versioned, and
  rollback-capable. Invalid signatures and incompatible protocols fail closed.
- A remotely obtained compatibility definition is data, never executable
  script. It uses a bounded signed schema and cannot expand Tauri capabilities,
  spawn arbitrary programs, or override hard-coded safety rules.
- Windows, shell, Tauri, xterm, and dependency updates trigger verification
  under the [compatibility maintenance contract](COMPATIBILITY_MAINTENANCE.md).
- Security restrictions may tighten in a patch release, but the reason and
  affected behavior are documented.

## Resource limits and denial of service

Input lines, request messages, pipeline stages, path counts, recursion, buffered
sort input, scrollback, and diagnostic sizes have explicit implementation
limits. Structured record streaming, UTF-8 failure, newline framing,
backpressure, short-circuit, partial output, and cancellation follow the
[text record and stream contract](TEXT_STREAM_MODEL.md). Limit failures produce
a bounded diagnostic and deterministic exit code; they do not crash the host
or silently execute a partial alternative plan.

Exact limits are selected and tested during implementation review rather than
invented independently by the frontend.

## Required security verification

Before release, tests cover at least:

1. CSP and production-local asset loading, including blocked remote script and
   navigation attempts.
2. Tauri capability inventory and rejection of malformed, oversized, bounded,
   and stale-session invoke inputs.
3. Terminal escape sequences, link schemes, clipboard access, and untrusted PTY
   output rendering.
4. P0 quoting and metacharacters proving that paths and patterns never become
   shell source.
5. Named-pipe access from another user/session, request guessing, replay,
   expiry, cancellation, app exit, and protocol mismatch.
6. Non-elevated and explicitly elevated sessions, proving no hidden elevation
   and a persistent elevated indicator.
7. `rm`, reparse points, roots, current-directory ancestors, ACL denial, locked
   files, and path-change race cases.
8. Logs and crash reports proving raw terminal data and request secrets absent.
9. Signed packaging, absolute runner selection, update signature failure, and
   compatibility-definition schema rejection.

## Prototype gaps to remove before release

The current prototype is behavioral evidence, not the release security
baseline. Implementation review must explicitly replace or justify:

- the current null CSP;
- any broad Tauri capability, including URL opening, without a required and
  tested user-facing feature;
- the writable temporary PowerShell profile and permissive bootstrap path;
- frontend-owned compatibility parsing and shell command-string construction;
- unbounded bridge inputs, terminal data, or request storage;
- clickable-link and terminal escape behavior without an explicit threat test.

These are migration requirements, not authorization to modify production code
before the project implementation gate is approved.
