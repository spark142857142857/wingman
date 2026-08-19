# Wingman 릴리스 수동 스모크 테스트

영문판: [RELEASE_SMOKE_TEST.md](RELEASE_SMOKE_TEST.md)

상태: 현재 P0 수동 게이트. 과거 prototype checklist인
[MANUAL_SMOKE_TEST.md](MANUAL_SMOKE_TEST.md)는 migration 증거로만 보존한다.

이 절차는 [RELEASE_TEST_MATRIX.ko.md](RELEASE_TEST_MATRIX.ko.md)의 자동 구간 뒤
정확한 릴리스 후보를 검증한다. Prototype 전용 명령은 의도적으로 검사하지 않는다.

## 검사 전 기록

```text
Commit:
실행 파일 또는 installer SHA-256:
설치본 / unpackaged release:
Windows edition, version, build:
Display 해상도와 배율:
Keyboard와 IME:
PowerShell version:
cmd version:
검사자와 날짜:
```

일반 비관리자 계정에서 검사한다. 시작 전에 다른 Wingman 창을 닫는다. 개발 server나
debug build를 사용하지 않는다.

## SM-01 — 일회용 fixture 준비

별도의 일반 PowerShell 창에서 격리한 test directory 하나를 만든다.

```powershell
$WingmanSmokeRoot = Join-Path $env:TEMP 'wingman-release-smoke'
Remove-Item -LiteralPath $WingmanSmokeRoot -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path (Join-Path $WingmanSmokeRoot 'nested') -Force | Out-Null
[IO.File]::WriteAllText((Join-Path $WingmanSmokeRoot 'sample.txt'), "TODO first`ndone`nTODO last`n", [Text.UTF8Encoding]::new($false))
[IO.File]::WriteAllText((Join-Path $WingmanSmokeRoot 'nested\sample.ts'), "const value = 'TODO';`n", [Text.UTF8Encoding]::new($false))
```

- [ ] `SM-01a` 출력한 임시 path 아래에만 fixture가 있다.
- [ ] `SM-01b` `sample.txt`에는 LF로 끝나는 UTF-8 record 3개가 있다.

이 외부 PowerShell은 launch, follow 파일 append, 최종 cleanup에 사용하므로 열어 둔다.

## SM-02 — PowerShell session 실행

설치한 명령을 사용하거나 `wingman`을 검사할 정확한 release 실행 파일로 바꾼다.

```powershell
wingman --shell powershell -- "$WingmanSmokeRoot"
```

- [ ] `SM-02a` Launcher가 성공하고 사용할 수 있는 Wingman 창이 정확히 하나 열린다.
- [ ] `SM-02b` 상태에 `PowerShell`, `Familiar: PAUSED`, fixture directory가 보인다.
- [ ] `SM-02c` 첫 prompt가 잘리지 않고 focus되어 즉시 입력된다.
- [ ] `SM-02d` 100%와 machine의 일반 display 배율에서 text, cursor, ANSI color, Cascadia Mono/Consolas fallback이 깨끗하다.

## SM-03 — Native PowerShell 보존

Familiar가 paused인 상태에서 입력한다.

```powershell
Get-Location
Get-Content sample.txt | Where-Object { $_ -match 'TODO' }
$env:WINGMAN_SMOKE = 'native-ok'; Write-Output $env:WINGMAN_SMOKE
'native redirect' > native-result.txt
Get-Content native-result.txt
```

- [ ] `SM-03a` 위치는 fixture directory다.
- [ ] `SM-03b` Object pipeline이 TODO record 둘을 출력한다.
- [ ] `SM-03c` Variable, statement separator와 native redirection이 PowerShell 원래 동작을 유지한다.
- [ ] `SM-03d` Wingman diagnostic이나 runner invocation이 native 명령을 대체하지 않는다.

## SM-04 — P0 Familiar surface 활성화와 실행

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

- [ ] `SM-04a` Status가 `Familiar: OFF`에서 `Familiar: ON`으로 바뀌고 앱 상태도 `PAUSED`에서 `ON`으로 바뀐다.
- [ ] `SM-04b` `pwd`는 절대 native Windows path를 출력하고 `ls -lah`는 계약한 long listing을 출력한다.
- [ ] `SM-04c` `which powershell`은 실행 파일 path를 출력한다.
- [ ] `SM-04d` `cat -n`은 record 3개에 번호를 붙이고 첫 pipeline은 `TODO first`만 출력한다.
- [ ] `SM-04e` `grep -in`은 1번과 3번 줄을 출력하고 find/sort/uniq/count pipeline은 `2`를 출력한다(`native-result.txt`, `sample.txt`).
- [ ] `SM-04f` `result.txt`에는 TODO record 둘과 `TODO first`가 그 순서로 있다.
- [ ] `SM-04g` `tail -n 1`은 `TODO last`를 출력한다.
- [ ] `SM-04h` 출력과 diagnostic이 UTF-8로 깨지지 않고 finite 명령마다 prompt가 돌아온다.

정확한 `ls -l`, `grep`, newline, ordering 형식은 명령 계약을 따른다. Wingman이 약속하지
않은 GNU 출력과 비교하지 않는다.

## SM-05 — Mutation을 fixture 안으로 제한

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

- [ ] `SM-05a` Nested directory와 두 file이 `work` 아래에만 생긴다.
- [ ] `SM-05b` 한글 filename에 replacement character가 없다.
- [ ] `SM-05c` Copy, move, file 제거, recursive directory 제거가 요청한 효과만 낸다.
- [ ] `SM-05d` `.wingman-stage-*` item이 남지 않는다.

이 절에서 `$WingmanSmokeRoot` 밖의 path로 바꾸지 않는다.

## SM-06 — 거부와 native bypass

```text
grep -E "TODO|done" sample.txt
grep TODO *.txt
familiar off
Get-ChildItem
Get-Content sample.txt | Select-Object -First 1
familiar on
```

- [ ] `SM-06a` Claimed P0 line 둘은 간결한 unsupported-syntax diagnostic으로 실패하며 일부만 변환하거나 native 실행으로 전달하지 않는다.
- [ ] `SM-06b` `familiar off` 뒤 앱 상태가 `PAUSED`로 바뀐다.
- [ ] `SM-06c` Familiar off에서도 명시한 PowerShell cmdlet과 object pipeline이 native로 성공한다.
- [ ] `SM-06d` `familiar on` 뒤 상태가 다시 `ON`이 된다.

## SM-07 — 편집, history, completion, Unicode, interrupt

- [ ] `SM-07a` `cat sample.txt`를 입력하고 Left로 중간을 편집한 뒤 End와 Enter를 누른다. 화면 편집 결과와 실행한 line이 같다.
- [ ] `SM-07b` Up으로 native command를 불러 실행한다. 한 번만 실행되고 history에 내부 Wingman command가 보이지 않는다.
- [ ] `SM-07c` `Get-Chi`에서 Tab completion 뒤 제출한다. PowerShell이 completion을 처리하고 native command가 실행된다.
- [ ] `SM-07d` 활성 IME로 한글을 입력하고 quoted `Write-Output` command에 emoji를 입력한다. 표시와 제출이 깨지지 않는다.
- [ ] `SM-07e` 한글 주변과 line 중간에서 Backspace/Delete를 써도 관련 없는 문자가 중복, 누락, 교체되지 않는다.

다음을 실행한다.

```text
tail -n 1 -f sample.txt
```

별도 PowerShell 창에서 record를 추가한다.

```powershell
[IO.File]::AppendAllText((Join-Path $WingmanSmokeRoot 'sample.txt'), "followed`n", [Text.UTF8Encoding]::new($false))
```

- [ ] `SM-07f` `followed`가 한 번 보인다.
- [ ] `SM-07g` `Ctrl+C`로 prompt가 빠르게 돌아오고 stale runner가 없으며 이후 `pwd`가 성공한다.

## SM-08 — Clipboard 안전

- [ ] `SM-08a` 보이는 terminal text를 선택하고 `Ctrl+Shift+C`를 누르면 선택한 내용만 clipboard에 들어간다.
- [ ] `SM-08b` Line break 없는 한 줄을 복사하고 `Ctrl+V`를 누르면 삽입만 되며 Enter 전에는 제출되지 않는다.
- [ ] `SM-08c` Line break가 있는 두 command를 복사하고 `Ctrl+V`를 누르면 어떤 pasted byte도 PTY에 가기 전에 경고가 한 번 뜬다.
- [ ] `SM-08d` 취소하면 어느 command도 삽입되거나 실행되지 않는다.
- [ ] `SM-08e` 다시 paste하고 확인/전송하면 원래 line 순서와 경계가 보존되고 각 command가 한 번씩 실행되며 line별 Familiar 변환은 없다.

`Ctrl+Shift+V`는 P0 shortcut이 아니다. Browser나 OS 동작을 Wingman paste 지원으로
기록하지 않는다.

## SM-09 — Native foreground child와 cmd root session

PowerShell-root Wingman 창에서 입력한다.

```text
cmd
cd
dir /b
echo child-ok
exit
```

- [ ] `SM-09a` Foreground child가 native 입력을 받고 기존 출력은 남으며 fixture directory를 상속한다.
- [ ] `SM-09b` `exit` 뒤 원래 PowerShell prompt로 돌아오고 editor readiness가 복구된 뒤 `pwd`가 동작한다.
- [ ] `SM-09c` 상태는 선택한 root session인 PowerShell을 계속 표시하고 child 안에서 Familiar interception을 주장하지 않는다.

그 창을 닫고 별도 PowerShell에서 `cmd` root를 실행한다.

```powershell
wingman --shell cmd -- "$WingmanSmokeRoot"
```

입력한다.

```bat
cd
dir /b
set WINGMAN_SMOKE=cmd-ok
echo %WINGMAN_SMOKE%
echo alpha|findstr alpha
familiar on
```

- [ ] `SM-09d` 상태는 `cmd`, `Familiar: PAUSED`다.
- [ ] `SM-09e` Native cwd, directory listing, variable 확장과 native pipeline이 일반 `cmd.exe`처럼 동작한다.
- [ ] `SM-09f` `familiar on`은 cmd에 그대로 전달된다. 앱은 `PAUSED`에 남고 Wingman의 `Familiar: ON` 응답을 출력하지 않는다. 무관한 `familiar` 실행 파일이 환경에 있으면 충돌로 기록한다.

## SM-10 — Window, session generation, persistence

- [ ] `SM-10a` Narrow/wide resize와 maximize/restore 뒤 prompt, status bar, terminal이 viewport 안에 남는다.
- [ ] `SM-10b` `Ctrl`+`+`/`=`와 `Ctrl`+`-`로 font가 바뀌며 focus와 terminal 내용이 유지된다.
- [ ] `SM-10c` 눈에 보이는 긴 native 출력을 시작하고 `Ctrl+Shift+R`을 누르면 old-session 출력이 새 session에 나타나지 않는다.
- [ ] `SM-10d` Restart 도중이나 직후 입력은 새 session에만 가거나 안전하게 무시되며 두 session에 함께 가지 않는다.
- [ ] `SM-10e` 같은 shell을 닫고 다시 실행하면 font size는 유지되고 Familiar는 `PAUSED`에서 시작하며 terminal을 사용할 수 있다.

## SM-11 — Cleanup과 보고

모든 Wingman 창을 닫는다. 별도 PowerShell에서 정확한 target을 확인하고 일회용
fixture만 제거한다.

```powershell
$ResolvedSmokeRoot = [IO.Path]::GetFullPath($WingmanSmokeRoot)
if ($ResolvedSmokeRoot -ne [IO.Path]::GetFullPath((Join-Path $env:TEMP 'wingman-release-smoke'))) {
  throw "Unexpected smoke root: $ResolvedSmokeRoot"
}
Remove-Item -LiteralPath $ResolvedSmokeRoot -Recurse -Force
```

- [ ] `SM-11a` 이번 실행의 모든 Wingman, runner, 선택 shell, test-only console process가 종료된다.
- [ ] `SM-11b` Fixture는 제거되고 무관한 file, user profile, Windows Terminal 창은 바뀌지 않는다.

실패마다 다음을 기록한다.

```text
Checklist ID:
Shell과 Familiar 상태:
정확한 입력:
Expected:
Actual:
항상 / 간헐적:
Screenshot 또는 복사한 출력:
관련 process 상태:
```

적용되는 checkbox가 모두 통과하거나 먼저 release scope와 계약을 명시적으로 좁혀야
수동 게이트를 통과한다.
