# Wingman Release Manual Smoke Test

Korean version: [RELEASE_SMOKE_TEST.ko.md](RELEASE_SMOKE_TEST.ko.md)

Status: current P0 manual gate. The old prototype checklist remains in
[MANUAL_SMOKE_TEST.md](MANUAL_SMOKE_TEST.md) only as migration evidence.

This procedure validates the exact release candidate after the automated
sections of [RELEASE_TEST_MATRIX.md](RELEASE_TEST_MATRIX.md). It intentionally
does not test prototype-only commands.

## Record before testing

```text
Commit:
Executable or installer SHA-256:
Installed / unpackaged release:
Windows edition, version, and build:
Display resolution and scale:
Keyboard and IME:
PowerShell version:
cmd version:
Tester and date:
```

Use an ordinary, non-elevated account for this pass. Close other Wingman
windows before starting. Do not use a development server or debug build.

## SM-01 — Prepare a disposable fixture

In a separate ordinary PowerShell window, create one isolated test directory:

```powershell
$WingmanSmokeRoot = Join-Path $env:TEMP 'wingman-release-smoke'
Remove-Item -LiteralPath $WingmanSmokeRoot -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path (Join-Path $WingmanSmokeRoot 'nested') -Force | Out-Null
[IO.File]::WriteAllText((Join-Path $WingmanSmokeRoot 'sample.txt'), "TODO first`ndone`nTODO last`n", [Text.UTF8Encoding]::new($false))
[IO.File]::WriteAllText((Join-Path $WingmanSmokeRoot 'nested\sample.ts'), "const value = 'TODO';`n", [Text.UTF8Encoding]::new($false))
```

- [ ] `SM-01a` The fixture exists only below the printed temporary path.
- [ ] `SM-01b` `sample.txt` contains three LF-terminated UTF-8 lines.

Keep this outside PowerShell open for launching, appending to a followed file,
and final cleanup.

## SM-02 — Launch the PowerShell session

Use the installed command, or replace `wingman` with the exact release
executable under test:

```powershell
wingman --shell powershell -- "$WingmanSmokeRoot"
```

- [ ] `SM-02a` The launcher returns successfully and exactly one usable Wingman window opens.
- [ ] `SM-02b` The status shows `PowerShell`, `Familiar: PAUSED`, and the fixture directory.
- [ ] `SM-02c` The first prompt is fully visible, focused, and accepts input immediately.
- [ ] `SM-02d` Text, cursor, ANSI colour, and the Cascadia Mono/Consolas fallback render cleanly at 100% and the machine's normal display scale.

## SM-03 — Preserve native PowerShell

Enter these commands while Familiar is still paused:

```powershell
Get-Location
Get-Content sample.txt | Where-Object { $_ -match 'TODO' }
$env:WINGMAN_SMOKE = 'native-ok'; Write-Output $env:WINGMAN_SMOKE
'native redirect' > native-result.txt
Get-Content native-result.txt
```

- [ ] `SM-03a` The location is the fixture directory.
- [ ] `SM-03b` The object pipeline prints both TODO records.
- [ ] `SM-03c` Variables, statement separators, and native redirection keep normal PowerShell behavior.
- [ ] `SM-03d` No command is replaced by a Wingman diagnostic or runner invocation.

## SM-04 — Enable and exercise the P0 Familiar surface

```text
familiar status
familiar on
familiar status
pwd
ls -lah
which powershell
cat sample.txt
cat -n sample.txt
cat sample.txt | grep TODO | head -n 1
grep -in TODO sample.txt
find . -type f -name "*.txt" | sort | uniq | wc -l
cat sample.txt | grep TODO > result.txt
cat sample.txt | head -n 1 >> result.txt
tail -n 1 sample.txt
```

- [ ] `SM-04a` Status changes from `Familiar: OFF` to `Familiar: ON`, and the app status changes from `PAUSED` to `ON`.
- [ ] `SM-04b` `pwd` prints an absolute native Windows path and `ls -lah` produces the contracted long listing.
- [ ] `SM-04c` `which powershell` prints an executable path.
- [ ] `SM-04d` `cat -n` numbers all three records; the first pipeline prints only `TODO first`.
- [ ] `SM-04e` `grep -in` prints lines 1 and 3, and the find/sort/uniq/count pipeline prints `2` (`native-result.txt` and `sample.txt`).
- [ ] `SM-04f` `result.txt` contains both TODO records followed by `TODO first`, in that order.
- [ ] `SM-04g` `tail -n 1` prints `TODO last`.
- [ ] `SM-04h` Output and diagnostics are UTF-8 clean and the prompt returns after each finite command.

The exact `ls -l`, `grep`, newline, and ordering formats are defined by the
command contracts; do not compare them to GNU output that Wingman does not
promise.

## SM-05 — Mutations stay inside the fixture

```text
mkdir -p work\a\b
touch "work\한글 이름.txt"
cp "work\한글 이름.txt" work\copy.txt
mv work\copy.txt work\moved.txt
ls -a work
rm work\moved.txt
rm -rf work\a
ls -a work
```

- [ ] `SM-05a` Nested directories and both files are created only under `work`.
- [ ] `SM-05b` The Korean filename renders without replacement characters.
- [ ] `SM-05c` Copy, move, file removal, and recursive directory removal have exactly the requested effects.
- [ ] `SM-05d` No `.wingman-stage-*` item remains.

Do not substitute a path outside `$WingmanSmokeRoot` in this section.

## SM-06 — Rejection and native bypass

```text
grep -E "TODO|done" sample.txt
grep TODO *.txt
familiar off
Get-ChildItem
Get-Content sample.txt | Select-Object -First 1
familiar on
```

- [ ] `SM-06a` The two claimed P0 lines fail with concise unsupported-syntax diagnostics; they are not partly converted or forwarded for native execution.
- [ ] `SM-06b` `familiar off` changes the app status to `PAUSED`.
- [ ] `SM-06c` Explicit PowerShell cmdlets and its object pipeline remain native and successful while Familiar is off.
- [ ] `SM-06d` `familiar on` restores the status to `ON`.

## SM-07 — Editing, history, completion, Unicode, and interrupt

- [ ] `SM-07a` Type `cat sample.txt`, move Left into the middle, edit it, move End, and submit; the visible edit and executed line agree.
- [ ] `SM-07b` Recall a native command with Up and submit it; it executes once with no injected Wingman command visible in history.
- [ ] `SM-07c` Use Tab completion on `Get-Chi`, then submit; PowerShell handles completion and the command executes natively.
- [ ] `SM-07d` Enter Korean through the active IME and type an emoji in a quoted `Write-Output` command; display and submission contain no corruption.
- [ ] `SM-07e` Backspace/Delete around Korean text and at the middle of a line do not duplicate, drop, or replace unrelated characters.

Now run:

```text
tail -n 1 -f sample.txt
```

Append a record from the separate PowerShell window:

```powershell
[IO.File]::AppendAllText((Join-Path $WingmanSmokeRoot 'sample.txt'), "followed`n", [Text.UTF8Encoding]::new($false))
```

- [ ] `SM-07f` `followed` appears once.
- [ ] `SM-07g` `Ctrl+C` returns to a usable prompt promptly, reports no stale runner, and a later `pwd` succeeds.

## SM-08 — Clipboard safety

- [ ] `SM-08a` Select visible terminal text and press `Ctrl+Shift+C`; the clipboard receives exactly the selection.
- [ ] `SM-08b` Copy a single line without a line break and press `Ctrl+V`; it is inserted but not submitted until Enter.
- [ ] `SM-08c` Copy two commands containing a line break and press `Ctrl+V`; one warning appears before any pasted byte reaches the PTY.
- [ ] `SM-08d` Choose Cancel; neither command is inserted or executed.
- [ ] `SM-08e` Paste again and choose OK/Send; the original line order and boundaries are preserved, each command executes once, and no per-line Familiar conversion occurs.

`Ctrl+Shift+V` is not a P0 shortcut. Do not record browser or OS behavior for it
as Wingman paste support.

## SM-09 — Native foreground child and cmd root session

In the PowerShell-root Wingman window, enter:

```text
cmd
cd
dir /b
echo child-ok
exit
```

- [ ] `SM-09a` The foreground child receives native input, keeps existing output visible, and inherits the fixture directory.
- [ ] `SM-09b` `exit` returns to the original PowerShell prompt; `pwd` works again after editor readiness resumes.
- [ ] `SM-09c` The status continues to identify the selected root session as PowerShell; it does not claim Familiar interception inside the child.

Close that window. From the separate PowerShell window launch a `cmd` root:

```powershell
wingman --shell cmd -- "$WingmanSmokeRoot"
```

Enter:

```bat
cd
dir /b
set WINGMAN_SMOKE=cmd-ok
echo %WINGMAN_SMOKE%
echo alpha|findstr alpha
familiar on
```

- [ ] `SM-09d` Status shows `cmd` and `Familiar: PAUSED`.
- [ ] `SM-09e` Native cwd, directory listing, variable expansion, and native pipeline behave as in ordinary `cmd.exe`.
- [ ] `SM-09f` `familiar on` is passed to cmd unchanged: the app remains `PAUSED` and does not print Wingman's `Familiar: ON` response. Record an environment conflict if an unrelated executable named `familiar` exists.

## SM-10 — Window, session generation, and persistence

- [ ] `SM-10a` Resize from narrow to wide and maximize/restore; the prompt, status bar, and terminal remain inside the viewport.
- [ ] `SM-10b` `Ctrl`+`+`/`=` and `Ctrl`+`-` change the font without losing focus or terminal contents.
- [ ] `SM-10c` Start a visibly long native output, press `Ctrl+Shift+R`, and confirm old-session output never appears in the new session.
- [ ] `SM-10d` Input typed during or immediately after restart reaches only the new session or is safely ignored; it never reaches both.
- [ ] `SM-10e` Close and relaunch the same shell. The chosen font size persists, Familiar starts `PAUSED`, and the terminal is usable.

## SM-11 — Clean up and report

Close every Wingman window. In the separate PowerShell window, verify the exact
target and remove only the disposable fixture:

```powershell
$ResolvedSmokeRoot = [IO.Path]::GetFullPath($WingmanSmokeRoot)
if ($ResolvedSmokeRoot -ne [IO.Path]::GetFullPath((Join-Path $env:TEMP 'wingman-release-smoke'))) {
  throw "Unexpected smoke root: $ResolvedSmokeRoot"
}
Remove-Item -LiteralPath $ResolvedSmokeRoot -Recurse -Force
```

- [ ] `SM-11a` All Wingman, runner, selected shell, and test-only console processes from this run exit.
- [ ] `SM-11b` The fixture is removed; unrelated files, user profiles, and Windows Terminal windows are unchanged.

For every failure, record:

```text
Checklist ID:
Shell and Familiar state:
Exact input:
Expected:
Actual:
Always / intermittent:
Screenshot or copied output:
Relevant process state:
```

The manual gate passes only when every applicable checkbox passes or the
release scope is explicitly narrowed and its contracts are updated first.
