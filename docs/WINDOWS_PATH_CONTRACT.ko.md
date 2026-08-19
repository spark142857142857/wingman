# Windows 경로·파일 시스템 계약

상태: 현재 P0 경로·파일 시스템 계약이며 릴리스 후보에 구현되어 있다.

영문판: [WINDOWS_PATH_CONTRACT.md](WINDOWS_PATH_CONTRACT.md)

## 범위와 원칙

이 계약은 P0 명령 operand, 마지막 stdout redirection, CLI 시작 폴더, 검증,
실행, test가 공유하는 유일한 경로 기준이다. 개별 명령 계약은 유효한 경로를 더
제한할 수 있지만, 여기서 거부한 형태를 허용하거나 안전 규칙을 약화할 수 없다.

> 사용자 경로는 한 번 parsing하고, runner의 실제 파일 시스템 위치에서 한 번
> 해석하며, 문자열로 부족한 안전 판단에는 Windows object identity를 사용한다.

Wingman은 파일 시스템 sandbox가 아니다. 허용한 경로는 현재 Windows 접근 토큰이
접근할 수 있는 곳에 도달할 수 있다. 이 규칙은 모호함과 의도하지 않은 대상 확대를
막지만 같은 사용자 권한의 적대적 process가 파일 시스템을 동시에 바꾸는 상황까지
격리한다고 약속하지 않는다.

## 경로 값과 패턴은 다르다

`PathValue`와 `PatternValue`는 서로 다른 검증 타입이다.

- 모든 파일, 폴더, 원본, 대상, redirection operand는 `PathValue`이며 wildcard
  확장을 하지 않는다.
- `PathValue` 안의 `*`, `?`는 거부한다.
- `find -name "*.ts"` 같은 명령별 패턴은 `PatternValue`다. 문법은 해당 명령이
  정하며 경로로 해석하지 않는다.
- 따옴표는 lexer word를 묶고 경로 검증 전에 제거한다. 파일 이름 일부가 아니다.

## 허용하는 사용자 경로

| 종류 | 예시 | 규칙 |
| --- | --- | --- |
| 상대 경로 | `file.txt`, `src\main.ts`, `./src`, `.\src`, `..\logs` | Runner가 상속한 파일 시스템 cwd에서 해석한다. 두 separator 형태를 허용한다. |
| Drive absolute | `C:\work\file.txt`, `C:/work/file.txt` | Drive letter 뒤 separator가 반드시 필요하다. Drive letter는 대소문자를 구분하지 않는다. |
| UNC absolute | `\\server\share`, `\\server\share\folder` | 시작은 backslash 두 개여야 하고 server와 share가 비어 있으면 안 된다. 그 뒤 separator는 두 형태를 허용한다. |

`.`과 `..` component는 허용하고 lexical하게 해석한다. Drive root와 UNC share
root는 read-only operand로는 유효하며 파괴 명령 계약이 별도 root 보호를 적용한다.

`~`, `$HOME`, `%USERPROFILE%`은 확장하지 않는다. 다른 조건이 유효하면 문자
그대로의 파일 이름 component다.

## 거부하는 사용자 경로

| 형태 | 예시 | 이유 |
| --- | --- | --- |
| 빈 값 | `""` | 대상이 없다. |
| Drive relative | `C:`, `C:temp\file.txt` | Drive별 셸 현재 폴더에 따라 의미가 달라 runner 경계에서 안정적이지 않다. |
| Root relative | `\Windows`, `/home/user` | 현재 drive에 의존하고 Linux absolute path와 혼동된다. |
| Slash UNC | `//server/share` | P0 UNC는 `\\`로 시작하는 명시적인 Windows 표기만 허용한다. |
| Device·NT namespace | `\\?\...`, `\\.\...`, `\??\...`, `\\?\GLOBALROOT...`, volume GUID path | 일반 Win32 경로 해석을 우회하고 root·device 안전 검사를 복잡하게 만든다. |
| Alternate data stream | `file.txt:secret` | P0는 일반 파일·폴더만 다룬다. Drive letter 뒤 colon만 허용한다. |
| Wildcard path | `*.log`, `src\?.ts` | P0는 경로 operand를 확장하지 않는다. |
| 유효하지 않은 문자 | NUL, U+0001-U+001F, `<`, `>`, `"`, `|` | 일반 Win32 파일 이름이 아니거나 터미널 문법과 충돌한다. |
| 모호한 component | `name.`, `name ` | Win32 정규화가 화면 입력과 다른 이름을 가리킬 수 있다. |
| 예약 device component | `CON`, `NUL.txt`, `COM1`, `LPT9` | Win32가 파일이 아니라 device로 해석할 수 있다. |

예약 device 검사는 대소문자를 구분하지 않고 첫 dot 앞의 component 이름에
적용한다. 모든 component에서 `CON`, `PRN`, `AUX`, `NUL`, `CONIN$`, `CONOUT$`,
`COM1`-`COM9`, `LPT1`-`LPT9`를 거부한다.

해석된 절대 경로는 P0에서 UTF-16 code unit 4096개로 제한한다. 더 길면 미지원
문법으로 거부한다. 검증 후 내부적으로 long-path Win32 표기를 사용할 수는 있지만,
사용자가 입력한 `\\?\` prefix는 허용하거나 화면에 표시하지 않는다.

## 분류와 해석

같은 공통 Rust library가 두 신뢰 경계에서 다음 순서로 경로를 처리한다.

```text
host 준비:
lexer word
  -> Relative | DriveAbsolute | UncAbsolute 분류 또는 거부
  -> 문자, component, 예약 이름, wildcard, 길이 검증
  -> ExecutionPlan 안에 저장할 ValidatedPathSpec

runner 실행:
  -> ValidatedPathSpec을 방어적으로 다시 검증
  -> runner 파일 시스템 cwd와 위치 종류 확인
  -> 허용된 `/` separator를 `\`로 변환
  -> Relative 앞에 cwd 결합
  -> 반복 separator, `.`, `..`를 lexical하게 정리
  -> drive root 또는 UNC share root 위로 올라가면 거부
  -> 절대 native 표기를 가진 ResolvedPath
  -> 작업별 파일 시스템·identity 검사
```

Host는 셸의 현재 폴더를 추측하거나 host process에서 얻은 절대 경로를 저장하지
않는다. `ExecutionPlan`에는 검증된 경로 문법만 들어간다. Runner가 활성 셸의 실제
환경과 cwd를 상속한 뒤 `ResolvedPath`를 만든다. 해석된 경로와 file identity는
WebView로 반환하지 않는다.

Runner의 lexical 해석은 파일 시스템을 열거나 symbolic link, junction, mount point, 다른
reparse point를 따라가지 않는다. 따라서 아직 없는 destination leaf에도 적용할 수
있다. 파일 시스템 검사는 별도 단계다.

Unicode normalization은 하지 않는다. 사용자 원문 표기는 history와 진단에 남기고
해석된 표기는 실행에 사용한다. Forward slash는 입력 편의일 뿐이며 개별 명령이
달리 정하지 않으면 출력에는 native backslash를 사용한다.

## PowerShell과 cmd 위치

- `cmd`에서 상대 경로는 runner process가 상속한 현재 폴더를 사용한다. Cmd가
  drive별 현재 폴더를 추적해도 drive-relative operand는 거부한다.
- PowerShell에서는 runner 호출 전에 전달 shim이 현재 위치가 FileSystem provider에
  속함을 증명해야 한다.
- Non-filesystem provider 위치에서는 예전 process cwd로 fallback하지 않고 P0
  파일 시스템 요청을 문서화한 위치 오류로 실패시킨다.
- 경로 문자열은 runner를 호출하는 셸 명령에 보간하지 않는다.

## 비교와 object identity

Windows에서 문자열 비교만으로 안전을 판단하면 부족하다.

- Lexical 안전 비교는 separator를 정리한 뒤 ordinal·대소문자 비구분으로 한다.
  Case-sensitive 폴더에서는 alias를 보수적으로 거부할 수 있다.
- 기존 object의 원본·대상이 같은지 확인해야 하는 작업은 열린 handle에서 얻은
  volume/file identity를 사용한다.
- 이름이 다른 hard link도 identity가 같으면 `cp`, `mv`, redirection alias
  검사에서 같은 파일이다.
- 일반 hard-link 경로 하나를 `rm`하면 그 이름의 link만 제거한다.
- 출력 leaf가 아직 없으면 기존 parent를 검증하고 생성·open한 leaf를 쓰기 전에
  다시 확인한다.

## Reparse point 정책

Reparse point에는 symbolic link, junction, mount point, 그 밖의 Windows link
형태 object가 포함된다.

| 작업 종류 | P0 정책 |
| --- | --- |
| 명시적 비재귀 읽기 (`cat`, `head`, 파일 `grep`, `ls PATH`) | 일반 Windows 접근 규칙에 따라 사용자가 명시한 경로를 따라갈 수 있다. Wingman은 sandbox 격리를 주장하지 않는다. |
| 재귀 읽기 (`find`, `grep -r`) | 시작 경로 아래에서 만난 reparse point 안으로 들어가지 않는다. 시작 경로 자체가 reparse여도 순회하지 않는다. |
| `cp`/`mv` | Reparse 원본, 재귀 원본에서 발견한 reparse 항목, 작업에 관련된 reparse ancestor·기존 destination을 거부한다. |
| `mkdir`, `touch`, 출력 redirection | 쓰기 전에 기존 reparse target 또는 reparse ancestor를 거부한다. |
| `rm` | Ancestor는 reparse가 아니어야 한다. 명시한 leaf가 reparse면 target이 아니라 link 자체만 삭제한다. 재귀 순회는 reparse를 따라가지 않는다. |
| CLI 시작 폴더 | 사용자가 명시한 기존 폴더 경로를 따라갈 수 있으며 mutation을 하지 않는다. |

쓰기·파괴 경로가 reparse를 통과하는지 확인할 수 없으면 해당 작업을 수행하지 않고
실패한다.

## Root, 상위 관계, containment

작업별 검사는 해석된 경로와 가능한 경우 object identity를 함께 사용한다.

- 재귀 `rm`은 drive root, UNC share root, 현재 파일 시스템 폴더, 그 모든 상위
  폴더를 거부한다.
- 재귀 `cp` destination과 `mv` destination은 source 자체, source 내부, 다른
  표기로 가리킨 같은 object일 수 없다.
- Redirection target은 같은 실행 계획에서 입력으로 연 어떤 파일과도 같은
  identity일 수 없다.
- UNC server만 있는 경로는 유효하지 않다. UNC share root가 containment root이며
  `..`로 벗어날 수 없다.
- Mount point와 reparse alias가 이 검사를 약화하지 않는다.

첫 mutation 전에 모든 operand 형태와 미리 판단할 수 있는 안전 조건을 검증한다.
재귀 작업은 순회 중 identity와 reparse 상태를 다시 확인한다. 검사 사이에 파일
시스템이 바뀌어 안전한 identity를 유지할 수 없으면 새 대상을 따라가지 않고
실행 실패로 중단한다.

요청 전체 사전 검증 경계, 결정적 순회, staging·commit 시점, 부분 결과, 취소,
종료 상태 집계는 [mutation 실행 계약](MUTATION_EXECUTION_CONTRACT.ko.md)을 따른다.

## 오류

- 유효하지 않거나 미지원인 경로 형태, wildcard path, 예약 이름, 길이 초과,
  root 위 traversal, 확인된 안전 규칙 위반은 `2`로 종료한다.
- 형태가 유효하지만 없거나, 접근 불가·잠김·offline·unavailable 상태이거나 실제
  파일 시스템이 거부하면 `1`이다. 단, 없는 leaf에 대한 `rm -f` 같은 명령별
  결과 규칙을 우선한다.
- 쓰기·파괴 작업에서 identity나 reparse 상태를 조사할 수 없으면 계속하는 권한이
  아니라 안전한 실행 실패 `1`로 처리한다.
- 진단에는 operand와 위반 규칙을 표시하되 secret, 내부 namespace 변환, broker
  데이터를 출력하지 않는다.

## 필수 검증 matrix

최소한 다음을 시험한다.

```text
허용:
  file.txt
  .\src\main.ts
  ../src/main.ts
  C:\work\한글 파일.txt
  C:/work/project
  \\server\share\folder

경로 형태·안전 위반 종료 2:
  C:relative.txt
  \root-relative.txt
  /home/user/file
  //server/share/file
  \\?\C:\file.txt
  \\.\PhysicalDrive0
  file.txt:stream
  *.log
  folder\name.
  folder\NUL.txt
```

이름은 다르지만 같은 파일인 hard link, file·directory symbolic link, junction,
source 안 destination, 안전하게 가능한 drive·UNC root, 한글·대소문자 variant,
없는 leaf, locked file, 접근 거부, redirection alias, 통제된 경로 변경 race도
시험한다. 파괴 fixture는 검증된 일회용 test root 안에만 둔다.

## 구현 상태 메모 (2026-08-09)

`runner_io` 기반 구현은 이제 출력 경로의 각 상위 디렉터리를 이미 검증한 부모
디렉터리 handle에 상대적으로 열고, reparse 처리를 비활성화한 상태로 다음
구성요소를 검증한다. 최종 출력 leaf도 고정된 마지막 부모 handle에 상대적으로
연다. 부모 handle을 고정한 직후 원래 경로를 junction으로 바꾸는 통제된 테스트를
통해 다른 target을 따라가지 않는 것을 확인한다. 기존 reparse leaf와 ancestor는
truncate 전에 거부되며, reparse ancestor 아래의 없는 leaf도 생성하지 않는다.

Production sidecar는 이제 이 primitive를 검증된 `cat`·`head`·유한 `tail -n N`·단일 파일 `tail -f`·`wc -l`·`grep`·`sort`·`uniq` record stream의 `>`·`>>`에
연결한다. Integration test는 input-before-output 순서, overwrite·append, hard-link 동일 파일
거부, reparse 거부, output-open 실패, runtime partial output, 실제 redirected runner process
취소를 확인한다. Production PowerShell/ConPTY vertical test도 Unicode 경로의
`cat | head >`·`wc -l >`·`tail -n 1 >`·Unicode `grep -n >`·`uniq -c >`·`sort -n >` 제출을 이 primitive까지 연결한다. Familiar OFF, Uncertain, 명시적 native,
`cmd` 입력은 이 interception 경로 밖에 둔다.

## 조사 근거

Windows는 fully qualified, root-relative, drive-relative, UNC, device namespace
경로를 다르게 처리한다. 이 계약은 그중 제한된 범위만 의도적으로 지원한다.
[Microsoft: 파일·경로·namespace 이름 규칙](https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file)을 참고한다.
