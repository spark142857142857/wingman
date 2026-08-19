# Wingman Manual Smoke Test

> **Prototype checklist — not the common-interpreter acceptance contract.**
>
> It contains prototype commands now outside P0. Preserve it as migration
> evidence under the [prototype/target boundary](PROTOTYPE_TARGET_BOUNDARY.md).
> Use the current [release manual smoke test](RELEASE_SMOKE_TEST.md) for
> candidate acceptance.

This checklist covers the actual window, fonts, focus, clipboard, and interactive PTY behavior that automated tests cannot reliably verify. It takes about 10 minutes.

## 1. Start

```powershell
npm run tauri dev
```

- [ ] Only one Wingman window opens.
- [ ] The terminal font is clearly rendered as `Cascadia Mono`, or its `Consolas` fallback.
- [ ] The default 17px font size is neither too small nor too large.
- [ ] The status bar shows `PowerShell`, the Familiar state, and the starting directory.
- [ ] The first prompt is not clipped and the input cursor is immediately active.

## 2. Create a disposable fixture

Use only the following path for testing.

```powershell
$TestRoot = Join-Path $env:TEMP 'wingman-manual-test'
Remove-Item -LiteralPath $TestRoot -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path (Join-Path $TestRoot 'nested') -Force | Out-Null
Set-Content -LiteralPath (Join-Path $TestRoot 'sample.txt') -Value @('TODO first','done','TODO last')
Set-Content -LiteralPath (Join-Path $TestRoot 'nested\sample.ts') -Value @('const value = "TODO";','done')
Set-Location $TestRoot
```

- [ ] The fixture is created in a temporary path without spaces.
- [ ] The prompt path changes to the temporary directory.

## 3. Native PowerShell

```powershell
Get-Process -Id $PID
Get-ChildItem
Get-Content sample.txt | Where-Object { $_ -match 'TODO' }
'native redirect' > native-result.txt
Get-Content native-result.txt
```

- [ ] Every command runs as it would in regular PowerShell.
- [ ] Table and color output render correctly.
- [ ] `native-result.txt` contains `native redirect`.

## 4. PowerShell Familiar

```powershell
familiar status
cat sample.txt | grep TODO | head -n 1
grep -rni TODO . --include '*.ts'
find . -iname '*.TS' -type f
cat sample.txt | cut -c '1-4'
'abc123' | tr 'a-z' 'A-Z'
cat sample.txt | sed 's/TODO/FIXME/g'
cat sample.txt | wc -l
```

- [ ] `familiar status` prints the current state on one line.
- [ ] The first pipeline prints only `TODO first`.
- [ ] The `grep` result includes `sample.ts` and its line number.
- [ ] `find` locates `sample.ts`.
- [ ] `cut` prints only the first four characters of each line.
- [ ] `tr` outputs `ABC123`.
- [ ] `sed` prints `TODO` as `FIXME` without modifying the source file.
- [ ] `wc -l` outputs `3`.

## 5. Familiar OFF

```powershell
fam off
Get-ChildItem
Get-Content sample.txt
Get-Process -Id $PID
fam on
```

- [ ] The status bar reflects the OFF state.
- [ ] Explicit PowerShell cmdlets continue to work.
- [ ] Familiar commands work again after switching back to ON.

## 6. Switch to cmd

```text
cmd
```

- [ ] The existing output remains visible; entering cmd does not clear or replace the view.
- [ ] The status bar Shell value changes to `cmd`.
- [ ] cmd starts in the same temporary directory inherited from PowerShell.

```bat
cd
dir /b
cat sample.txt | grep TODO | head -n 1
cat sample.txt | tail -n 1
cat sample.txt | wc -l
cat sample.txt | grep TODO > cmd-result.txt
type cmd-result.txt
```

- [ ] `cd` prints the `%TEMP%\wingman-manual-test` path.
- [ ] `dir /b` behaves as it does in regular cmd.
- [ ] `head` outputs `TODO first`.
- [ ] `tail` outputs `TODO last`.
- [ ] `wc -l` outputs `3`.
- [ ] `cmd-result.txt` contains both `TODO` lines.

## 7. Input and window behavior

- [ ] The Up arrow recalls and reruns the previous command.
- [ ] The Left and Right arrows can edit a character in the middle of a command.
- [ ] Backspace works in long commands.
- [ ] `Ctrl+C` interrupts a running command.
- [ ] Pasting multiple commands preserves their order.
- [ ] Selected text copies with `Ctrl+Shift+C`.
- [ ] Both `Ctrl+V` and `Ctrl+Shift+V` paste text through Wingman.
- [ ] `Ctrl` + `+`/`-` changes the font, and the size persists after restarting.
- [ ] Resizing the window does not clip the prompt or output.
- [ ] Output from the previous session does not appear in a new session after `Ctrl+Shift+R`.

## 8. Exit cmd back to PowerShell and clean up

```text
exit
```

```powershell
Remove-Item -LiteralPath (Join-Path $env:TEMP 'wingman-manual-test') -Recurse -Force
```

- [ ] `exit` returns to the existing parent PowerShell without closing Wingman.
- [ ] The previous PowerShell and cmd output remains visible.
- [ ] The status bar Shell value returns to `PowerShell`.
- [ ] The temporary test folder is deleted.

## Report format

For a failure, record the following information.

```text
Test ID or section:
Shell: PowerShell / cmd
Familiar: ON / OFF
Command:
Expected:
Actual:
Reproducible: always / intermittent
Screenshot or copied output:
```
