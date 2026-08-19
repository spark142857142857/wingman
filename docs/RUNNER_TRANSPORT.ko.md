# Runner 전달 계약

상태: 현재 P0 전달 계약이며 릴리스 후보에 구현되어 있다.

## 결정

로컬 세션 범위 named-pipe broker와 짧고 예측하기 어려운 요청 ID를 사용한다. 사용자 원문이나 직렬화한
계획을 셸 소스, 명령줄 인자, 요청별 환경 변수에 넣지 않는다. 보호된 일회용 요청 파일은 fallback이며 우선 방식은 아니다.

## 흐름

```text
신뢰 가능한 원문 입력
  -> Rust prepare_submission(session_id, command_sequence, raw_line)
  -> 의미상 PassThrough | Reject | Execute 또는 예약 Control
  -> PassThrough: 기준 원문 반환, 아무것도 저장하지 않음
  -> 그 외: PreparedRequestV1을 Rust 세션 메모리에 저장
  -> 일치하는 session·sequence envelope와 display line, 예측하기 어려운 요청 ID가
     든 InvokePrepared 반환
  -> 활성 셸이 그 ID로 wingman-runner 실행
  -> runner가 로컬 broker에 연결
  -> broker가 요청을 한 번 소비하고 제거
  -> broker가 로컬 pipe로 PreparedRequestV1 전달
  -> runner가 재검증 후 실행하거나 거부 진단·control 응답 출력
```

셸 소스에는 안전한 고정 runner 경로, 요청 ID, 고정된 위치 종류 flag만 나타난다.
사용자 경로, 패턴, 진단, control, 실행 계획은 broker pipe로 이동하기 전까지 Rust
안에 남는다. WebView 경계를 넘거나 명령줄 인자에 나타나지 않는다.

모든 판정은 활성 session과 command sequence에 묶이며 stale·불일치 결과는 버린다.
`PassThrough`는 요청 ID를 만들지 않는다. `Reject`, `Execute`, `Control`은 모두
같은 준비 요청 경로를 사용하므로 네이티브 셸이 runner의 stdout, stderr, 종료
상태를 일관되게 받는다.

## Broker 생명주기와 보안

- Wingman 세션마다 로컬 broker 하나를 둔다.
- pipe 이름에는 세션별 무작위 요소와 로컬 세션 범위를 넣는다.
- 현재 로그인 세션·사용자만 접근하게 하고 원격 접근을 거부한다.
- 요청 ID는 예측하기 어렵고 일회용이며 짧은 시간 뒤 만료한다.
- 셸 재시작이나 Wingman 종료 시 해당 세션의 대기 요청을 모두 무효화한다.
- 모르는 ID, 만료·재사용·프로토콜 불일치 요청은 실행하지 않는다.
- runner가 받은 protocol과 준비 요청 종류를 다시 검증한다. Execute 계획은 파일
  시스템 접근 전에 방어적으로 다시 검증한다.

## 구현된 방어적 재검증 (2026-08-09)

Runner 경계는 이제 알 수 없는 중첩 field를 거부하고 dispatch 전에 모든 typed
field를 다시 검증한다. Host와 runner가 공유하는 제한은 다음과 같다.

- 직렬화된 prepared request는 최대 64 KiB
- pipeline stage는 최대 16개, redirection을 포함한 전체 path operand는 최대 128개
- prepared diagnostic은 최대 4 KiB, control response는 최대 256 byte
- diagnostic/control text는 비어 있지 않아야 하며 terminal 제어문자를 포함할 수 없음
- Reject status는 `2`, Control status는 `0`으로 고정
- `head` count는 최대 `4,294,967,295`
- 직렬화된 모든 `ValidatedPathSpecV1`을 원문에서 정확히 재구성할 수 있어야 함
- catalog가 만들 수 있는 source/downstream stage 형태만 허용

Runner는 decode 직후와 직접 실행 entry point에서 각각 검증한다. 거부된 요청은
요청 내용을 출력하지 않고 고정된 bounded diagnostic 하나만 출력한다. 테스트만을
위해 존재하던 environment-probe 실행 variant는 제거했으며, 실제 working-directory
operation으로 process 경계 상속을 검증한다.

이는 request validation을 완성한다. Typed `cat`/`head`/유한 `tail -n N`/단일 파일 `tail -f`/`wc -l`/`grep` plan은 이제 production streaming
runner를 사용하고 typed `>`/`>>` plan도 같은 record stream을 safe prepared file sink로
보낸다. Reliable·Familiar ON PowerShell 입력은 이제 공통 lexer/parser/catalog로 `cat`,
`head`, 유한 `tail -n N`, 단일 파일 `tail -f`, `wc -l`, `grep`을 분류하고, 고정 editor replacement에는 opaque prepared request ID만 넣는다.
Sidecar의 공유 cancellation token과 Windows console control handler는 terminal·redirected
sink 모두에 적용되며, 실제 process-group test는 redirected streaming 중인 sidecar를
취소한다. PowerShell/ConPTY vertical test도 Unicode 경로 redirection과 다음 OOB readiness
cycle을 증명한다. `cmd`는 입증된 editor adapter가 생길 때까지 interception 밖에 둔다.

## 위치 메타데이터

`cmd`는 고정된 filesystem 위치 종류로 runner를 부른다. 최소 PowerShell 전달 shim은 provider 경로를
셸 소스에 넣지 않고 `filesystem` 또는 `non-filesystem`만 보고한다. runner는 계획을 받은 뒤 파일 시스템
요구 조건을 적용한다.

## 패키징

앱 실행 파일은 `wingman.exe`, 전용 sidecar는 `wingman-runner.exe`다. 둘은 같은 Cargo 패키지의 binary
target이므로 Tauri bundler가 runner를 앱 실행 파일 옆에 설치한다. `bundle.externalBin`에도 선언하면 같은
설치 파일이 중복되므로 그렇게 하지 않는다. 고정 PowerShell transport는 `wingman.exe`에 컴파일하고 앱 소유
`-Command` source로 전달하므로 쓰기 가능한 `.ps1` 지원 파일이나 process 전체 execution-policy 우회를
설치하지 않는다. Wingman은 앱이 통제하는 설치 runner의 절대 경로를 세션 환경 변수로 자식 셸에 전달한다.

## 화면과 히스토리

사용자의 원문 입력은 교체 전 mirrored 일관성 값의 기준이다. P0에는 Wingman 소유
프론트엔드 command history가 없으며 네이티브 셸 history에는 내부 runner 호출이
들어갈 수 있다. 네이티브 editor 내용, prompt 동기화, 정확한 교체 operation은
[터미널 제출·세션 계약](TERMINAL_SESSION_CONTRACT.ko.md)을 따른다.

호출 echo 숨김은 별도 경계 기술 검증이다. PowerShell에서 생성 echo만 정확히 숨기고, runner 첫 출력과 다음
prompt를 보존하며, Ctrl+C와 셸 line editing을 망치지 않는다고 증명할 때만 적용한다. 검증되지 않은 PTY 출력
필터보다는 짧은 내부 호출을 보이는 것이 안전한 fallback이다.
