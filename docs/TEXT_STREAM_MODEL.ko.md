# 텍스트 record·stream 계약

상태: 현재 P0 text-stream 계약이며 릴리스 후보에 구현되어 있다.

영문판: [TEXT_STREAM_MODEL.md](TEXT_STREAM_MODEL.md)

## 범위와 권위

이 문서는 파일 decoding, 논리 record, 내부 pipeline 전달, 마지막 newline 동작,
출력 encoding, redirection sink 순서, backpressure, downstream short-circuit, 치명
상태 전달, 부분 출력의 단일 P0 기준이다.

모든 P0 text producer·consumer는 이 모델을 쓴다. 명령별 raw-byte 우회, PowerShell
object stream, 네이티브 셸 pipe, 두 번째 line reader는 없다.

## 논리 record 모델

```text
RecordFrame {
  text: UnicodeString,
  terminated: bool
}

TextStream = ordered RecordFrame sequence
```

`text`에는 인식한 줄 끝 byte가 들어가지 않는다. File decoder에서는 `terminated`가
처음에 해당 source record가 LF 또는 CRLF로 끝났는지 기록한다. 논리
`TextStream` 안에서는 이 record 뒤에 논리 line boundary가 있다는 뜻이다.

Stream invariant는 다음과 같다.

- 마지막 record만 `terminated: false`일 수 있다.
- Producer·transform이 뒤 output record를 발견하면 pending 상태인 앞의 `false`
  record를 emission 전에 `true`로 승격한다.
- 빈 stream에는 termination 상태가 없다.

Record 하나의 lookahead면 충분하다. 이 규칙은 filtering·복수 source 출력 뒤에도
downstream `wc -l`과 최종 encoder를 일치시키면서 실제 마지막 출력 record의
terminator 유무를 보존한다.

## UTF-8 decoder

모든 명시적 text file은 하나의 streaming UTF-8 decoder를 사용한다.

- 임의 read 경계에서 나뉜 유효 UTF-8은 완전한 Unicode scalar가 될 때까지 보관한다.
- 잘못된 형식, overlong, surrogate, 범위 초과, EOF에서 미완성인 UTF-8은 종료 `1`의
  실행 입력 실패다.
- P0 파일 입력은 replacement character로 고치지 않는다. 진단은 source와 byte
  offset을 알리되 원문 byte를 복사해 넣지 않는다.
- NUL byte는 유효 UTF-8이지만 Wingman P0 text 의미 밖이다. 만나면 종료 `1`의 text
  입력 실패다.
- UTF-16, UTF-32, ANSI, OEM, locale code-page 파일은 P0 밖이다. 추측하거나
  transcoding하지 않는다.
- 다른 Unicode control 문자는 text data로 남긴다. 조용히 지우지 않고 terminal
  security model로 그 효과를 제한한다.

Downstream이 일찍 멈추면 읽지 않은 byte는 decode·검증하지 않는다. Stop 전에 발견한
malformed sequence는 계속 fatal이고, 읽지 않은 suffix의 malformed data는 P0가
검사하지 않았으므로 결과에 영향을 주지 않는다.

## BOM 정책

각 명시적 파일의 byte offset 0에서 UTF-8 BOM 하나(`EF BB BF`)를 encoding
signature로 허용하고 record framing 전에 제거한다. Text record가 아니므로 세거나,
match·sort·출력하지 않는다.

- 다른 위치의 U+FEFF 또는 같은 byte는 일반 text다.
- 복수 파일 명령은 파일마다 offset-zero BOM 검사를 한다.
- 내부 pipeline은 Unicode record를 전달하므로 BOM 개념이 없다.
- 터미널·redirection P0 출력에는 BOM을 추가하지 않는다.
- `>>`는 기존 파일에 두 번째 BOM을 넣지 않는다.

## Newline framing

P0는 LF와 CRLF를 줄 끝으로 인식한다. LF 바로 앞의 CR은 CRLF의 일부이며 `text`에서
제거한다. 단독 CR은 일반 text다.

```text
input bytes       records
empty             []
a                 [{ text: "a", terminated: false }]
a\n               [{ text: "a", terminated: true }]
a\r\nb            [{ "a", true }, { "b", false }]
\n                [{ text: "", terminated: true }]
a\n\n             [{ "a", true }, { "", true }]
```

끝 terminator 뒤에는 가짜 record를 만들지 않는다. 빈 줄은 자체 terminator가 만든
실제 empty record다. 줄 번호는 마지막 미종료 record를 포함해 출력 record마다
붙인다. `wc -l`만 예외로 `terminated`가 true인 입력 frame 수를 센다.

## 파일 source와 생성 source

`cat FILE...`은 UTF-8·BOM 검증을 위해 파일마다 따로 decode하지만 newline framing
전의 decoded character stream을 이어 붙인다. 따라서 앞 파일의 미종료 suffix와 다음
파일 prefix는 text 이어 붙이기 의미대로 하나가 된다. `cat -n`은 그 결과 record를
계속 번호 매긴다.

복수 파일·재귀 `grep`처럼 파일을 따로 검사하는 명령은 파일 경계를 넘어 record를
합치지 않는다. `PATH:LINE:` 같은 prefix는 `text`를 바꾼다. 앞 파일의
unterminated record 뒤에 나중 선택 결과가 있으면 앞 pending 결과를 terminated로
승격해 둘을 서로 다른 논리 record로 유지한다.

`ls`, `find`, `which`, Familiar control 응답, `wc -l` 같은 생성 record source는
모든 출력 record를 terminated로 표시한다. 진단은 stderr에 남고 P0 stdout pipeline에
들어가지 않는다.

## Transform 규칙

- Streaming selection·mapping 명령(`cat -n`, `grep`, `head`, non-follow `tail`)은
  나중 output frame 때문에 stream invariant에 따라 승격해야 하는 경우를 제외하고
  선택한 입력 frame의 `terminated` 값을 보존한다.
- `head -n 0`은 빈 stream을 내고 payload record를 읽지 않은 채 정상 upstream stop을
  요청한다.
- `wc -l`은 terminated 입력 frame만 세고 생성된 terminated count record 하나를 낸다.
- `sort`는 상한 안에서 입력을 materialize한다. 재정렬 뒤 논리적 마지막 record를
  제외한 모든 출력 record를 terminated로 만들고, 마지막 출력 record에는 입력
  stream 최종 record의 termination 상태를 준다.
- `sort -u`도 dedup 뒤 같은 최종 상태 규칙을 쓴다.
- `uniq`는 순서를 보존한다. 출력 group은 그 group 마지막 입력 frame의 termination
  상태를 쓴다. 뒤 group이 filter로 빠지면 그 상태가 최종 출력 newline을 정한다.
- 출력 record가 없는 명령은 입력 최종 termination 상태와 무관하게 빈 stream을 낸다.

명령은 자체 계약에 필요한 만큼만 buffering할 수 있다. Streaming 명령은 마지막
newline을 정하려고 전체 입력을 materialize하지 않으며 pending 출력 record 하나면
충분하다.

## 최종 encoding과 sink

내부 pipeline은 UTF-8 chunk가 아니라 invariant를 만족한 Unicode `RecordFrame`을
전달한다. 마지막 stdout sink만 record를 encoding한다.

- 정상 P0 stdout byte는 BOM 없는 UTF-8이다.
- `terminated: true`인 모든 frame에는 CRLF를 붙이고 마지막 `false` frame에는
  붙이지 않는다. 있을 수 없는 nonfinal `false` frame은 byte를 추측해 만들지 않고
  내부 pipeline 실패로 거부한다.
- 입력 newline byte style은 보존하지 않는다. LF와 CRLF 모두 마지막 터미널·파일
  sink에서 CRLF가 된다.
- 정상 동작에서 encoder는 불완전 Unicode scalar를 내지 않는다.
- Low-level write 실패는 redirected file에 일부 UTF-8 sequence를 포함한 byte
  prefix를 남길 수 있다. P0는 실패를 보고하지만 rollback을 약속할 수 없다.
- Stderr 진단은 크기가 제한된 UTF-8·CRLF를 쓰지만 `TextStream` record가 아니며
  stdout redirection을 따르지 않는다.

터미널 표시는 이후 정상 Windows pseudoconsole 경로를 지난다. P0 text를 locale
code page로 다시 이중 encoding하지 않는다.

## Redirection 준비와 open 순서

Runner는 pipeline task가 data를 내기 전에 다음을 끝낸다.

1. 전체 plan, 명령 grammar, 경로 shape, 안전 규칙, redirection·input identity 제한 검증
2. 시작 때 필요한 모든 명시적 regular-file 입력을 왼쪽부터 해석하고 open 시도
3. 최종 stdout sink 열기
4. stage task와 record 흐름 시작

명시적 입력 하나라도 열 수 없으면 이미 연 input handle을 모두 닫고 진단을 operand
순서로 유지하며 stage를 시작하지 않고 redirection target도 그대로 둔다. 출력
sink를 열 수 없으면 stage를 시작하지 않는다. Directory 순회와 data decoding은
sink를 연 뒤에도 실패할 수 있다.

`>`는 3단계에서 target을 만들거나 비운다. `>>`는 만들거나 기존 끝으로 이동한다.
Append는 기존 byte를 검사·transcode·복구하지 않는다. 추가 segment만 이 UTF-8·no-BOM
encoder를 따른다고 보장한다. 기존 파일이 final newline 없이 끝나도 첫 append byte
전에 숨은 separator를 넣지 않는다.

`head -n 0 FILE > out.txt`도 payload record를 읽지는 않지만 `FILE`을 먼저 열고
`out.txt`를 만들거나 비운다. 이후 실행 실패·취소는 비거나 일부만 쓴 target을 남길
수 있다. Atomic output replacement는 P0 약속이 아니다.

Multi-source 명령은 source를 왼쪽부터 소비한다. Runtime read·decode fault 뒤 `cat`과
비재귀 `grep`은 그 source를 멈추고 이후 독립 operand를 계속하며, 재귀 `grep`은
traversal 순서의 다음 파일을 계속한다. Operand·traversal 순서에서 첫 fault가 primary,
최종 상태는 `1`이고 이미 출력한 stdout은 남는다. 취소, fatal sink 실패, downstream
normal stop 뒤에는 새 source를 시작하지 않는다.

## 제한된 pipeline과 backpressure

인접 stage 사이는 크기가 제한된 record channel을 쓴다. Capacity와 byte 상한은 구현
재검토에서 고정하고 성능 계약으로 측정한다.

- P0 초기 구현에서 decoded record 하나의 상한은 인식된 LF byte를 제외한 source
  UTF-8 1 MiB다. 초과는 truncation이 아니라 operational input failure다. Release
  build 보정 뒤 상한을 낮출 수 있지만, 늘리려면 계약과 memory budget을 다시
  검토해야 한다.
- Downstream channel이 차면 producer는 memory를 늘리지 않고 기다린다.
- Pending-record encoder, decoder fragment, 개별 record 길이, stage 수, `tail` buffer,
  `sort` materialization에는 모두 명시적 상한이 있다.
- 유한 `tail`은 선할당하지 않는 ring을 사용하며 최대 65,536개 record와 16 MiB의
  보관 record text로 제한한다. 상한 실패 시 ring을 비우고 tail record를 출력하지 않는다.
- `sort` materialization은 최대 262,144개 record와 64 MiB의 record text로 제한한다.
  상한 실패는 materialized input을 비우고 sorted record를 출력하지 않는다.
- 입력 data·materialization 상한 초과는 종료 `1`의 실행 실패다. 자르거나 일부를
  재해석하지 않는다.
- Blocking read·write·traversal·wait·channel operation은 공통 cancellation token을
  확인한다.
- Streaming source는 downstream consumer가 느리다는 이유로 busy-poll하면 안 된다.

`sort`는 정렬 record를 내기 전에 상한 안에서 전체 입력을 검증·materialize한다.
따라서 numeric data·materialization 실패는 sorted stdout 일부를 내지 않는다.
다만 이미 연 `>` target은 비어 있을 수 있다.

## 정상 short-circuit

`head` 같은 stage는 prefix만 소비하고 완료할 수 있다. 필요한 record를 받은 뒤
upstream에 정상 stop signal을 보내고 들어오는 흐름만 닫는다.

- Upstream은 정상 stop을 보고 broken-pipe 진단 없이 끝난다.
- 정상 stop은 cancellation이 아니며 종료 `130`을 만들지 않는다.
- Stop 확인 뒤 읽지 않은 data는 작업 밖이며 decode·traversal·검증하지 않는다.
- Stop 확인 전에 이미 관찰한 실행 실패는 계속 fatal이며 downstream 성공보다 우선한다.
- 정상 stop 때문에 생긴 synthetic closed-channel 오류는 숨기되 실제 source·decoder·
  sink 오류는 숨기지 않는다.

따라서 `cat huge.log | head -n 1`은 첫 complete record 뒤 읽기를 멈출 수 있다. 그
record 전의 invalid UTF-8은 실패하고 읽지 않은 suffix의 오류는 실패시키지 않는다.

## 결과·진단 우선순위

```text
PreExecution = ValidationFailure(exit 2) | Ready

StageOutcome =
    Success(exit 0)
  | Result(exit 1)          # 예: grep 미일치
  | StoppedNormally
  | OperationalFailure(exit 1, diagnostic)
  | Cancelled(exit 130)
```

Runner는 다음 순서로 pipeline 상태 하나를 발표한다.

1. 실행 전 문법·안전·request·plan 실패는 `2`이며 stage를 시작하지 않는다.
2. 최종 완료 발표 전에 받은 사용자 취소는 `130`이다.
3. 실제 source·decoder·stage·redirection write·filesystem 실행 실패는 `1`이며 뒤
   stage 성공보다 우선한다.
4. 그 외에는 마지막 stage 결과 코드가 이긴다. Upstream `Result(1)`은 fatal이
   아니므로 성공한 마지막 stage를 덮지 않는다.
5. Downstream short-circuit가 만든 `StoppedNormally`에는 실패 상태가 없다.

실행 실패가 여러 개면 pipeline stage index가 가장 낮고 그다음 source operand 순서가
빠른 것을 primary 진단으로 결정한다. 추가 진단을 유지한다면 같은 크기 제한·안정
순서를 따른다. Shutdown 과정의 부수 오류가 primary 원인을 대체하지 않는다.

따라서 `grep NOTHING file`은 `1`, `grep NOTHING file | head -n 5`는 `0`일 수 있고,
`cat missing | head -n 5`는 `1`이다.

## `tail -f` record 동작

Follow mode도 record 기반으로 유지한다.

- 초기 snapshot과 append byte는 같은 streaming UTF-8 decoder를 쓴다.
- 현재 미종료 suffix는 LF가 record를 끝낼 때까지 보관한다. 나중에 append한 text는
  같은 pending record에 이어진다.
- `Ctrl+C`는 아직 미종료인 pending record를 flush하지 않는다.
- Rotation 추적과 byte-fragment streaming은 P0 밖이다.
- Decode, NUL, access, 명령 계약에서 정한 truncation, resource 실패는 실행 종료 `1`,
  사용자 취소는 `130`이다.

Non-follow `tail`과 다른 finite reader는 EOF의 최종 미종료 record를 출력한다. Follow
mode buffering은 파일이 계속 append되는 동안 한 논리 줄을 두 번 보이거나 가짜
record 경계를 만드는 일을 막는다.

## 부분 출력 계약

Streaming stage는 나중 실행 실패·취소 전에 complete record를 이미 출력했을 수 있다.
그 record는 화면이나 redirected target에 남는다. Wingman은 fatal 결과 뒤 성공을
출력하거나 터미널 출력을 rollback하지 않는다.

Record encoder는 의도적으로 record 절반을 내지 않지만 OS write 실패는 파일에 byte
prefix를 남길 수 있다. 진단은 출력이 일부일 수 있음을 알린다. Mutation 명령은 별도
파일 시스템 부분 작업 규칙을 따르며 이 계약은 stdout data만 다룬다.

## 필수 검증 matrix

테스트는 적어도 다음을 다룬다.

1. 모든 byte 경계에서 나눈 UTF-8 scalar, invalid·overlong·surrogate·범위 초과·EOF
   미완성·NUL·UTF-16 BOM·ANSI 입력
2. BOM 없음, 첫 UTF-8 BOM 하나, read 사이에서 나뉜 BOM, BOM-only 파일, 파일 중간
   U+FEFF, 복수 파일 operand마다 첫 BOM
3. empty, LF, CRLF, mixed LF/CRLF, lone CR, blank line, final terminated·unterminated
   입력과 이 계약의 예시
4. `cat` 복수 파일 경계 join, `cat -n`, `grep` 선택·prefix와 multi-source
   unterminated 승격, `head`, finite `tail`, `wc -l`, `sort`, `sort -u`, 모든
   `uniq` filter
5. 2·3단계 pipeline의 empty output과 final-newline 상태
6. `>`, `>>`, 기존 unterminated append target, BOM target, missing input, output-open
   실패, same-file 거부, disk·write 실패, 부분 출력
7. record channel capacity 1, 느린 consumer, 긴 record 제한, bounded sort, blocked 상태
   취소, busy polling 부재
8. `head -n 0`, early `head`, stop 경계 전후 invalid data, 숨긴 broken pipe,
   fatal-before-stop 우선순위
9. upstream result와 fatal failure, 마지막 stage 상태, 복수 fatal 순서, sink 실패,
   cancellation race, 종료 `0/1/2/130`
10. `tail -f` 초기 complete record, pending unterminated suffix, 나중 completion, split
    UTF-8 append, 느린 append, flush 없는 `Ctrl+C`

## 표준 참고 자료

RFC 3629는 유효 UTF-8 byte sequence를 정의하고 decoder가 잘못된 형태를 막아야
한다고 정한다:
[RFC 3629](https://www.rfc-editor.org/info/rfc3629/).

Unicode Consortium은 선택적인 UTF-8 BOM을 byte order 요구가 아닌 encoding
signature로 설명한다:
[Unicode BOM FAQ](https://unicode.org/faq/utf_bom.html).

Microsoft는 Windows pseudoconsole 통로가 UTF-8을 사용한다고 문서화한다:
[Pseudoconsoles](https://learn.microsoft.com/en-us/windows/console/pseudoconsoles).
