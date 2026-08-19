# 입력 판정 계약 (초안)

상태: 공통 해석기의 소유권 경계를 위한 합의된 설계 방향.

## 결과

```text
OwnershipDecision = PassThrough | Reject | Execute
```

이 판정기의 가장 중요한 안전 책임은 Wingman이 소유하면 안 되는 입력을 바꾸지 않는 것이다.

이는 Rust 내부의 의미상 소유권 결과이며 WebView에 직접 반환하지 않는다.
`PassThrough`는 `FrontendDecisionV1`의 pass-through variant가 되고, `Reject`와
`Execute`는 `PreparedRequestV1`으로 저장한 뒤 prepared-invocation variant가 된다.
두 variant 모두 문서화한 payload 외에는 활성 session·command-sequence envelope만
담는다. 예약 control도 상태 변경을 검증한 뒤 같은 준비 요청 경로를 사용한다.

## 판정 순서

```text
1. 신뢰 가능한 제출 한 줄인가?
   아니오 -> PassThrough
2. Wingman 제어 명령인가?
   예    -> 내부 처리
3. 네이티브 셸 상태 명령인가?
   예    -> PassThrough
4. 첫 명령이 경로 없이 쓴 P0 이름인가?
   아니오 -> PassThrough
5. P0 한 줄 문법이 유효한가?
   아니오 -> Reject (종료 2)
6. 모든 파이프 단계가 P0 명령인가?
   아니오 -> Reject (종료 2)
7. 명령 계약·입력원·안전 규칙이 유효한가?
   아니오 -> Reject (종료 2)
   예    -> Execute
```

## 신뢰 가능한 입력만 해석

Wingman은 [터미널 제출·세션 계약](TERMINAL_SESSION_CONTRACT.ko.md)의 편집
allowlist로 복원하고 검증된 네이티브 셸 prompt에서 제출한 줄만 해석한다. 알 수
없는 편집, 셸 completion, 여러 줄 paste, foreground program, 불확실한 셸 identity가
있으면 판정하지 않는다. 이때는 사용자의 네이티브 입력 동작만 전달하고 추측한 줄로
editor buffer를 교체하지 않는다.

## 예약 명령과 네이티브 명령

`familiar on`, `familiar off`, `familiar status`와 허용된 짧은 별칭은 제품 제어 명령으로 예약하며,
판정보다 먼저 내부에서 처리한다.

`cd`, `chdir`, `pushd`, `popd`, `Set-Location`, `exit`은 통과시킨다. 네이티브 셸과 해당 세션 상태
모델이 계속 소유한다.

## P0 소유권

Familiar 모드가 켜져 있을 때, 첫 번째 실제 명령 단어가 경로 없이 쓴 P0 이름이고 대소문자를
구분하지 않고 일치할 때만 Wingman이 입력 한 줄을 소유한다.

```text
grep TODO app.log       -> Wingman
GREP TODO app.log       -> Wingman
git status              -> 통과
grep.exe TODO app.log   -> 통과
.\grep.exe TODO app.log -> 통과
C:\tools\grep.exe TODO app.log -> 통과
```

명시적인 실행 파일 확장자나 경로는 네이티브 실행을 명시적으로 요청한 것이다. `find`, `sort`처럼
이름이 겹치는 명령은 Familiar ON에서 Wingman P0 의미를 쓴다. 원래 명령을 쓰려면 `.exe` 형태를
명시하거나 모드를 끈다.

## Wingman이 소유한 P0 문법

Wingman이 P0 명령을 소유한 뒤에는 계약에 따라 실행하거나 거부한다. 미지원 문법을 부분 변환하거나
셸에 넘기지 않는다.

P0는 단어, 작은따옴표·큰따옴표로 묶인 단어, `--`, 파이프 구분자, 마지막 하나의 `>` 또는 `>>`
출력 리다이렉션만 허용한다. 명령 체인, 입력·오류 리다이렉션, 명령 치환, 환경 변수 확장,
그 밖의 일반 셸 문법은 해석하지 않는다.

인용부호 밖의 `&&`, `||`, `;`, `&`, `<`, `2>`, 잘못된 위치의 리다이렉션은 Wingman이 소유한
P0 한 줄에서 거부한다. `>>`는 마지막 하나의 출력 리다이렉션일 때만 유효하다. `$HOME`은 Wingman의
환경 변수 확장이 아니라 문자 그대로의 P0 인자다.

P0 셸 문법은 와일드카드를 확장하지 않는다. `find -name "*.ts"`처럼 명령 계약이 자체 패턴 인자로
허용한 경우만 가능하고, 그 외에는 거부한다.

## 파이프

Wingman P0 파이프는 전체를 Wingman이 소유하는 파이프다. 모든 단계가 P0 명령이어야 하고,
카탈로그가 텍스트 입력·출력을 승인해야 한다.

```text
cat app.log | grep ERROR | head -n 10  -> Execute
find src -type f | wc -l               -> Execute
grep TODO app.txt | findstr TODO       -> Reject
git log | grep fix                     -> PassThrough
```

마지막 예는 Wingman 호환성을 약속하지 않는다. 네이티브 출력에서 Wingman 텍스트 처리로 잇는 기능은
P0이 아니라 가능한 미래 기능이다.

## 진단

거부된 입력은 가능한 한 소유한 명령과 미지원 요소를 이름으로 말하고 종료 코드 `2`를 반환한다.
의도적으로 네이티브 셸 문법을 쓴 경우 Familiar 모드를 끄라는 안내를 덧붙일 수 있다.

진단은 준비된 Reject에 저장한다. 프론트엔드는 요청 ID만 받고 runner가 활성 셸을
통해 진단을 출력한 뒤 `2`를 반환한다.

## 필수 판정 예시

| 입력 | 결과 |
| --- | --- |
| `git status` | PassThrough |
| `cd ..` | PassThrough |
| `find.exe /v "" file.txt` | PassThrough |
| `grep -in TODO app.txt` | Execute |
| `grep -z TODO app.txt` | Reject |
| `grep TODO *.txt` | Reject |
| `cat app.log | grep ERROR | head -n 3` | Execute |
| `cat app.log | powershell Get-Date` | Reject |
| `git log | grep fix` | PassThrough |
| `grep TODO app.txt && dir` | Reject |
| `familiar off` | 내부 제어 |
| Familiar OFF의 `grep TODO *.txt` | PassThrough |

## 현재 활성화 상태 (2026-08-19)

Production classifier는 모든 P0 이름에 이 ownership algorithm을 활성화한다. 대상은
`pwd`, `clear`, `which`, `ls`/`ll`, `find`, `cat`, `head`, 유한 `tail -n N`, 단일 파일
`tail -f`, `wc -l`, `grep`, `sort`, `uniq`, `mkdir`, `touch`, `cp`, `mv`, `rm`이다.
Reliable evidence와 Familiar ON을 요구하고, 공통 lexer/parser/catalog를 사용하며, 이미
소유한 잘못된 줄은 결정적인 rejection으로 준비한다. 명시적 executable 이름과
native-first pipeline은 native pass-through를 보존한다. PowerShell FileSystem/OOB editor
경로는 실제 ConPTY, session broker, packaged sidecar, Unicode 경로, redirection,
mutation 안전·자원 gate, 다음 readiness cycle까지 검증됐다. `cmd`는 신뢰할 수 있는
editor adapter가 없으므로 native pass-through를 유지한다.
