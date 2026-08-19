# 전체 계획 통합 재검토

상태: 과거의 전체 계획 재검토다. 이후 사용자가 구현을 승인했고 P0 코드 cutover는
현재 릴리스 후보에 반영되어 있다.

영문판: [CONSOLIDATED_PLAN_REVIEW.md](CONSOLIDATED_PLAN_REVIEW.md)

## 결론

제품 방향은 일관성이 있으며 처음부터 다시 짤 필요가 없다. 네이티브 Windows
셸 위의 작은 Unix 친숙성 계층, 하나의 Rust 호환 core, 구조화된 실행 계획,
전용 runner, 일회용 로컬 전달, 네이티브 원문 통과 경계는 실현 가능한 P0 구조다.

검토에서 계약 blocker 4개와 가치가 큰 수정 6개를 찾았고 문서 수준에서 모두 닫았다.
이후 사용자가 구현을 명시적으로 승인했고 경계 기술 검증과 공통 runner도 구현했다.
아래 prototype test 결과는 과거 migration 증거로 남으며, 현재 수락은 prototype
결과가 아니라 release matrix와 contract suite를 따른다.

## 그대로 유지할 결정

- 제품 역할: **Windows 셸은 그대로, Unix 명령 습관도 그대로.** Linux, Bash,
  WSL, POSIX runtime을 제공하지 않는다.
- 대상 사용자: 터미널 명령을 이미 사용하며 네이티브 Windows 환경에서도 익숙한
  Unix 명령 습관을 쓰고 싶은 사람이다.
- P0 범위: 문서화한 제한 명령, 전체가 P0인 텍스트 pipeline, 마지막 stdout
  redirection 하나만 지원한다.
- 네이티브 PowerShell, cmd, Windows 관리, 환경·상태 명령은 원문 통과로 유지한다.
- Familiar OFF에서는 모든 입력을 원문 통과한다.
- 하나의 공통 Rust classifier, lexer, validator, plan model, command engine이
  프론트엔드 cmd 변환과 명령별 PowerShell 함수를 대체한다.
- `wingman-runner.exe`가 검증된 P0 작업을 직접 실행하며 셸 소스를 조립해
  구현하지 않는다.
- 초기 UI는 Tauri/WebView2를 유지하고 core는 renderer와 분리한다.
- 초기 지원은 Windows 11 24H2 이상 x64, Windows PowerShell 5.1, `cmd.exe`다.
  PowerShell 7, Windows 10/Server, P1 명령, 원격 규칙 전달, 네이티브 renderer
  재작성은 P0 밖에 둔다.
- 성능 편법보다 보안과 정확성을 우선한다.
- 사용자가 이 수정된 계획을 다시 검토하고 명시적으로 승인하기 전에는 구현하지 않는다.

## 반드시 수정할 계약

| ID | 심각도 | 발견 내용 | 필요한 해결 |
| --- | --- | --- | --- |
| C1 | Blocker | 데이터 모델은 프론트엔드가 직렬화된 `RunnerRequestV1`을 전달한다고 쓰지만, 나중에 정한 전달 계약은 Rust가 계획을 보관하고 일회용 요청 ID만 반환한다. | 전달 계약을 기준으로 통일한다. 프론트엔드는 session·command-sequence envelope 안에서 `PassThrough { raw_line }` 또는 `InvokePrepared { request_id, display_line }`만 받는다. Broker만 `PreparedRequestV1 = Reject | Execute | Control`을 runner에 전달한다. |
| C2 | Blocker | 공통 Windows 경로 계약이 없다. drive-relative, root-relative, `/home`처럼 보이는 입력, UNC, device namespace, ADS, 긴 경로, 끝의 점·공백, 대소문자 alias, reparse race가 대상을 바꾸거나 root 검사를 우회할 수 있다. | 모든 명령, redirection, CLI 경로, validator, test가 쓰는 하나의 경로·파일 시스템 계약을 추가한다. P0는 일반 상대 경로, drive-absolute, 명시적 UNC를 받고 drive-relative, device/NT namespace, ADS, 명령 패턴 밖 wildcard, 모호한 이름은 거부한다. 파괴적 안전 검사는 문자열 정규화만 믿지 않고 handle/file identity와 링크 비순회 규칙을 사용한다. |
| C3 | Blocker | 신뢰 가능한 입력 복원과 활성 셸 상태가 가장 어려운 경계인데, 지원 편집, Unicode 지우기, completion fallback, 여러 줄 paste, 중첩 셸 전환, 알 수 없는 escape/edit 이후 상태를 정한 전용 계약이 없다. | 터미널 제출·세션 계약을 추가한다. 알려진 편집만으로 복원한 줄만 해석하고 알 수 없는 편집과 completion은 통과시킨다. 문서화한 standalone 셸 전환만 지원하고 셸 정체가 불명확하면 interception을 멈춘다. 명령 마이그레이션 전에 두 셸에서 Unicode, 넓은 문자, history recall, Ctrl+C, paste, 생성 호출 교체를 경계 기술 검증으로 증명한다. |
| C4 | Blocker | Runner 계약은 `cat`이 chunk를 직접 stream할 수 있다고 하면서 UTF-8 decoding과 CRLF 직렬화도 약속한다. BOM 출력, 마지막 newline 없는 줄, `head` 조기 완료, upstream 중단, 치명 상태 우선순위가 정의되지 않았다. | 명령 전에 하나의 text-record/stream model을 정한다. Streaming decoder가 분할 UTF-8과 선택적 입력 BOM을 처리하고 newline 상태를 명시한다. decode하지 않은 raw chunk 우회는 없다. 정상 short-circuit, 치명 오류 우선순위, redirection open 순서, backpressure, 부분 출력 동작을 정한다. |
| C5 | High | 파괴적·복수 대상 동작이 불완전하다. 안전하지 않은 `rm` 대상 하나가 전체 삭제를 막는지, 일부 `mkdir`/`touch`/`cp`/`mv` 실패 후 계속하는지, redirection과 입력 파일이 같은 경우가 불명확하다. | 첫 mutation 전에 전체 문법과 모든 안전 규칙을 검증한다. 안전 위반은 아무것도 바꾸지 않고 `2`로 종료한다. 실행 중 실패는 문서화한 부분 결과를 남길 수 있고 `1`로 종료한다. 출력 대상이 입력과 같은 file identity면 거부한다. `rm`은 구현 마지막에 둔다. |
| C6 | High | 하나의 구현과 결정적 test를 만들기에 일부 명령 계약이 부족하다. `grep` regex/class와 재귀 순회, `find` wildcard와 출력 경로, `sort -n` 숫자 문법·비교기, `ls -l/-h` 열·시간·크기 형식, `which` PATH/PATHEXT 해석, 복수 입력 오류 순서가 해당된다. | 기존 명령 문서에서 의미를 닫는다. GNU 전체 폭보다 작고 결정적인 규칙을 우선한다. 재귀 결과 순서는 미지정으로 유지할 수 있지만 test는 결과 집합을 비교하고 진단·종료 상태는 결정적으로 확인한다. |
| C7 | High | 보안 문서는 히스토리가 세션 메모리에만 있고 paste가 자체 실행되지 않는다고 썼다. 기본 PowerShell PSReadLine은 history를 점진 저장하며 현재 초안은 붙여넣은 줄바꿈을 제출로 보낸다. | Wingman 소유 데이터에 한정해 약속한다. Wingman은 P0에서 명령·출력 영구 히스토리를 추가하지 않고 네이티브 셸은 사용자 설정을 유지한다. 줄바꿈이 있는 paste는 전달 전에 짧은 확인 한 번을 거친다. P0 원문은 Wingman 화면 recall에 남고 네이티브 shell history에는 내부 opaque 호출이 남을 수 있다. |
| C8 | High | `wingman` CLI 동작은 정했지만 셸 호출이 반환된 뒤 GUI 창을 유지할 process topology가 없다. 인자 조합·순서, GUI subsystem, 오류 전달, 같은 binary 재실행이 미정이다. | CLI/GUI handoff를 경계 기술 검증으로 만든다. 하나의 binary가 내부 GUI mode로 자신을 detached spawn할지 별도 launcher를 패키징할지 정확한 grammar와 함께 정한다. 다른 signed internal binary가 필요하다는 검증 결과가 없으면 공개 이름은 `wingman.exe`, `wingman-runner.exe`를 유지한다. |
| C9 | High | 마이그레이션 계획은 대부분의 명령 뒤에 전체 pipeline, redirection, cancellation engine을 구현하도록 했지만 명령 의미가 이 기능에 의존한다. | 실제 명령 전에 runner skeleton, streaming/pipeline engine, redirection, 상태 전달, 자원 제한, cancellation을 test stage로 만든다. 그다음 read-only 명령, 나중에 mutation, 마지막에 `rm`을 연결한다. |
| C10 | Medium | 현재 초안과 목표 문서가 함께 있다. README는 Windows 10, P1 명령, 입력 redirection, legacy mapping을 계속 약속하고 기존 test matrix는 P0 밖의 동작을 의도적으로 시험한다. 성능 수치도 아직 보정 전이다. | 기존 test는 마이그레이션 기준으로 유지하되 공개 약속은 cutover 때 교체한다. Legacy 기대값을 그대로 변형하지 않고 새 계약 suite를 추가한다. 경계 기술 검증 단계에서 성능 예산을 한 번 측정·보정하고 P0 합격까지 고정한다. |
| C11 | Blocker | 원래 계획은 네이티브 `cmd`가 PowerShell과 같은 신뢰 prompt·editor 경계를 제공할 수 있다고 가정했다. 2026-08-08 검증에서 `PROMPT`는 prompt별 sequence와 중첩 depth를 제공하지 못하고, 사용자 prompt 변경으로 marker가 사라지며, 안전한 네이티브 buffer 교체 수단도 증명되지 않았다. | `cmd.exe`는 지원 네이티브 터미널로 유지하되 P0의 모든 `cmd` 입력은 원문 통과로 둔다. Familiar interception은 패키지 Windows PowerShell 5.1 PSReadLine adapter에서만 활성화한다. `cmd` Familiar는 별도 검토를 거친 hook 또는 Wingman 소유 line editor가 있을 때 다시 평가한다. |

## 계약 수정 진행 상태

- **C1 2026-08-06 해결:** 의미상 소유권 판정과 `FrontendDecisionV1`을 분리했다.
  Rust가 `PreparedRequestV1`을 보관하고 WebView에는 session·sequence envelope와
  원문 통과 또는 불투명한 일회용 요청 ID·display line만 반환한다. Reject,
  Execute, Control은 같은 broker 경로를 사용한다.
- **C2 2026-08-06 해결:** 하나의 공통 Windows 경로 계약이 허용 형태, 거부
  namespace, host `ValidatedPathSpec`, runner 측 해석, file identity, root,
  hard link, reparse 정책을 정의한다.
- **C3 2026-08-06 해결:** 터미널 제출·세션 계약이 검증된 prompt 증거, 보수적인
  세션 상태 기계, Unicode-safe mirrored 편집, completion·알 수 없는 편집 뒤의
  지속적인 uncertain 상태, 네이티브 foreground 통과, 확인된 셸 stack 전환을
  요구한다. 명령 migration 전에 경계 기술 검증을 반드시 통과해야 한다.
- **C4 2026-08-06 해결:** 하나의 `RecordFrame { text, terminated }` 계약이 streaming
  UTF-8·BOM decoding, LF·CRLF framing, final newline, 명령 transform, 출력
  encoding, redirection open 순서, bounded backpressure, 정상 short-circuit, fatal
  우선순위, `tail -f`, 부분 출력을 소유한다. 명령별 raw-byte 우회를 금지한다.
- **C5 2026-08-06 해결:** mutation 계약이 요청 전체 무변경 안전 사전 검증과 순서가
  있는 실행 작업을 분리했다. `cp`·`mv` staging·commit, `rm` 전체 대상 사전 검증,
  redirection identity, 취소, 부분 상태, 진단, 종료 집계를 고정했다.
- **C6 2026-08-06 해결:** 명령 계약이 P0 regex·glob grammar, Unicode folding,
  순회·표시 경로, 정확한 decimal sort, `ls -l/-h` field·rounding, `which` 해석,
  multi-source 실패 순서를 고정했다. Acceptance plan에 결정적 fixture도 명시했다.
- **C7 2026-08-06 해결:** Wingman 소유 recall만 세션 메모리에 두고 네이티브 셸
  history는 설정된 동작을 유지하며 불투명한 호출을 담을 수 있다. 줄바꿈 paste는
  Send/Cancel 확인 한 번 뒤 줄별 Wingman 판정 없이 하나의 네이티브 paste로 남는다.
- **C8 2026-08-06 해결:** 공개 console launcher가 같은 signed `wingman.exe`를 보호된
  내부 GUI role로 시작하고 bounded 양방향 readiness handoff를 기다린다. 필수 기술
  검증이 실패하면 별도 internal GUI binary를 추가하기 전에 계약을 다시 연다.
- **C9 2026-08-06 해결:** 실제 명령 전에 runner, `RecordFrame` pipeline, redirection,
  상태 우선순위, resource bound, cancellation을 만들고 시험한다. Read-only 명령 뒤
  mutation을 연결하고 `rm`은 마지막이다.
- **C10 2026-08-06 해결:** README·legacy test 문서를 prototype snapshot으로 표시하고
  target 권위를 분리했다. 승인 뒤 contract-v1 test를 legacy 증거 옆에 추가하며
  cutover 규칙과 사용자 승인 뒤 고정할 1단계 성능 보정 한 번을 정했다.

## 권장하는 최소 제품 결정

다음 기본값은 핵심 가치를 줄이지 않으면서 P0 크기를 줄인다.

- 예약 명령은 `familiar on`, `familiar off`, `familiar status`와 짧은 `fam`
  형태만 둔다. 문서화되지 않은 `compat` 별칭은 목표 계약에서 제거한다.
- 클릭 가능한 터미널 링크는 P0에서 제외한다. URL은 선택·복사 가능한 텍스트로
  남기고 현재 URL 열기 권한과 link-handler 공격 표면을 제거한다.
- P0는 원격 호환 정의를 내려받지 않는다. 호환 의미는 서명된 Wingman release로만
  변경한다.
- Wingman 자체는 P0에서 명령·출력 영구 히스토리를 저장하지 않는다. 활성 셸의
  자체 history 설정을 몰래 바꾸지 않는다.
- 줄바꿈이 있는 paste에는 send/cancel 확인을 한 번만 보여준다. 한 줄 paste는
  즉시 처리한다.
- 세션 안 셸 상태는 신뢰성 있게 캡처한 standalone `cmd[.exe]`,
  `powershell[.exe]`, 대응하는 `exit` 전환만 지원한다. 다른 방식의 interactive
  shell 실행은 P0 밖이며 Familiar OFF 또는 올바른 셸로 새 세션을 시작해야 한다.
  Wingman이 모든 wrapper·alias를 탐지할 수 있다고 주장하지 않는다.
- 상태 표시줄 경로는 **마지막으로 확인한 파일 시스템 위치**로만 표시한다.
  live provider path라고 과장하지 않는다. PowerShell non-filesystem 위치는
  그대로 표시하고 P0 파일 명령은 provider guard로 실패시킨다.

## 수정된 목표 흐름

```text
xterm 입력
  -> 신뢰 가능한 제출·세션 tracker
     -> 불확실 또는 Familiar OFF: 원문과 Enter를 활성 셸로 전달
     -> 신뢰 가능한 한 줄: Rust prepare_submission(session_id, raw_line)
          -> PassThrough: 원문과 Enter 전달
          -> InvokePrepared: plan/diagnostic을 Rust 세션 메모리에 보관
               -> 보이는 shell edit buffer를 고정 호출로 교체
               -> shell이 일회용 요청 ID로 wingman-runner 호출
               -> runner가 세션 broker에 연결
               -> broker가 PreparedRequestV1을 원자적으로 한 번 소비
               -> runner가 다시 검증하고 실행하거나 거부 진단 출력
               -> stdout/stderr/exit code가 네이티브 shell PTY로 반환
```

프론트엔드는 경로가 들어간 실행 계획을 받거나 P0 옵션을 parsing하거나 명령별
shell 문자열을 만들지 않는다. Shell별 코드는 신뢰 가능한 입력 교체, 정확한 셸
전환, PowerShell 파일 시스템 provider guard, 고정 runner 전달로 제한한다.

## 수정된 구현 순서

필수 계약 수정, 마지막 문서 검토, 명시적인 승인 후에만 구현을 시작한다.

### 0단계: 계약 닫기

1. 데이터 모델을 요청 ID 전달과 일치시킨다.
2. 공통 Windows path/filesystem 규칙을 추가한다.
3. 터미널 제출·세션과 paste/history 규칙을 추가한다.
4. text stream/pipeline과 명령 세부 의미를 완성한다.
5. 보안, 성능, CLI, README 마이그레이션, 합격 test를 맞춘다.

### 1단계: 경계 기술 검증

1. Runner를 package·launch하고 CLI/GUI process topology를 결정한다.
2. Named-pipe ACL, 일회 소비, 만료, restart, protocol 불일치를 증명한다.
3. 두 셸에서 cwd, environment, PATH, token, exit code, non-filesystem provider를
   증명한다.
4. 신뢰 가능한 Unicode 입력 교체, 화면 원문 history, 내부 호출 echo, paste
   정책, Ctrl+C를 증명한다.
5. 첫 release-build startup, memory, input, runner 전달 성능 기준을 기록한다.

경계 기술 검증이 실패하면 P0 명령을 구현하지 않고 계약으로 돌아간다.

### 2단계: 공통 pure core

파괴적 작업을 연결하지 않은 상태에서 path type, lexer, parser, classifier,
command catalog, typed plan, diagnostic, protocol validation, 자원 제한, 준비 요청
저장소를 구현하고 test한다.

### 3단계: Runner engine

Broker client, 방어적 재검증, text decoding, streaming stage, pipeline
short-circuit, 치명 상태 우선순위, stdout/stderr, redirection, backpressure,
cancellation, 결정적인 test stage를 구현한다.

### 4단계: Read-only 명령

`pwd`, `which`, `ls`/`ll`, `clear`, `cat`, `head`, `tail`, `wc`, `grep`,
`find`, `sort`, `uniq`를 정확한 계약·shell transport test와 함께 구현한다.

### 5단계: 파일 시스템 변경

`mkdir`, `touch`, 그다음 `cp`, `mv`를 구현한다. Path, file identity,
reparse point, root, ancestor, race test가 통과한 뒤에만 `rm`을 연결한다.

### 6단계: 통제된 전환

Legacy와 common-v1 비교는 내부 개발 flag에서만 실행한다. P0 matrix 통과 후
프론트엔드 cmd mapping, 명령별 PowerShell 호환 함수, 쓰기 가능한 임시 profile,
임시 flag를 제거한다. 오래된 실행 test는 historical evidence를 target acceptance로
바꾸지 않고 retire한다. 같은 cutover에서 README와 지원 약속을 갱신한다.

### 7단계: 배포 강화

CSP와 최종 Tauri 권한 최소화, signed packaging, update·uninstall 확인,
성능·지속 실행 게이트, 현재·직전 Windows release canary, 최종 수동
UI/PTY/security smoke test를 적용한다.

## 합격 게이트

다음을 모두 충족해야 P0 준비가 끝난다.

```text
[x] C1-C11 계약 해결과 영문·한글 문서 일치
[x] cmd 경계 검증 완료, P0를 검증된 네이티브 통과로 축소
[ ] PowerShell 5.1 경계 matrix 통과
[ ] pure, runner, filesystem, pipeline, transport, native-preservation suite 통과
[ ] path, reparse, 파괴 동작, WebView, broker, paste, 관리자 권한 보안 test 통과
[ ] 기준 장비에서 성능 배포 상한 통과
[ ] README, 지원 matrix, installer, 실제 동작 일치
[x] legacy 호환 parser와 쓰기 가능한 임시 profile 제거
[ ] 최종 통합 검토 제시
[ ] 사용자가 구현 시작과 이후 P0 합격을 각각 명시적으로 승인
```

## 세 가지 수정의 조사 근거

- PowerShell 작업 위치는 process current directory와 같지 않다. 따라서
  non-filesystem provider guard와 상속 cwd 검증이 필요하다:
  [Microsoft about_Locations](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_locations).
- PSReadLine 기본 history 방식은 `SaveIncrementally`이므로 네이티브 동작을
  바꾸지 않으면서 모든 shell history가 메모리에만 있다고 약속할 수 없다:
  [Microsoft Set-PSReadLineOption](https://learn.microsoft.com/en-us/powershell/module/PSReadline/set-psreadlineoption?view=powershell-5.1).
- Windows는 fully qualified, root-relative, drive-relative, UNC, device
  namespace 경로를 서로 다르게 처리한다. 명령마다 문자열 검사를 만들지 않고
  중앙 경로 계약이 필요하다:
  [Microsoft Windows 파일·경로·namespace](https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file).

## 현재 게이트 상태

처음부터 끝까지 통합 재검토, C1-C10 수정, 마지막 문서 일관성 검토를 2026-08-06에
끝냈다. 프로젝트 Markdown 58개의 local link, fence, trailing whitespace를 검사해
문제가 없었고 모든 target 영문·한글 pair의 heading·fence 수도 일치했다. 이제
사용자의 통합 계획 재검토만 남았다. 명시적 승인 전까지 제품 코드 구현, 호환성
refactor, 경계 기술 검증 code, 동작을 바꾸는 test는 허가되지 않았다.
