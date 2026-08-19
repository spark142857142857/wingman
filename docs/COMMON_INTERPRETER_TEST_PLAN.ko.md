# 공통 해석기 테스트 계획

상태: 현재 P0 수락 계획이다. 자동 suite는 구현됐으며 수동·외부 gate는 릴리스
매트릭스에 명시적으로 남아 있다.

## 테스트 케이스 형태

```text
TestCase {
  id, initial_file_tree, current_directory, familiar_mode, active_shell, input_line,
  expected_classification, expected_stdout, expected_stderr, expected_exit_code,
  expected_final_file_tree
}
```

테스트는 입력 판정, 스트림, 종료 상태, 최종 파일 시스템 상태를 함께 확인한다.

## A. 순수 계약 스위트

Windows 셸·PTY 의존성 없이 실행한다. 입력 소유권 판정, lexer·parser 구조, 명령 카탈로그 검증과 안전 검사, 실행 계획 생성, 결정적인 진단과 종료 동작을 다룬다.

네이티브 통과, P0 실행, Wingman이 소유한 미지원 문법, 따옴표, 리다이렉션 구조, 경로 없는 `.exe` 우회는 필수 사례다.

경계 검증은 PassThrough가 저장 요청을 만들지 않고 Reject, Execute, 예약 control이
프론트엔드에 예측 불가능한 ID와 display line만 반환함을 증명한다. 직렬화된 계획,
경로, 패턴, 진단 payload가 `FrontendDecisionV1`에 나타나면 안 된다. Session과
command-sequence envelope는 활성 prompt와 일치해야 하며 stale·불일치 판정은
무시해야 한다.

## B. runner 파일 시스템 스위트

검증된 일회용 Windows 임시 테스트 루트에서 runner를 실행한다. fixture에는 빈 파일, UTF-8·UTF-8 BOM 파일, 디코딩 불가 입력, 재귀 소스 트리, 숨김·읽기 전용 항목, 공백·한국어 경로, 가능한 경우 안전한 재분석 지점 사례를 넣는다.

허용·거부 경로 형태, hard-link identity, reparse, root, containment,
redirection alias, 통제된 race 전체 matrix는
[Windows 경로 계약](WINDOWS_PATH_CONTRACT.ko.md)을 따른다.

전체 사전 검증, 복수 operand 순서, staging·commit, 정리, 부분 결과, 취소, 종료 상태
집계 matrix는 [mutation 실행 계약](MUTATION_EXECUTION_CONTRACT.ko.md)에서 가져온다.
Test는 mutation 없는 알려진 안전 거부(`2`), mutation 없는 안전 증거 확인 실패(`1`),
일반 실행의 부분 작업(`1`), 취소된 부분 작업(`130`)을 구분해야 한다.

모든 P0 명령의 출력, 종료 상태, 최종 파일 트리를 확인한다. 삭제 테스트는 실행 전과 cleanup 중에 절대 대상이 항상 테스트 루트 내부임을 증명해야 한다.

명령 세부 fixture는 P0 `grep` regex·class grammar와 Unicode folding, 재귀 순서·표시
경로, `find` glob grammar·depth/type/reparse·경로 형식·preorder, `sort -n`의 정확한
decimal parsing·안정성, `ls -l/-h`의 모든 field·rounding 경계, `which`의
cwd/PATH/PATHEXT 순서, startup·runtime multi-source 실패를 고정한다.

## C. 파이프·리다이렉션·상태 스위트

Decoder, BOM, `RecordFrame`, newline, final terminator, transform, redirection
open, bounded channel, backpressure, short-circuit, `tail -f`, 부분 출력, 결과
우선순위 전체 matrix는 [텍스트 record·stream 계약](TEXT_STREAM_MODEL.ko.md)을
따른다.

지원 text pipeline, `>`·`>>`, stdout·stderr 분리, 마지막 stage 결과 상태, upstream
fatal, 결정적인 primary 진단, 정상 upstream stop, Ctrl+C 종료 `130`을 확인한다.

필수 사례는 `cat app.log | grep TODO | head -n 1`, `find src -type f -name "*.ts" | wc -l`, `grep NOTHING app.log`, `grep NOTHING app.log | head -n 5`, `cat missing.txt | head -n 5`, early `head` 전후 invalid UTF-8, 두 output redirection 방식, final unterminated 입력, bounded slow-consumer 흐름, `tail -f app.log` 취소다.

## D. 셸 통로 스위트

같은 P0 fixture를 `cmd`와 Windows PowerShell을 통해 실행한다. 현재 파일 시스템 폴더, `PATH`, UTF-8 상속, Familiar ON·OFF, 네이티브 명령 원문 통과, 취소 전달, PowerShell FileSystem 위치 허용, non-FileSystem provider guard를 확인한다.

전달 matrix는 준비된 Reject, Execute, Control variant를 broker가 한 번만 소비하고
stdout·stderr·종료 상태를 올바르게 전달하며 재사용·세션 불일치 ID를 거부함도 증명한다.

guard는 `Set-Location HKLM:\` 뒤 P0 파일 명령이 오래된 상속 폴더를 쓰지 않고 명확히 실패한다는 것을 보여야 한다.

Application launch matrix는 [CLI 실행 계약](CLI_LAUNCH_CONTRACT.ko.md)을 따른다.
공개 grammar, cwd·environment·token 상속, 같은 binary의 보호된 GUI role,
`Ready`/`Failed` acknowledge, 독립 child lifetime, timeout·Ctrl+C, orphan 없음,
internal mode 직접 호출 거부를 두 shell에서 시험한다.

## E. 네이티브 보존 스위트

Familiar ON에서는 네이티브 PowerShell cmdlet, cmd built-in, 셸 변수, 상태 명령이 원문 그대로 남는다. Familiar OFF에서는 P0처럼 보이지만 미지원인 문법을 포함한 모든 입력이 원문 통과한다.

## F. 터미널 제출·세션 스위트

정확한 자동화·integration·경계 기술 검증 matrix는
[터미널 제출·세션 계약](TERMINAL_SESSION_CONTRACT.ko.md)을 따른다. 검증된 prompt
marker와 세션 상태, foreground interactive 통과, Unicode·IME 편집, completion과
알 수 없는 편집 fallback, Wingman 소유 recall 없는 네이티브 history fallback,
한 줄·여러 줄 paste
확인, 인증된 동일 process PowerShell 중첩 depth 전환과 foreground child 네이티브
통과, 고정 호출 교체, 세션 재시작, `Ctrl+C`를
다룬다.

필수 negative case는 prompt처럼 보이는 출력, stale·malformed marker, Tab·history
search, 여러 줄 paste, foreground program 실행 중 입력이 `prepare_submission`에
도달하지 못함을 증명한다.

## G. 수동 애플리케이션 smoke 스위트

자동화에 적합하지 않은 UI·PTY 동작을 확인한다. 시작, 폰트, 포커스, 리사이즈, 편집, 원문 히스토리, 붙여넣기 신뢰성 fallback, `tail -f` 중 Ctrl+C, 셸 전환, 한국어 텍스트·경로, 리다이렉션 중 진단 표시, 세션 재시작 격리가 대상이다.

## 승인 게이트

아래가 모두 통과하기 전에는 P0가 완료된 것이 아니다.

```text
[ ] A-C 자동 스위트
[ ] D cmd·PowerShell 매트릭스
[ ] 지원 Windows 업데이트 카나리
[ ] E 네이티브 보존 회귀
[ ] F 터미널 제출·세션 matrix
[ ] G 수동 smoke 스위트
[ ] 성능 예산과 회귀 스위트
[ ] 문서와 관찰된 동작 일치
[ ] 구현 시작 게이트 재검토 완료
[ ] 사용자 구현·최종 승인
```

Wingman 계약이 약속한 출력만 정확히 비교한다. 지역화된 진단이나 원문 통과한 네이티브 명령의 동작을 고정하지 않는다.
