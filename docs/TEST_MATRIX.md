# Wingman Test Matrix

Wingman의 회귀 테스트 기준 문서입니다. 명령 동작은 가능한 한 자동화하고, 실제 창의 렌더링과 입력 감각만 수동으로 확인합니다.

## Verification commands

```powershell
npm run typecheck
npm test
npm run build
```

`npm test`에는 TypeScript 입력 파서·셸 상태, PowerShell Familiar, 일반 Windows 셸 회귀, cmd Familiar, Rust PTY 세션 테스트가 포함됩니다.

## Native PowerShell regression

Linux Familiar가 PowerShell 고유 기능을 망가뜨리지 않는지 확인합니다.

| ID | Mode | Command or behavior | Expected | Coverage |
| --- | --- | --- | --- | --- |
| PS-N01 | ON | `Get-Process -Id $PID` | 현재 PowerShell 프로세스 반환 | Automated |
| PS-N02 | ON | `Get-ChildItem` | 현재 폴더 항목 반환 | Automated |
| PS-N03 | ON | `Get-Content file.txt \| Where-Object { $_ }` | PowerShell 객체 파이프 정상 | Automated |
| PS-N04 | ON | `'text' > result.txt` | 파일 리다이렉션 정상 | Automated |
| PS-N05 | OFF | `ls`, `cat`, `rm`, `sort` | 원래 PowerShell 별칭과 같은 동작 | Automated |
| PS-N06 | ON/OFF | `git status`, `npm --version`, `python --version` | 설치된 외부 프로그램에 그대로 전달 | Manual |
| PS-N07 | ON/OFF | `$env:PATH`, 변수·스크립트 블록 | PowerShell 문법에 그대로 전달 | Manual |

## PowerShell Linux Familiar

| ID | Command | Expected | Coverage |
| --- | --- | --- | --- |
| PS-F01 | `'Alpha','beta' \| grep -i alpha \| head -n 1` | `Alpha` | Automated |
| PS-F02 | `grep -rni TODO . --include '*.ts'` | TypeScript 파일의 경로·줄 번호·본문 | Automated |
| PS-F03 | `grep -w TODO file.txt` | 완전한 단어 `TODO`만 반환 | Automated |
| PS-F04 | `grep -q missing file.txt` | 출력 없음, `$LASTEXITCODE`는 1 | Automated |
| PS-F05 | `find . -iname '*.TS' -type f` | 대소문자 무시 파일 검색 | Automated |
| PS-F06 | `find . -mindepth 2 -maxdepth 3 -size +10c -mtime 0` | 모든 조건을 만족하는 항목만 반환 | Automated |
| PS-F07 | `'a,b,c' \| cut -d ',' -f '1,3'` | `a,c` | Automated |
| PS-F08 | `'abc123' \| tr 'a-z' 'A-Z'` | `ABC123` | Automated |
| PS-F09 | `'foo foo' \| sed 's/foo/bar/g'` | `bar bar` | Automated |
| PS-F10 | `'one two','three' \| xargs -n 2 command` | 인자를 2개씩 안전하게 전달 | Automated |
| PS-F11 | `'3','1','1','2' \| sort -n \| uniq -c` | 숫자 정렬 후 인접 중복 개수 | Automated |
| PS-F12 | `'one two','three' \| wc -l -w` | `2 3` | Automated |
| PS-F13 | `mkdir -p path`, `touch file`, `rm -rf path` | 생성·갱신·삭제 정상 | Automated in temp sandbox |
| PS-F14 | `compat on/off/status` | 실행 중 상태 전환과 상태 표시 | Parser automated, UI manual |

## Native cmd regression

| ID | Mode | Command | Expected | Coverage |
| --- | --- | --- | --- | --- |
| CMD-N01 | ON/OFF | `dir /b` | 현재 폴더 항목 반환 | Automated |
| CMD-N02 | ON/OFF | `set NAME=value` | cmd 환경 변수 정상 | Automated |
| CMD-N03 | ON/OFF | `where cmd.exe` | cmd 실행 파일 경로 반환 | Automated |
| CMD-N04 | ON/OFF | `echo alpha \| findstr alpha` | `alpha` | Automated |
| CMD-N05 | ON/OFF | `echo text > result.txt` | cmd 리다이렉션 정상 | Automated |
| CMD-N06 | ON/OFF | `git status`, `npm --version` | 설치된 외부 프로그램에 그대로 전달 | Manual |

## cmd Linux Familiar

| ID | Command | Expected | Coverage |
| --- | --- | --- | --- |
| CMD-F01 | `ls -la` | `dir /a`로 변환 | Automated |
| CMD-F02 | `mkdir -p demo\nested` | 중첩 폴더 생성 | Automated in temp sandbox |
| CMD-F03 | `touch sample.txt` | 파일 생성 또는 수정 시간 갱신 | Automated in temp sandbox |
| CMD-F04 | `grep -inv missing app.txt` | 줄 번호 포함 역검색 | Automated |
| CMD-F05 | `cat app.txt \| grep TODO \| head -n 1` | 첫 번째 `TODO` 줄 | Automated |
| CMD-F06 | `cat app.txt \| tail -n 1` | 마지막 줄 | Automated |
| CMD-F07 | `cat numbers.txt \| sort -n` | 숫자 정렬 | Automated |
| CMD-F08 | `cat app.txt \| wc -l` | 줄 수 | Automated |
| CMD-F09 | `grep TODO < app.txt` | 입력 리다이렉션 검색 | Mapping automated |
| CMD-F10 | `cat app.txt \| grep TODO > result.txt` | 결과 파일 생성 | Automated |
| CMD-F11 | `cp -r source target`, `rm -rf target` | 재귀 복사·삭제 | Mapping and temp sandbox automated |
| CMD-F12 | `&&`, `||`, 단일 `&`가 포함된 줄 | Wingman이 변환하지 않고 cmd에 그대로 전달 | Automated |

## PTY and frontend behavior

| ID | Behavior | Expected | Coverage |
| --- | --- | --- | --- |
| UI-01 | 새 세션 직후 이전 세션 출력 도착 | 이전 세션 출력 무시 | Automated |
| UI-02 | UTF-8 문자가 여러 PTY read로 분할 | 문자 손실 없이 결합 | Automated |
| UI-03 | 잘못된 UTF-8 바이트 | 유효 문자는 유지하고 잘못된 부분만 대체 | Automated |
| UI-04 | 빠른 연속 입력 | 입력 순서 보존 | Code-path automated, UI manual |
| UI-05 | 방향키·백스페이스·Ctrl+C | 셸 입력과 내부 파서 상태 일치 | Automated |
| UI-06 | 여러 줄 붙여넣기 | 줄마다 순서대로 실행 | Parser automated, UI manual |
| UI-07 | `cmd`, `powershell` 입력 | 새 셸 세션으로 전환하고 상태바 갱신 | Parser automated, UI manual |
| UI-08 | `Ctrl+Shift+R` | 현재 셸의 새 세션 시작 | Manual |
| UI-09 | `Ctrl` + `+`/`-` | 글자 크기 변경·저장 | Manual |
| UI-10 | 창 크기 변경 | PTY 열·행 크기와 화면 맞춤 | Manual |

## Current compatibility boundary

- PowerShell Familiar가 기준 구현입니다.
- cmd Familiar는 핵심 파일 명령과 텍스트 파이프를 우선 지원합니다.
- cmd의 `cut`, `tr`, `sed`, `xargs`는 아직 지원하지 않습니다.
- cmd에서 `wc`는 현재 `-l`만 지원합니다.
- cmd의 조건 연결 `&&`, `||`, `&`는 Familiar 변환 대상이 아닙니다.
- PowerShell과 cmd 모두 실제 Linux와 출력 형식·종료 코드가 완전히 같지는 않습니다.

