# Terminal Submission and Session Contract (Draft)

Status: accepted design direction for closing consolidated findings C3 and C7.
This document does not authorize implementation.

Korean version: [TERMINAL_SESSION_CONTRACT.ko.md](TERMINAL_SESSION_CONTRACT.ko.md)

## Scope and authority

This contract defines when Wingman may interpret a submitted line, how it
mirrors native line editing, how it falls back after uncertain input, and how
it tracks the active `cmd.exe` or Windows PowerShell 5.1 session. It is the
shared authority for terminal input, completion fallback, paste, visible
recall, prompt synchronization, nested-shell transitions, and interruption.

The shell remains the owner of its editor, native history, prompt, foreground
program, and process state. Wingman may intercept only at a validated shell
prompt with a reliably reconstructed single line.

## Non-negotiable invariant

> If Wingman cannot prove both the active prompt and the submitted line, it
> forwards input to the terminal session and does not interpret or replace it.

Terminal output is never scraped to guess a command line. A visually plausible
prompt is not evidence. Unknown editing does not produce a best-effort line.

## Session state

```text
TerminalSessionState =
    AwaitingPrompt { expected_root_shell }
  | Editing {
      shell, shell_depth, command_sequence,
      buffer, cursor, evidence
    }
  | Running { shell, shell_depth, command_sequence }
  | Suspended { reason }
  | Closed

LineEvidence = Reliable | Uncertain { reason }
```

- `AwaitingPrompt` is used at startup, after an interrupt, and while a shell
  transition is awaiting confirmation.
- `Editing/Reliable` is the only state from which Wingman may call the common
  interpreter.
- `Editing/Uncertain` keeps forwarding native input but cannot become reliable
  again during the same submission.
- `Running` covers a submitted native command, a Wingman runner, a continuation
  prompt, and a foreground interactive program.
- `Suspended` means shell identity or integration evidence is unavailable.
  Familiar may remain visibly enabled as a preference, but interception is
  inactive until a valid synchronization event or a new session.

While `Running`, `Suspended`, or `Closed`, every terminal input operation is
native pass-through except Wingman's separate window shortcuts. In particular,
input to `ssh`, a REPL, an editor, a pager, or another foreground program is
never classified as a Wingman command.

## Prompt synchronization evidence

The target architecture uses a minimal packaged shell-integration hook for
Windows PowerShell 5.1. The hook sends editor-readiness frames over a dedicated
session named pipe; readiness is never inferred from PTY output. `cmd.exe`
intentionally has no trusted editor hook in P0 and therefore remains native
pass-through. A valid PowerShell readiness frame carries:

```text
EditorReadiness {
  protocol_version,
  session_nonce,
  command_sequence,
  shell: WindowsPowerShell,
  shell_depth,
  location_kind: FileSystem | NonFileSystem,
  adapter_capability: PsReadLineReplaceV1
}
```

The readiness broker accepts fixed-size ASCII frames, authenticates the
session nonce, bounds its queue, and rejects duplicate or replayed sequences.
Only a current-session nonce and exact expected sequence may change session
state. The readiness worker never acquires application or terminal locks;
`handle_terminal_input` drains its inbox while holding the active session lock.
Ordinary PTY OSC/CSI data has no production readiness authority.

**Current cutover status (2026-08-09):** the OOB channel is connected to
production PowerShell sessions and has passed repeated ConPTY
PowerShell → readiness → Rust decision → request broker → real sidecar → next
readiness tests. PTY readiness parsing is explicitly disabled in production.
Familiar remains default-paused, but an explicit `familiar on` now activates
the proved `cat`/`head`/finite `tail -n N`/single-file `tail -f`/`wc -l`/`grep` read-only slice, including pipelines and final output
redirection. Familiar off, uncertain editing, and `cmd` remain native. A
`prompt` PTY hook and a
`PSConsoleHostReadLine` PTY-writing wrapper were both rejected by earlier
boundary tests; neither is used.

If any input byte is forwarded before readiness arrives, that editor cycle is
permanently dirty and a late frame cannot upgrade it to `Reliable`. Queue
overflow, malformed authenticated frames, replay, worker failure, connect
failure, or bounded PowerShell write failure all suspend interception and
preserve the native editor path.

A marker is synchronization evidence against accidental ambiguity, not a
sandbox boundary against a hostile process already running as the same user.
The integration hook must be installed from protected packaged code, preserve
the user's visible prompt behavior, and not use a writable temporary profile.

After any submission Wingman enters `Running`. Only the next valid prompt
marker, or a confirmed nested-shell marker described below, starts a new empty
`Editing/Reliable` state. A timeout, plausible prompt text, or terminal silence
never does so.

## Input mirroring and Unicode

During `Editing`, ordinary input continues to the native line editor while a
bounded mirror records the same committed editing operations. The mirror is
evidence, not a second shell editor.

- Browser/IME pre-edit text is not forwarded or recorded as committed text.
  The final composition result is forwarded and inserted exactly once.
- Valid committed Unicode is preserved without NFC/NFD normalization.
- The mirror must not index by JavaScript UTF-16 code units or terminal screen
  cells. Its text-boundary behavior must match the supported shell adapter.
- Korean syllables and Jamo, combining marks, surrogate-pair characters, emoji,
  and double-width CJK text are required boundary-spike and acceptance cases.
- If an edit boundary cannot be matched exactly between the mirror and native
  editor, evidence becomes `Uncertain`; Wingman never repairs the line by
  guessing.
- Focus reports, bracketed-paste delimiters, and allowlisted terminal protocol
  responses are transport events, not command text.

The implementation review selects an explicit bounded line length. Exceeding
it stops mirroring and forces native pass-through for that submission; it does
not truncate, reinterpret, or partly execute the line.

## Editing allowlist and uncertainty

P0 must prove the following operations in both supported shells before they may
preserve `Reliable` evidence:

- committed text insertion at the current cursor;
- Backspace and Delete;
- Left, Right, Home, and End;
- Wingman-owned Up/Down recall replacement;
- `Ctrl+C` line cancellation.

An adapter may recognize normal and application cursor-key encodings only when
their effect is identical and tested. All other editing or control behavior is
native first. Tab completion, prediction acceptance, reverse/history search,
F7/F8/F9, Ctrl+Arrow, Alt chords, mouse repositioning, selection replacement,
Vi command mode, custom PSReadLine bindings, unknown CSI/SS3/OSC input, and an
incomplete or oversized escape sequence set evidence to `Uncertain` and are
forwarded unchanged.

Once uncertain, the current submission stays uncertain even if later text
looks simple. Enter then accepts the native editor's actual buffer without
calling `prepare_submission`. Reliability returns only at a later valid prompt
marker or session restart.

## Submission algorithm

All ordinary edit input has already reached the native editor. Enter is held
briefly only in `Editing/Reliable` so Rust can decide ownership.

```text
Enter
  -> not Editing/Reliable or Familiar OFF
       forward Enter only; enter Running
  -> Editing/Reliable
       prepare_submission(session, command_sequence, shell, mirrored_line)
         -> PassThrough { raw_line }
              require exact mirror match
              forward Enter only; enter Running
         -> InvokePrepared { request_id, display_line }
              require exact mirror/display match and valid request
              replace the known native edit buffer with the fixed runner
              invocation through the proved shell adapter
              submit once; enter Running
```

`PassThrough.raw_line` is a consistency value. The frontend does not resend the
line because the native editor already contains it. For a prepared rejection,
execution, or Familiar control, only the fixed installed runner path, fixed
transport fields, and one-shot request ID replace the line. User text never
becomes shell source.

Buffer replacement and submission use one serialized adapter operation. If a
session, sequence, line, or request check fails before replacement begins, the
request is invalidated and the original line receives native Enter. A failure
after replacement begins suspends interception and surfaces a bounded internal
error; Wingman does not retry, concatenate a second command, or pretend the
original line ran.

The boundary spike must prove replacement at every supported cursor position,
with Unicode and wide text, without deleting prompt content or the runner's
first output. A visible fixed internal invocation is an acceptable safe
fallback; brittle output filtering is not required.

## Completion and visible recall

Shell completion and prediction may replace arbitrary editor content, so their
use always selects native pass-through for that submission. Wingman does not
parse screen output to recover the completion result.

Wingman may keep a bounded, session-memory-only visible recall list containing
the nonempty raw lines the user submitted. It never substitutes an internal
runner invocation into that list. Up/Down may recall those entries only from
`Editing/Reliable` and only through the proved buffer-replacement adapter. A
failed replacement becomes uncertain.

When the user moves beyond Wingman's available recall range, native shell
history remains accessible by forwarding the history operation and marking the
line uncertain. The active shell retains its own configured history behavior.
Wingman does not erase, relocate, disable, or promise secrecy for native
history, and that history may contain the opaque internal runner invocation.

Session restart clears Wingman's visible recall. Persistent Wingman history is
outside P0 and would require explicit opt-in, retention, and deletion controls.

## Paste contract

Clipboard text is untrusted input and uses a dedicated paste path.

- A paste with no CR or LF is inserted immediately but never submits by itself.
  Plain committed text may remain reliable; control characters or an edit the
  adapter cannot mirror make the submission uncertain.
- A paste containing any CR or LF, including one trailing line break, is held
  before any byte reaches the PTY. Wingman shows one compact Send/Cancel
  confirmation with the logical line count.
- Cancel leaves the native edit buffer unchanged.
- Send forwards the paste as one native paste operation, preserving text and
  line order while encoding line boundaries in the supported shell's normal
  input form. Wingman does not split, classify, convert, or execute individual
  pasted lines.
- After a confirmed line-breaking paste, interception is suspended until a
  valid prompt marker re-establishes a fresh editing state.
- Bracketed-paste wrappers, when supported and proved by the shell adapter, are
  transport metadata and are never stored as command text.

Thus multiline paste can still run native commands after explicit confirmation,
but it cannot silently turn a pasted block into a sequence of Wingman-owned
operations.

## Shell transitions

The supported terminal shell kinds are `cmd.exe` and Windows PowerShell 5.1,
but only a validated PowerShell adapter may enter `Editing/Reliable` in P0.
Entering `cmd` suspends Familiar interception and keeps all input native. A
reliably captured standalone PowerShell line, case-insensitively equal after
outer whitespace to one of the following, is a documented transition candidate:

```text
cmd
cmd.exe
powershell
powershell.exe
```

Arguments, assignments, pipelines, redirection, wrappers, aliases, and paths do
not qualify. `pwsh`, `wsl`, `bash`, `ssh`, language REPLs, and other interactive
programs are ordinary native commands in P0.

Submitting a candidate records an expected child shell but does not immediately
change the active-shell stack. A matching valid child prompt marker confirms
the push. If no matching marker arrives, Wingman remains `Running` or
`Suspended` and forwards input natively.

At a confirmed nested-shell prompt, standalone `exit` records an expected pop
but does not pop early. The parent shell and its editing state become active
only after the matching parent prompt marker. Root-shell exit is confirmed by
process termination and closes the Wingman session. An alias, script, profile,
or child process cannot change the stack merely by printing prompt-like text.

P0 shell integration must establish valid PowerShell markers inside a
documented PowerShell transition without placing user data in shell source.
For `cmd` and every other child, Familiar interception is suspended. Wingman
never guesses a shell from its prompt.

## Interrupts, restart, and stale work

- In `Editing`, `Ctrl+C` is forwarded, the mirror is discarded, and Wingman
  waits for a valid fresh prompt marker.
- During a Wingman runner, `Ctrl+C` also follows the runner cancellation
  contract and the session remains non-editing until the prompt is confirmed.
- During native or unknown foreground work, interrupt input is passed through
  unchanged.
- Session restart creates a new session nonce, clears the shell stack and
  visible recall, cancels outstanding preparation, invalidates request IDs,
  and ignores all old PTY events and markers.
- Shell/process exit, PTY write failure, malformed integration state, or a
  sequence mismatch cannot reuse the last known line or prompt.

## Security and privacy rules

- Raw input, IME text, paste contents, mirrored buffers, native history, and PTY
  output are not written to production logs or telemetry.
- Only Rust owns session state, marker validation, preparation, and request
  invalidation. The WebView cannot assert that a prompt or line is reliable.
- Every input, output, marker, and decision carries the active session ID; stale
  events are discarded.
- Marker, escape, line, paste, recall, and pending-request storage are bounded.
- A terminal output escape sequence cannot invoke Tauri commands, access the
  clipboard, or mark a user line reliable.
- Familiar OFF disables preparation but does not disable safe paste confirmation
  or session isolation.

## Required validation matrix

The final application tests cover the following full matrix in Windows
PowerShell 5.1:

1. prompt markers split across reads, coalesced with output, stale, malformed,
   replayed, wrong-shell, wrong-depth, and prompt-like child output;
2. ASCII, Korean IME, Jamo/combining text, CJK width, emoji/surrogate pairs,
   middle insertion, Backspace, Delete, Home/End, and cursor movement;
3. Tab completion, prediction, Ctrl+R, F7/F8/F9, custom/unknown escape input,
   and the required native-pass-through fallback;
4. raw visible recall, buffer replacement, native-history fallback, session
   clearing, and possible opaque invocation in native history;
5. single-line paste, CR/LF/CRLF and trailing-newline paste, Send and Cancel,
   ordering, bracketed paste, and paste while a foreground program is active;
6. native pass-through, continuation input, a test interactive child, a
   full-screen-style child, and no interception before the next valid prompt;
7. confirmed standalone nested PowerShell transitions, transition into native
   `cmd` pass-through, arguments and wrappers that do not qualify, matching
   `exit`, missing markers, and root exit;
8. Familiar ON/OFF, prepared rejection/execution/control replacement, line and
   sequence mismatches, PTY write failure, session restart, and `Ctrl+C`.

Command migration cannot begin until this spike proves the prompt protocol,
Unicode-safe mirror, conservative fallback, and fixed-invocation replacement.
If a required operation cannot be proved, P0 narrows to native pass-through for
that operation rather than shipping heuristic interception.

The 2026-08-08 `cmd.exe` boundary spike established the P0 scope correction:
native `PROMPT` can emit a fixed marker, but cannot prove a prompt-by-prompt
sequence or nested-shell depth, and user prompt changes can remove the marker.
Accordingly, `cmd` acceptance tests cover exact native preservation, stale
session rejection, paste safety, foreground input, and absence of Familiar
interception—not marker-driven mirroring or editor replacement.

## Platform references

Microsoft documents that a pseudoconsole host is responsible for collecting
user input and rendering output, and that the pseudoconsole channel uses UTF-8:
[Pseudoconsoles](https://learn.microsoft.com/en-us/windows/console/pseudoconsoles).

Windows also documents that console input can contain virtual-terminal
sequences and that sequences may be split across writes:
[Console Virtual Terminal Sequences](https://learn.microsoft.com/en-us/windows/console/console-virtual-terminal-sequences).

PowerShell's native PSReadLine history may be saved to a host-specific file and
defaults can save incrementally; Wingman therefore cannot describe all shell
history as session-memory-only:
[Set-PSReadLineOption](https://learn.microsoft.com/en-us/powershell/module/PSReadline/set-psreadlineoption?view=powershell-5.1).
