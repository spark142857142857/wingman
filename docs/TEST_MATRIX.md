# Wingman Test Matrix

This document defines Wingman's regression test baseline. Command behavior is automated where possible; only visual rendering and hands-on input behavior are checked manually.

## Verification commands

```powershell
npm run typecheck
npm test
npm run build
```

`npm test` includes TypeScript input parser and shell-state tests, PowerShell Familiar tests, native Windows shell regression tests, cmd Familiar tests, and Rust PTY session tests.

## Native PowerShell regression

Confirms that Linux Familiar mode does not interfere with native PowerShell functionality.

| ID | Mode | Command or behavior | Expected | Coverage |
| --- | --- | --- | --- | --- |
| PS-N01 | ON | `Get-Process -Id $PID` | Returns the current PowerShell process | Automated |
| PS-N02 | ON | `Get-ChildItem` | Returns items in the current directory | Automated |
| PS-N03 | ON | `Get-Content file.txt \| Where-Object { $_ }` | PowerShell object pipeline works | Automated |
| PS-N04 | ON | `'text' > result.txt` | File redirection works | Automated |
| PS-N05 | OFF | `ls`, `cat`, `rm`, `sort` | Behaves like the original PowerShell aliases | Automated |
| PS-N06 | ON/OFF | `git status`, `npm --version`, `python --version` | Passes through to installed external programs | Manual |
| PS-N07 | ON/OFF | `$env:PATH`, variables, and script blocks | Passes through as PowerShell syntax | Manual |

## PowerShell Linux Familiar

| ID | Command | Expected | Coverage |
| --- | --- | --- | --- |
| PS-F01 | `'Alpha','beta' \| grep -i alpha \| head -n 1` | `Alpha` | Automated |
| PS-F02 | `grep -rni TODO . --include '*.ts'` | Path, line number, and contents of TypeScript files | Automated |
| PS-F03 | `grep -w TODO file.txt` | Returns only the exact word `TODO` | Automated |
| PS-F04 | `grep -q missing file.txt` | No output; `$LASTEXITCODE` is 1 | Automated |
| PS-F05 | `find . -iname '*.TS' -type f` | Case-insensitive file search | Automated |
| PS-F06 | `find . -mindepth 2 -maxdepth 3 -size +10c -mtime 0` | Returns only items meeting every condition | Automated |
| PS-F07 | `'a,b,c' \| cut -d ',' -f '1,3'` | `a,c` | Automated |
| PS-F08 | `'abc123' \| tr 'a-z' 'A-Z'` | `ABC123` | Automated |
| PS-F09 | `'foo foo' \| sed 's/foo/bar/g'` | `bar bar` | Automated |
| PS-F10 | `'one two','three' \| xargs -n 2 command` | Safely passes arguments in groups of two | Automated |
| PS-F11 | `'3','1','1','2' \| sort -n \| uniq -c` | Counts adjacent duplicates after numeric sorting | Automated |
| PS-F12 | `'one two','three' \| wc -l -w` | `2 3` | Automated |
| PS-F13 | `mkdir -p path`, `touch file`, `rm -rf path` | Create, update, and delete work | Automated in temp sandbox |
| PS-F14 | `familiar on/off/status`, `fam` alias | Changes and displays state at runtime | Parser automated, UI manual |

## Native cmd regression

| ID | Mode | Command | Expected | Coverage |
| --- | --- | --- | --- | --- |
| CMD-N01 | ON/OFF | `dir /b` | Returns items in the current directory | Automated |
| CMD-N02 | ON/OFF | `set NAME=value` | cmd environment variables work | Automated |
| CMD-N03 | ON/OFF | `where cmd.exe` | Returns the cmd executable path | Automated |
| CMD-N04 | ON/OFF | `echo alpha \| findstr alpha` | `alpha` | Automated |
| CMD-N05 | ON/OFF | `echo text > result.txt` | cmd redirection works | Automated |
| CMD-N06 | ON/OFF | `git status`, `npm --version` | Passes through to installed external programs | Manual |

## cmd Linux Familiar

| ID | Command | Expected | Coverage |
| --- | --- | --- | --- |
| CMD-F01 | `ls -la` | Converts to `dir /a` | Automated |
| CMD-F02 | `mkdir -p demo\nested` | Creates nested directories | Automated in temp sandbox |
| CMD-F03 | `touch sample.txt` | Creates a file or updates its modification time | Automated in temp sandbox |
| CMD-F04 | `grep -inv missing app.txt` | Inverse search with line numbers | Automated |
| CMD-F05 | `cat app.txt \| grep TODO \| head -n 1` | The first `TODO` line | Automated |
| CMD-F06 | `cat app.txt \| tail -n 1` | The last line | Automated |
| CMD-F07 | `cat numbers.txt \| sort -n` | Numeric sorting | Automated |
| CMD-F08 | `cat app.txt \| wc -l` | Line count | Automated |
| CMD-F09 | `grep TODO < app.txt` | Search using input redirection | Mapping automated |
| CMD-F10 | `cat app.txt \| grep TODO > result.txt` | Creates a result file | Automated |
| CMD-F11 | `cp -r source target`, `rm -rf target` | Recursive copy and delete | Mapping and temp sandbox automated |
| CMD-F12 | Lines containing `&&`, `||`, or a single `&` | Passes through to cmd without conversion | Automated |

## PTY and frontend behavior

| ID | Behavior | Expected | Coverage |
| --- | --- | --- | --- |
| UI-01 | Previous-session output arrives after a new session starts | Ignores output from the previous session | Automated |
| UI-02 | A UTF-8 character is split across multiple PTY reads | Reassembles it without losing characters | Automated |
| UI-03 | Invalid UTF-8 bytes | Retains valid characters and replaces only invalid portions | Automated |
| UI-04 | Rapid consecutive input | Preserves input order | Code-path automated, UI manual |
| UI-05 | Arrow keys, Backspace, and Ctrl+C | Shell input and parser state remain in sync | Automated |
| UI-06 | Multi-line paste | Runs each line in order | Parser automated, UI manual |
| UI-07 | PowerShell: `cmd`, then cmd: `exit` | Keeps one PTY and the visible output; status changes to cmd and returns to PowerShell | Shell-state automated, UI manual |
| UI-08 | `Ctrl+Shift+R` | Starts a new session for the current shell | Manual |
| UI-09 | `Ctrl` + `+`/`-` | Changes and persists font size | Manual |
| UI-10 | Window resize | Fits the PTY column/row dimensions and view | Manual |
| UI-11 | Narrow window with a wide xterm child | Terminal stage and status bar remain within the viewport | Automated in headless Edge |

## Current compatibility boundary

- PowerShell Familiar is the reference implementation.
- cmd Familiar prioritizes core file commands and text pipelines.
- `cut`, `tr`, `sed`, and `xargs` are not yet supported in cmd.
- `wc` in cmd currently supports only `-l`.
- cmd conditional chaining with `&&`, `||`, and `&` is not converted by Familiar mode.
- Neither PowerShell nor cmd is an exact match for Linux output formats or exit codes.
