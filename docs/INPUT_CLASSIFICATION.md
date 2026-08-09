# Input Classification Contract (Draft)

Status: accepted design direction for the common interpreter's ownership boundary.

## Result

```text
OwnershipDecision = PassThrough | Reject | Execute
```

The classifier's primary safety responsibility is to avoid changing input that
Wingman should not own.

This is a semantic ownership result inside Rust. It is not returned directly to
the WebView. `PassThrough` becomes the `FrontendDecisionV1` pass-through
variant; `Reject` and `Execute` are stored as `PreparedRequestV1` and become its
prepared-invocation variant. Both variants carry only the active session and
command-sequence envelope in addition to their documented payload. Reserved
controls use the same prepared request path after their state change is
validated.

## Decision order

```text
1. Reliable submitted line?
   no  -> PassThrough
2. Wingman control command?
   yes -> handle internally
3. Native shell-state command?
   yes -> PassThrough
4. First command is an unqualified P0 name?
   no  -> PassThrough
5. P0 one-line grammar valid?
   no  -> Reject (exit 2)
6. Every pipeline stage is a P0 command?
   no  -> Reject (exit 2)
7. Command contracts, sources, and safety rules valid?
   no  -> Reject (exit 2)
   yes -> Execute
```

## Reliable input only

Wingman interprets only a line submitted from a validated native-shell prompt
and reconstructed from the editing allowlist in the [terminal submission and
session contract](TERMINAL_SESSION_CONTRACT.md). Unknown editing, shell
completion, multiline paste, a foreground program, or uncertain shell identity
prevents classification. In that case Wingman forwards only the user's native
input operation; it never replaces the editor buffer with a guessed line.

## Reserved and native commands

`familiar on`, `familiar off`, and `familiar status`, with the accepted short
aliases, are reserved product controls and are handled before classification.

`cd`, `chdir`, `pushd`, `popd`, `Set-Location`, and `exit` pass through. They
remain owned by the native shell and its session-state model.

## P0 ownership

With Familiar mode enabled, Wingman claims an input line only when its first
actual command word is an unqualified P0 name, compared case-insensitively.

```text
grep TODO app.log       -> Wingman
GREP TODO app.log       -> Wingman
git status              -> pass through
grep.exe TODO app.log   -> pass through
.\grep.exe TODO app.log -> pass through
C:\tools\grep.exe TODO app.log -> pass through
```

An explicit executable suffix or path is an explicit request for native
execution. Shared names such as `find` and `sort` use Wingman's P0 meaning
while Familiar mode is enabled; a user may use the native `.exe` form or turn
the mode off to bypass that ownership.

## Claimed P0 syntax

Once it claims a P0 command, Wingman either executes it according to contract
or rejects it. It never partly converts or forwards unsupported syntax to the
shell.

P0 accepts only words, single/double quoted words, `--`, pipeline separators,
and one final `>` or `>>` output redirection. Shell chains, input/error
redirection, command substitution, environment expansion, and other general
shell syntax are not interpreted.

Unquoted shell operators such as `&&`, `||`, `;`, `&`, `<`, `2>`, and misplaced
redirection cause rejection in a claimed P0 line. `>>` is valid only as its one
final output redirection. `$HOME` is a literal P0 argument, never a Wingman
environment expansion.

Wildcards are not expanded by the P0 shell grammar. A command contract may
allow a wildcard as its own pattern argument, such as `find -name "*.ts"`;
otherwise it is rejected.

## Pipelines

Wingman P0 pipelines are wholly owned pipelines: every stage must be a P0
command and the catalog must approve its text source and text output.

```text
cat app.log | grep ERROR | head -n 10  -> Execute
find src -type f | wc -l               -> Execute
grep TODO app.txt | findstr TODO       -> Reject
git log | grep fix                     -> PassThrough
```

The last example is not a Wingman compatibility promise. Native-output to
Wingman-text bridging is a possible future feature, not P0.

## Diagnostics

Rejected input produces a consistent diagnostic and exit code `2`, naming the
claimed command and the unsupported construct where possible. A diagnostic may
suggest disabling Familiar mode for intentional native shell syntax.

The diagnostic is stored in a prepared rejection. The frontend receives only
its request ID; the runner prints the diagnostic and returns `2` through the
active shell.

## Required classification examples

| Input | Result |
| --- | --- |
| `git status` | PassThrough |
| `cd ..` | PassThrough |
| `find.exe /v "" file.txt` | PassThrough |
| `grep -in TODO app.txt` | Execute |
| `grep -z TODO app.txt` | Reject |
| `grep TODO *.txt` | Reject |
| `cat app.log | grep ERROR | head -n 3` | Execute |
| `cat app.log | powershell Get-Date` | Reject |
| `git log | grep fix` | PassThrough |
| `grep TODO app.txt && dir` | Reject |
| `familiar off` | internal control |
| Familiar off: `grep TODO *.txt` | PassThrough |

## Current activation status (2026-08-09)

The production classifier now activates this ownership algorithm for `cat`,
`head`, finite `tail -n N`, `wc -l`, `grep`, and `uniq`. It requires Reliable evidence and Familiar on, uses the shared
lexer/parser/read-only catalog, prepares deterministic rejections for claimed
invalid lines, and preserves native pass-through for explicit executable names
and native-first pipelines. The PowerShell FileSystem/OOB editor path is proved
through the real ConPTY, broker, sidecar, Unicode-path redirection, and next
readiness cycle. Other planned P0 names remain unpublished; `cmd` has no trusted
editor adapter and remains native pass-through.
