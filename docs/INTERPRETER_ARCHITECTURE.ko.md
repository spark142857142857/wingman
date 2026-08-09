# 공통 해석기 구조 (초안)

상태: 합의된 구조 방향. 이 문서만으로 구현을 시작하지는 않는다.

## 원칙

> 셸은 상태를 소유하고, Wingman은 합의한 Unix 명령의 의미를 소유한다.

PowerShell과 `cmd`는 계속 활성 네이티브 셸이다. 현재 위치, 환경 변수, 세션 기능,
네이티브 명령 동작은 셸이 소유한다. Wingman은 이 저장소에서 합의한 제한된 P0 호환 문법과
명령 계약만 소유한다.

## 판정 흐름

```text
제출된 입력 한 줄
  -> Familiar 모드가 꺼짐?             원문 그대로 전달
  -> 네이티브 상태 명령인가?            원문 그대로 전달
  -> P0 Unix 명령 후보로 인식되는가?    파싱·검증
       -> 유효                         공통 실행 계획 실행
       -> 인식했지만 문법이 잘못됨      Wingman 진단 출력
  -> 그 외                            원문 그대로 전달
```

네이티브 상태 명령에는 `cd`, `chdir`, `pushd`, `popd`, PowerShell의
`Set-Location`이 포함된다. Wingman은 이를 변환하지 않는다.

## 계층

1. **입력 판정기**: 원문 전달, Wingman 진단, 공통 해석 중 하나를 고른다. 신뢰성 있게
   캡처되어 제출된 한 줄에만 작동한다.
2. **제한된 lexer·parser**: P0 한 줄 문법만 받는다. 단어, 작은따옴표·큰따옴표, `--`,
   `|`, 마지막 하나의 `>` 또는 `>>`가 대상이다.
3. **명령 카탈로그·검증기**: 일반 파싱 결과를 계약에 맞는 명령으로 바꾼다. 미지원 옵션과
   위험 요청을 거부하고, 문서의 종료 코드를 정한다.
4. **공통 실행 계획**: 요청한 작업을 셸과 무관하게 모호함 없이 표현한다.
5. **Wingman runner**: Windows 파일 시스템·프로세스·ACL 의미로 계획을 실행하고,
   제한된 pipeline으로 구조화된 text record를 전달하며 출력·진단·종료 코드를
   처리한다.

parser는 Bash 문법을 구현하지 않는다. 명령 치환, 환경 변수 확장, glob 확장, `&&`, `||`,
`;`, 입력·오류 리다이렉션 등 P0 밖 문법은 대상이 아니다.

## 셸 경계

Rust가 prompt·세션 tracker를 소유한다. 검증된 prompt와 allowlist에 든 mirrored
편집 sequence가 함께 있을 때만 신뢰 가능한 제출 줄이 된다. 그 뒤 프론트엔드는
네이티브 Enter와 준비 요청 ID 중 하나를 고른다. 명령이나 foreground program이
실행 중이면 모든 터미널 입력을 통과시킨다. 정확한 상태와 fallback은
[터미널 제출·세션 계약](TERMINAL_SESSION_CONTRACT.ko.md)을 따른다.

Rust는 모든 거부, control 응답, 실행 계획을 세션 메모리에 보관하며 WebView에
계획을 반환하지 않는다. Runner는 활성 셸의 자식 프로세스로 시작한다. 따라서
`cd` 같은 네이티브 상태 명령 뒤에도 실제 현재 파일 시스템 폴더, `PATH`, 환경,
접근 토큰을 물려받는다.

셸별 코드는 P0 옵션을 파싱하거나 독자적인 명령 의미를 정하면 안 된다. 검증된 runner 요청을
안전하게 전달만 한다. 이 전달 과정은 사용자 경로나 패턴을 셸 코드에 끼워 넣지 않고,
버전이 있는 불투명 요청 인코딩을 사용한다.

## 필수 일관성

- 같은 유효 P0 입력은 PowerShell과 `cmd`에서 비슷한 화면 결과와 같은 종료 코드를 낸다.
- Wingman이 인식한 P0 명령의 미지원 문법은 명확히 실패한다. 부분 변환이나 추측은 하지 않는다.
- 원래 네이티브 명령과 셸 상태 명령은 계속 쓸 수 있다.
- 화면과 프론트엔드가 관리하는 히스토리에는 내부 runner 호출이 아니라 사용자가 친 원문이 남는다.
- 취소, 스트리밍 출력, 리다이렉션, 오류는 runner 경계 테스트로 검증한다.

## 마이그레이션 목표

현재 초안에는 프론트엔드 `cmd` 변환과 PowerShell 호환 프로필이 따로 있다. 목표 구조에서는
이들이 독립적으로 P0를 파싱·실행하지 않고, 하나의 카탈로그·parser·실행 계획 형식·runner를
공유한다. 전환 중 셸별 shim은 전달 계층으로만 임시 허용한다.

판정·파싱·실행 계획·runner 요청의 경계는 [공통 해석기 데이터 구조](INTERPRETER_DATA_MODEL.ko.md)를 참고한다.
파싱 이전의 소유권 판정은 [입력 판정 계약](INPUT_CLASSIFICATION.ko.md)을 참고한다.
Prompt 증거, Unicode-safe 입력 mirror, completion fallback, paste, history, 셸 전환은
[터미널 제출·세션 계약](TERMINAL_SESSION_CONTRACT.ko.md)을 따른다.
[lexer 계약](LEXER_CONTRACT.ko.md)은 제한된 토큰 규칙을 정한다.
구현은 [구현 시작 게이트](IMPLEMENTATION_GATE.ko.md)를 따른다.
업데이트 검증과 지원 정책은 [호환성 유지보수 계약](COMPATIBILITY_MAINTENANCE.ko.md)에 정한다.
runner의 I/O, 취소, 종료 동작은 [runner 실행 계약](RUNNER_EXECUTION_CONTRACT.ko.md)에 정한다.
UTF-8 decoding, BOM·newline record, pipeline backpressure, short-circuit,
redirection sink, fatal 우선순위는
[텍스트 record·stream 계약](TEXT_STREAM_MODEL.ko.md)을 따른다.
권한, WebView, 전달, 터미널 데이터, 업데이트 경계는 [보안·신뢰 모델](SECURITY_MODEL.ko.md)에 정한다.
허용 경로 형태, runner 측 해석, identity, reparse 동작은
[Windows 경로·파일 시스템 계약](WINDOWS_PATH_CONTRACT.ko.md)에 정한다.
사용자가 느끼는 지연, 자원 상한, 비교 기준, renderer 교체 판단은
[성능 예산](PERFORMANCE_BUDGET.ko.md)에 정한다.
legacy에서 runner로 전환하는 순서는 [마이그레이션 계획](MIGRATION_PLAN.ko.md)에 정한다.
변경 요청의 사전 검증, 순서, staging, commit, 부분 결과 규칙은
[mutation 실행 계약](MUTATION_EXECUTION_CONTRACT.ko.md)에 정한다.
