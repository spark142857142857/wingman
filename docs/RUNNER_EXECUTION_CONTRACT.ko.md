# Runner 실행 계약

상태: 현재 P0 실행 계약이며 packaged runner에 구현되어 있다.

## 셸 경계와 파일 시스템 위치

runner는 실제 Windows 파일 시스템 위치에서만 실행한다.

- `cmd`에서는 자식 runner가 셸의 현재 Windows 폴더를 쓴다.
- PowerShell에서는 전달 shim이 runner 시작 전에 `Get-Location`이 FileSystem provider 위치인지 확인한다.
- `HKLM:\` 같은 파일 시스템이 아닌 PowerShell 위치에서는 shim이 runner를 시작하지 않는다.
  명확한 파일 시스템 위치 진단을 출력하고 종료 코드 `1`을 반환한다.

이 guard는 PowerShell이 파일 시스템이 아닌 provider에 있을 때 네이티브 자식 프로세스가 오래된
프로세스 폴더를 실수로 상속받는 일을 막는다.

Host는 검증된 경로 문법만 저장한다. Runner는 이 상속 위치를 기준으로
[Windows 경로·파일 시스템 계약](WINDOWS_PATH_CONTRACT.ko.md)에 따라 다시
검증·해석한다. Host process cwd나 프론트엔드가 만든 절대 경로로 대체하지 않는다.

## 요청 실행

```text
셸 전달 계층
  -> 일회용 요청 ID
  -> broker가 PreparedRequestV1 반환
  -> 프로토콜 버전 검사
  -> Reject: 준비된 진단을 출력하고 2로 종료
  -> Control: 준비된 응답을 출력하고 준비된 상태 반환
  -> Execute: 방어적인 실행 계획 검증
       -> 상속된 현재 폴더, 환경, PATH, 접근 토큰
       -> P0 실행
  -> stdout, stderr, 종료 코드
```

runner는 프론트엔드와 Rust host의 앞선 검증 모두와 독립적으로 준비 요청을
검증한다. 잘못된 요청 형태나 프로토콜 불일치는 `2`이고, 실제 환경·파일 시스템
실패는 개별 명령 계약이 달리 말하지 않는 한 `1`이다. Runner는 프론트엔드나
명령줄 인자에서 실행 계획을 직접 받지 않는다.

## 텍스트 스트림

P0는 [텍스트 record·stream 계약](TEXT_STREAM_MODEL.ko.md)의 구조화된
`RecordFrame { text, terminated }` pipeline을 쓴다. PowerShell object stream,
네이티브 셸 pipe, 명령별 raw byte 우회는 없다. 하나의 streaming decoder가 split
UTF-8, 선택적 source BOM, LF·CRLF framing, invalid 입력, 마지막 unterminated
record, record 상한을 처리한다. `cat`은 decoded record를 streaming하고 `sort`는
상한이 있는 논리 입력만 materialize한다.

마지막 sink만 BOM 없는 UTF-8과 CRLF로 encoding한다. P0는 text 전용이며 binary
복사나 입력 newline byte의 완전 보존은 약속하지 않는다.

## 출력과 리다이렉션

- 정상 데이터는 stdout, Wingman 진단은 stderr다.
- `>`는 마지막 대상 파일을 만들거나 비운다. `>>`는 만들거나 끝에 추가한다.
- stdout만 리다이렉션한다. 진단은 터미널에 계속 보인다.
- 없는 상위 폴더, 폴더 대상, 그 밖의 출력 열기 실패는 `1`이다.
- 출력은 스트리밍한다. 실행 실패나 취소가 나면 대상 파일에 부분 결과가 남을 수 있으며,
  원자적 교체는 P0 약속이 아니다.
- 명시적 regular-file 입력을 출력 sink보다 먼저 연다. 모든 문법·안전·same-file
  검사는 target을 바꿀 수 있는 open 전에 끝낸다. 정확한 순서와 append 동작은
  text stream 계약을 따른다.

## 파이프 종료 코드

P0는 `grep`의 미일치 같은 결과 상태와 실제 실행 실패를 구분한다.

```text
문법·안전·요청 검증 실패            -> 2
치명적인 파일 시스템·접근·디코딩 실패 -> 1
사용자 취소                          -> 130
그 외                                -> 마지막 단계의 종료 코드
```

따라서 `grep NOTHING app.log`는 `1`이고, `grep NOTHING app.log | head -n 5`는 마지막 `head`가
정상 종료했으므로 `0`일 수 있다. 다만 앞 단계의 치명적 실패는 뒤 단계의 성공보다 항상 우선한다.
정상 downstream short-circuit는 fatal이 아니며 synthetic broken-pipe 오류를 숨기고
읽지 않은 suffix data를 decode할 필요가 없다. 정확한 result·cancellation·fatal·진단
순서는 text stream 계약을 따른다.

## 구현된 read-only·redirection vertical slice (2026-08-10)

Reliable한 Familiar 입력의 `which NAME`도 이제 공통 catalog와 runner가 소유한다.
Runner의 현재 파일 시스템 폴더를 먼저 보고 상속받은 `PATH` snapshot을 순서대로
검색하며, 정리·중복 제거한 `PATHEXT` snapshot 또는 문서화된 기본값을 적용한다.
검색 폴더 중복은 대소문자 구분 없이 건너뛰고 처음 발견한 non-directory match의
정규화된 Windows 절대 경로를 출력한다. 셸을 호출하지 않으므로 셸 alias·function·
built-in이나 Wingman 호환 명령은 결과에 포함하지 않는다. Match가 없으면 진단 없이
result `1`이고 잘못된 이름은 실행 전에 거부한다.

`clear`는 검증된 독립 터미널 작업이다. Runner는 고정된 화면 지우기·커서 홈
sequence만 출력한다. 인자, pipeline, redirection은 거부하며 prepared control text가
터미널 escape 문자를 주입할 수는 없다.

`ls`와 정확한 long-form 별칭 `ll`은 같은 ordered text engine의 generated-record
source다. 디렉터리 바로 아래 항목 또는 명시한 파일 하나를 output mutation 전에
수집하고, 최대 262,144개 항목과 64 MiB filename text로 제한한다. Windows Unicode
ordinal ignore-case 순서와 case-sensitive ordinal tiebreaker로 정렬한다. `-a`는
Windows Hidden/System 속성을 따르고, `-l`은 고정된 `TYPE ATTRS SIZE MODIFIED NAME`
형식을 내며, `-h`는 `-l`과 함께일 때만 integer half-up IEC 크기 형식을 적용한다.
명시적인 비재귀 경로는 일반 Windows 접근 규칙으로 따라가되 발견한 reparse 항목은
type `l`로 표시한다. 생성 record는 지원되는 모든 ordered text stage와 기존
reparse-safe final redirection sink로 보낼 수 있다.

`find`는 두 번째 generated-record source다. 명시한 start를 depth 0에서 검사하고
`ls`와 같은 Windows ordinal child 순서로 depth-first pre-order 순회한다. 숨김 항목을
포함하지만 reparse entry나 reparse start 안으로는 절대 들어가지 않는다. 전용 bounded
Unicode glob matcher가 basename 전체에 `-name`/`-iname`을 적용하고, 순회기가 `-type`·
`-mindepth`·`-maxdepth`를 판정한다. 순회는 최대 100,000개 방문 항목과 depth 256으로
제한하며 취소·resource 실패 뒤에는 새 파일 시스템 object로 계속 진행하지 않는다.
상대 display path는 상대 형태와 native separator를 유지하고 `.`·`.\child` 형식을
보존한다. Find record도 같은 ordered stage와 안전한 final redirection으로 보내며,
정상적인 빈 검색은 status `0`이다.

비재귀 text stage는 plan에 선언된 왼쪽에서 오른쪽 순서 그대로 실행된다. 지원 stage는
반복하거나 다시 조합할 수 있으며, 반복 `grep`·`sort`·`uniq`·유한 `tail`과
`head`/`tail` 출력 뒤의 filter·materializing stage도 포함한다. Downstream `head`는
요청하지 않은 upstream 입력을 계속 단축 종료하지만, 치명적인 source 실패는 여전히
최종 stage 상태보다 우선한다. 이 의미론은 하나의 ordered stage engine이 소유하며,
runner는 더 이상 plan을 명령별 순서 보정 flag로 평탄화하지 않는다.

재귀 `grep`은 정렬된 directory frame 하나씩 열거하고 차례가 된 discovered file만 연다.
전체 file 목록을 미리 만들지 않으므로 downstream `head`가 멈추면 뒤의 하위 directory를
검사하지 않는다. 명시적 root directory handle은 redirection보다 먼저 연다. 그 뒤 output을
변경 없이 열어 root 내부의 기존 target인지 검사하고 traversal output 전에 commit한다.
Link가 여러 개인 target은 identity-only preflight를 거쳐 실제 input alias만 truncate 없이
거부하며, 관련 없는 multiply-linked output은 허용한다. Root 안에 새로 만든 target은 pinned
file identity로 traversal에서 제외한다.

Production sidecar는 이제 검증된 `clear`·`which`·`ls`/`ll`·`find`·`cat`·`head`·유한 `tail -n N`·단일 파일 `tail -f`·`wc -l`·`grep`·`sort`·`uniq` plan을 writer 기반 streaming entry point로
실행하고, normal stdout 또는 최종 `>`·`>>` file sink로 출력한다. 모든 명시적 input을
output보다 먼저 열고, pinned-parent/reparse-safe primitive로 redirection output을 열며,
overwrite truncate 전에 file identity를 검사한다. 공통 bounded UTF-8 reader로 각 파일을
decode하고 복수 파일 연결과 BOM 규칙을 보존한다. `cat -n`은 연속 번호를 사용하고 final
sink는 pending record 하나만 유지한다.

`wc -l`은 같은 bounded record stream을 materialize하지 않고 소비하며 input terminator가
있던 frame만 센다. 따라서 마지막 unterminated record는 세지 않는다. 정확히 한 file 또는
지원 pipeline input을 받고 generated terminated count record 하나를 출력한다.

유한 `tail`은 요청한 마지막 record만 보관한다. `N`을 기준으로 선할당하지 않으며
보관 ring은 최대 65,536개 record와 16 MiB의 record text로 제한한다. 어느 상한이든
넘으면 tail data를 출력하지 않고 종료 `1`이다. `tail -n 0`은 명시한 input을 열지만
payload는 decode하지 않는다. `tail -f`와 `--follow`는 정확히 한 파일만 받고 같은 bounded
초기 suffix를 보관한 뒤 제한된 간격으로 append byte를 확인한다. 완료 record는 즉시
flush하며 미종료 suffix는 이후 LF가 끝낼 때까지 공통 UTF-8 decoder에 보관한다. 취소는
그 suffix를 버리고 `130`으로 끝난다. 관찰된 truncation은 실행 실패이며 rotation은
추적하지 않는다.

`uniq`는 bounded 인접 그룹 하나만 메모리에 유지하고 전체 줄을 대소문자 구분하여 비교한다.
`-c`·`-d`·`-u`를 지원하며 마지막 그룹 구성원의 termination 상태를 보존하고, downstream
`head`·유한 `tail`·`wc -l`·안전한 redirection과 조합된다. 재귀 `grep -r`도 이제 같은
ordered stage로 출력을 보낸다.

`sort`는 출력 전에 전체 logical input을 검증하고 materialize한다. 최대 262,144개
record와 64 MiB의 record text로 제한하며, 기본 Unicode ordinal 순서와 floating point가
아닌 정확한 decimal sign·coefficient·scale 비교를 구현한다. 숫자 tie는 `-r`에서도
stable하고 `-u`는 text가 완전히 같은 record만 제거한다. Decode·numeric data·상한
실패는 sorted stdout을 내지 않는다. 재귀 `grep -r`도 반복 downstream filter와
materializing stage를 포함해 같은 ordered stage로 출력을 보낸다.

`head`는 필요한 prefix를 받은 직후 upstream reader를 멈춘다. OS read buffer에 들어왔더라도
record reader가 요청하지 않은 invalid UTF-8 suffix는 decode하지 않으므로 명령을 실패시키지
않는다. `cat` source 하나의 runtime 실패는 완료된 출력을 보존하고 exit `1`을 기록한 뒤
나중의 독립 파일을 계속 처리한다.
Redirect output도 같은 BOM 없는 UTF-8/CRLF encoder를 사용한다. Append는 BOM이나 숨은
separator를 추가하지 않고 diagnostic은 stderr에 남는다. Runtime 실패나 취소 뒤에는 위
계약대로 비어 있거나 부분적으로 작성된 target이 남을 수 있다.

이 slice는 typed runner request와 실제 `wingman-runner` process로 접근할 수 있다.
`clear`·`which`·`ls`/`ll`·`find`·`cat`·`head`·유한 `tail -n N`·단일 파일 `tail -f`·`wc -l`·`grep`·`sort`·`uniq`·`mkdir`·`touch`·`cp`·`mv`·`rm`은 Familiar ON이고 production PowerShell editor cycle이 FileSystem 위치에서
Reliable일 때 이제 분류·공개된다. 공통 lexer·parser·catalog는 하나의 typed plan을 만들거나
결정적인 exit `2` rejection을 준비한다. 명시적 `.exe`, native-first pipeline, Familiar OFF,
Uncertain 입력은 native pass-through를 유지한다. 실제 PowerShell/ConPTY test는 Familiar
활성화, OOB readiness, Unicode 경로, `cat | head >`, `wc -l >`, `tail -n 1 >`, Unicode `grep -n >`, `uniq -c >`, `sort -n >`, request broker, sidecar, 다음 readiness
cycle까지 검증한다. `cmd`는 입증된 editor-readiness adapter가 없으므로 native
pass-through를 유지한다.

## 취소

Ctrl+C는 runner 취소를 보낸다. 재귀 순회, 스트리밍, 대기, `tail -f`를 멈추고 출력 핸들을 닫은 뒤
`130`으로 종료한다. 이미 완료된 복사·이동·삭제 작업은 되돌리지 않는다. 리다이렉션 출력은 부분적으로
남을 수 있다.

Production sidecar는 실행 전에 Windows console control handler를 설치하고
`CTRL_C_EVENT`와 `CTRL_BREAK_EVENT`를 하나의 공유 cancellation token으로 변환한다.
Read-only 실행은 input open 전, record read 사이, sink 출력 전후, diagnostic 출력 전에
token을 확인한다. 수락된 취소는 동시에 발생한 operational I/O 실패보다 우선하고,
sink의 아직 commit되지 않은 pending record는 버리며, 완료된 stdout은 보존한 채 취소
diagnostic 없이 `130`으로 종료한다. Process-level test는 실제 sidecar를
`CREATE_NEW_PROCESS_GROUP`으로 실행하고 streaming 시작 뒤 group 범위
`CTRL_BREAK_EVENT`를 보내 partial output과 exit `130`을 검증한다.

변경 요청의 사전 검증, staging 정리, commit 경계, commit 뒤 취소 규칙은
[mutation 실행 계약](MUTATION_EXECUTION_CONTRACT.ko.md)을 따른다.

## runner 경계

runner는 검증된 P0 작업만 직접 구현한다. 명령을 만들기 위해 셸 소스를 다시 조립하거나, `cmd`·PowerShell을
재호출하거나, Bash 문법을 다시 해석하거나, 네이티브 명령을 중간 파이프 단계로 실행하지 않는다.
