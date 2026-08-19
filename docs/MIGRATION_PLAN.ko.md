# 공통 해석기 마이그레이션 계획 (초안)

상태: 합의된 마이그레이션 방향. 이 계획은 구현을 허가하지 않는다.

## 범위

터미널 UI, xterm 연동, ConPTY·PTY 세션 관리, resize, Familiar UI 상태는 경계
기술 검증이 증명하는 범위에서 보존한다. 추측 기반 prompt·입력 추적과 미리 바꾸는
셸 stack 추적도 교체한다. 프론트엔드 `cmd` 문자열 변환, 명령별 PowerShell 함수,
셸별 옵션 parsing, 셸 명령 문자열 조립으로 나뉜 중복 호환 계층도 교체한다.

## 게이트

구현 전에 모든 제품·명령·parser·runner·유지보수·테스트 계약을 하나의 제안으로 재검토한다. 모순과 마이그레이션 위험을 해결하고 통합 계획을 제시한 뒤 구현 시작에 대한 명시적 승인을 받는다.

## 1단계: 위험 경계 기술 검증

명령 구현 전에 다음 위험 경계를 확인한다.

1. 패키지 out-of-band readiness가 root·동일 process 중첩 PowerShell 편집과
   네이티브 foreground child를 구분하고 `cmd`는 네이티브로 유지하는가
2. Unicode-safe mirror, completion·history fallback, 여러 줄 paste, `Ctrl+C`가
   터미널 세션 계약을 따르는가
3. 고정 runner 호출 교체가 prompt, 첫 출력, 다음 prompt를 망치지 않으며 짧은
   호출 표시와 네이티브 history 기록을 안전한 P0 fallback으로 유지하는가
4. 별도 `wingman-runner.exe`가 Tauri 설치본에 안정적으로 포함되고 실행되는가
5. 활성 셸의 현재 파일 시스템 폴더와 환경을 상속하는가
6. 요청 ID가 셸 문자열 보간 없이 검증된 계획을 전달하는가
7. Same-binary launcher·GUI handoff가 `cmd`와 PowerShell에서 orphan, console child,
   손실된 startup error 없이 CLI 실행 계약을 만족하는가
8. 첫 release build 성능 보정을 기록하고 승인한 P0 상한을 migration 중 반복해서
   느슨하게 만들지 않고 고정했는가

## 2단계: 공통 pure core

세션 증거, 입력 판정, lexer, parser, 카탈로그 검증, validated path value, 실행 계획·
protocol type, 진단, resource-limit constant를 하나의 Rust 구현으로 만든다. 이
단계에서는 실제 파일시스템 mutation이나 P0 명령 구현을 연결하지 않는다. TypeScript는 입력 event를 전달하고
터미널 세션 계약이 허용할 때만 Rust에 판정을 요청한다. Prompt reliability를
주장하거나 P0 옵션을 parsing하지 않는다. Runner는 나중에 같은 Rust 계획 타입을
방어적으로 검증한다.
Rust는 기준 원문 통과 또는 불투명한 일회용 요청 ID만 반환한다. TypeScript는
직렬화된 계획이나 준비된 진단을 받지 않는다.
공통 library가 `ValidatedPathSpec`도 소유한다. Runner만 상속한 셸 cwd에서 이를
해석하고 파일 시스템 identity를 구한다.
Process·filesystem 동작을 추가하기 전에 pure-contract와 protocol serialization
suite를 통과한다.

## 3단계: runner·broker·shell transport skeleton

전용 runner를 패키징하고 [runner 전달 계약](RUNNER_TRANSPORT.ko.md)의 local broker·
one-shot request ID를 구현한다. Test-only prepared operation으로 protocol validation,
cwd, environment, token, stdout·stderr·status, expiry, replay 거부, restart, cancellation을
증명한다. 실제 P0 명령은 아직 필요하지 않다.

명령별 PowerShell profile을 보호된 패키지 prompt integration과 파일 시스템·비파일
시스템 위치 전달, runner 호출, 종료 상태 보존만 하는 작은 shim으로 바꾼다.
`cmd`에도 동등한 prompt integration을 두고 요청만 전달한다. 어떤 셸 shim도 P0
옵션 의미를 소유하지 않는다.

Prompt, 편집, paste, recall, 전환 동작은
[터미널 제출·세션 계약](TERMINAL_SESSION_CONTRACT.ko.md)을 따른다.

## 4단계: runner stream·pipeline engine

실제 명령을 옮기기 전에 공통 `RecordFrame` UTF-8·BOM decoder, file·test source,
transform, bounded channel, backpressure, final sink, `>`·`>>` open order, same-file
거부, stage outcome priority, normal downstream stop, resource limit, Ctrl+C cancellation을
구현하고 시험한다.

Synthetic test stage와 disposable fixture로 pipeline, redirection, fatal·result status,
부분 출력, cancellation suite를 먼저 통과한다. 이 engine만 migration한 text 명령을
운반하며 raw-byte·shell-pipeline shortcut은 두지 않는다.

## 5단계: read-only·control 명령

1. `pwd`, `which`, `clear`
2. `cat`, `head`, `tail`, `wc`
3. `grep`
4. `sort`, `uniq`
5. `ls`, `find`

각 그룹은 다음 그룹 전에 이미 시험한 runner, pipeline, redirection, cancellation,
두 shell transport를 통해 정확한 명령 계약을 통과해야 한다.

## 6단계: 파일시스템 mutation

1. `mkdir`, `touch`
2. staged `cp`, `mv`
3. `rm`

모든 그룹은 [mutation 실행 계약](MUTATION_EXECUTION_CONTRACT.ko.md)을 따른다.
파괴적인 `rm`은 global preflight, path·identity, reparse, root·ancestor, staging,
부분 실패, cancellation, 통제된 race test가 통과한 뒤 마지막에 연결한다.

## 7단계: 통제된 전환

개발 중 내부 플래그로 legacy와 common-v1 경로를 잠시 비교할 수 있다. 영구 사용자용 이중 엔진은 아니다. 새 P0 매트릭스 통과 후 프론트엔드 `cmd` 변환, 명령별 PowerShell 호환 함수, 레거시 동작 테스트, 임시 플래그를 제거한다.

큰 변경의 대상은 호환 서브시스템이다. 안정적인 터미널·PTY 기반까지 불필요하게 다시 만드는 계획은 아니다.

애플리케이션 명령 등록과 실행 동작은 [CLI 실행 계약](CLI_LAUNCH_CONTRACT.ko.md)에 정한다.
