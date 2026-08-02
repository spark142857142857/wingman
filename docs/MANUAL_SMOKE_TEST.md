# Wingman Manual Smoke Test

자동 테스트가 확인하기 어려운 실제 창, 폰트, 포커스, 클립보드와 대화형 PTY 동작을 확인하는 체크리스트입니다. 약 10분 정도 걸립니다.

## 1. Start

```powershell
npm run tauri dev
```

- [ ] Wingman 창이 한 번만 열린다.
- [ ] 터미널 글꼴이 `Cascadia Mono` 또는 fallback인 `Consolas`로 또렷하게 보인다.
- [ ] 기본 글자 크기 14px가 너무 작거나 크지 않다.
- [ ] 상태바에 `PowerShell`, Compat 상태, 시작 경로가 보인다.
- [ ] 첫 프롬프트가 잘리지 않고 입력 커서가 바로 활성화된다.

## 2. Create a disposable fixture

아래 경로만 테스트에 사용합니다.

```powershell
$TestRoot = Join-Path $env:TEMP 'wingman-manual-test'
Remove-Item -LiteralPath $TestRoot -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path (Join-Path $TestRoot 'nested') -Force | Out-Null
Set-Content -LiteralPath (Join-Path $TestRoot 'sample.txt') -Value @('TODO first','done','TODO last')
Set-Content -LiteralPath (Join-Path $TestRoot 'nested\sample.ts') -Value @('const value = "TODO";','done')
Set-Location $TestRoot
```

- [ ] 공백 없는 임시 경로에 fixture가 생성된다.
- [ ] 프롬프트 경로가 임시 폴더로 변경된다.

## 3. Native PowerShell

```powershell
Get-Process -Id $PID
Get-ChildItem
Get-Content sample.txt | Where-Object { $_ -match 'TODO' }
'native redirect' > native-result.txt
Get-Content native-result.txt
```

- [ ] 모든 명령이 일반 PowerShell과 동일하게 실행된다.
- [ ] 표와 색상 출력이 깨지지 않는다.
- [ ] `native-result.txt`에 `native redirect`가 기록된다.

## 4. PowerShell Familiar

```powershell
compat status
cat sample.txt | grep TODO | head -n 1
grep -rni TODO . --include '*.ts'
find . -iname '*.TS' -type f
cat sample.txt | cut -c '1-4'
'abc123' | tr 'a-z' 'A-Z'
cat sample.txt | sed 's/TODO/FIXME/g'
cat sample.txt | wc -l
```

- [ ] `compat status`가 현재 상태를 출력한다.
- [ ] 첫 파이프는 `TODO first` 한 줄만 출력한다.
- [ ] `grep` 결과에 `sample.ts`와 줄 번호가 보인다.
- [ ] `find`가 `sample.ts`를 찾는다.
- [ ] `cut` 결과가 각 줄의 앞 네 글자만 출력한다.
- [ ] `tr` 결과가 `ABC123`이다.
- [ ] `sed`가 `TODO`를 `FIXME`로 바꿔 출력하되 원본 파일은 수정하지 않는다.
- [ ] `wc -l` 결과가 `3`이다.

## 5. Compat OFF

```powershell
compat off
Get-ChildItem
Get-Content sample.txt
Get-Process -Id $PID
compat on
```

- [ ] OFF 상태가 상태바에 반영된다.
- [ ] 명시적인 PowerShell cmdlet은 계속 정상 동작한다.
- [ ] ON으로 되돌리면 Familiar 명령이 다시 동작한다.

## 6. Switch to cmd

```text
cmd
```

- [ ] 화면이 새 cmd 세션으로 전환된다.
- [ ] 상태바 Shell 값이 `cmd`로 바뀐다.

```bat
cd /d %TEMP%\wingman-manual-test
dir /b
cat sample.txt | grep TODO | head -n 1
cat sample.txt | tail -n 1
cat sample.txt | wc -l
cat sample.txt | grep TODO > cmd-result.txt
type cmd-result.txt
```

- [ ] `dir /b`가 일반 cmd처럼 동작한다.
- [ ] `head` 결과는 `TODO first`다.
- [ ] `tail` 결과는 `TODO last`다.
- [ ] `wc -l` 결과는 `3`이다.
- [ ] `cmd-result.txt`에 두 개의 `TODO` 줄이 저장된다.

## 7. Input and window behavior

- [ ] 위쪽 방향키로 이전 명령을 불러와 다시 실행할 수 있다.
- [ ] 왼쪽·오른쪽 방향키로 중간 문자를 수정할 수 있다.
- [ ] 긴 명령에서 백스페이스가 정상 동작한다.
- [ ] 실행 중인 명령을 `Ctrl+C`로 중단할 수 있다.
- [ ] 여러 줄 명령을 붙여넣어도 순서가 바뀌지 않는다.
- [ ] 텍스트 선택 후 `Ctrl+Shift+C`로 복사된다.
- [ ] `Ctrl+Shift+V`로 붙여넣어진다.
- [ ] `Ctrl` + `+`/`-`로 글자가 변하고 재시작 후 크기가 유지된다.
- [ ] 창을 작게·크게 바꿔도 프롬프트와 출력이 잘리지 않는다.
- [ ] `Ctrl+Shift+R` 후 이전 세션 출력이 새 세션에 섞이지 않는다.

## 8. Return to PowerShell and clean up

```text
powershell
```

```powershell
Remove-Item -LiteralPath (Join-Path $env:TEMP 'wingman-manual-test') -Recurse -Force
```

- [ ] 상태바 Shell 값이 `PowerShell`로 돌아온다.
- [ ] 임시 테스트 폴더가 삭제된다.

## Report format

실패한 경우 아래 형식으로 기록합니다.

```text
Test ID or section:
Shell: PowerShell / cmd
Compat: ON / OFF
Command:
Expected:
Actual:
Reproducible: always / intermittent
Screenshot or copied output:
```

