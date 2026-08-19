# 공통 해석기 데이터 구조 (초안)

상태: 합의된 설계 방향. 구현 때 타입 이름은 바뀔 수 있지만, 소유권과 데이터 경계는 의도적으로 정한 것이다.

## 의미상 소유권 판정

```text
OwnershipDecision = PassThrough | Reject | Execute
```

- `PassThrough`는 원문이 활성 셸의 소유라는 뜻이다.
- `Reject`는 Wingman이 P0 형태의 입력을 소유했지만 문법·옵션·안전 형태가 계약
  밖이라는 뜻이다.
- `Execute`는 Wingman이 입력을 소유하고 완전히 검증된 셸 독립 실행 계획을
  만들었다는 뜻이다.

이는 WebView에 반환하는 값이 아니라 내부 의미 판정이다. 예약된 Familiar control은
이 소유권 판정보다 먼저 감지한다.

## 프론트엔드 판정

```text
FrontendDecisionV1 {
  session_id,
  command_sequence,
  decision:
      PassThrough     { raw_line }
    | InvokePrepared  { request_id, display_line }
}
```

- Envelope ID는 결과를 활성 prompt와 제출 하나에 묶는다. Stale 또는 불일치 결과는
  버리며 이후 editor buffer를 교체할 수 없다.
- `PassThrough`는 수정 없이 셸에 제출할 기준 원문을 반환하며 broker 항목을 만들지
  않는다.
- `Reject`, `Execute`, 인식한 Familiar control은 Rust 세션 메모리에 준비 요청으로
  저장한다. Rust의 직렬화된 dispatch만 짧고 예측 불가능한 `request_id`와 원래
  `display_line`을 보유하며 WebView는 둘 다 받지 않는다.
- `display_line`은 editor 교체 전에 쓰는 정확한 mirrored-line 일관성 값이다. P0는
  이를 Wingman 소유 command history로 보존하지 않는다.

[터미널 제출·세션 계약](TERMINAL_SESSION_CONTRACT.ko.md)은 이 판정의 선행
조건이다. WebView는 prompt나 줄의 reliability를 선언할 수 없다. Rust가 판정을
정확한 mirrored line과 sequence에 묶는다. `PassThrough`일 때 네이티브 editor가
이미 `raw_line`을 가지고 있으므로 프론트엔드는 이를 일관성 값으로만 다루고
Enter만 전달하며 text를 다시 보내지 않는다.

Session·sequence envelope와 일회용 ID 이외의 파싱된 명령, 진단 payload, 경로,
패턴, 실행 계획, 직렬화된 요청, broker endpoint, request secret은 Rust에서
WebView로 넘어가지 않는다.

## 파싱 모델

```text
ParsedLine {
  stages: ParsedCommand[],
  redirect: Redirect | null
}

ParsedCommand {
  name: string,
  arguments: string[]
}

Redirect {
  mode: Overwrite | Append,
  path: string
}
```

parser는 단어, 따옴표, 파이프 경계, 마지막 출력 리다이렉션만 기록한다. 명령의 의미를 정하거나,
glob·환경 변수를 확장하거나, 일반 셸 문법을 해석하지 않는다.

## 검증된 경로 모델

```text
ValidatedPathSpec {
  original: string,
  kind: Relative | DriveAbsolute | UncAbsolute,
  components: string[]
}

ResolvedPath {
  absolute_native: string,
  identity: FileIdentity | null
}
```

Host catalog는 [Windows 경로 계약](WINDOWS_PATH_CONTRACT.ko.md)에 따라 path
operand를 `ValidatedPathSpec`으로 만든다. 형태만 검증하며 host process 위치에서
상대 경로를 해석하지 않는다. Runner가 specification을 다시 검증하고 활성 셸의
파일 시스템 cwd를 상속한 뒤 작업별 검사 직전에 `ResolvedPath`를 만든다.
`FileIdentity`는 object가 존재하고 identity 검사가 필요할 때만 구한다.

패턴은 명령별 별도 값이며 `ValidatedPathSpec`이 아니다. `ResolvedPath`와
`FileIdentity`는 `ExecutionPlan`에 직렬화하거나 WebView로 반환하지 않는다.

```text
ValidatedRedirect {
  mode: Overwrite | Append,
  path: ValidatedPathSpec
}
```

Catalog가 parser의 raw `Redirect`를 이 검증 형태로 바꾼다.

## 검증된 실행 모델

```text
ExecutionPlan {
  stages: StagePlan[],
  redirect: ValidatedRedirect | null
}
```

`StagePlan`은 일반 명령 문자열이 아니라 명령별 tagged 타입이다. 대표 형태는 다음과 같다.

```text
ReadTextFiles { paths, number_lines }
HeadLines { count, path }
TailLines { count, path }
FollowFile { count, path }
CountLines { path }
ListDirectory { path, include_hidden, long_format, human_sizes }
SearchText {
  pattern, paths, source,
  ignore_case, line_numbers, invert, fixed_string, recursive
}
FindPaths { start_path, kind, name_pattern, case_mode, min_depth, max_depth }
RemovePaths { paths, recursive, force }
SortLines { reverse, numeric, unique, source }
UniqueLines { count, duplicates_only, unique_only, source }
```

명령 카탈로그는 runner가 보기 전에 모든 명령 계약의 옵션·입력원·안전·종료 코드 검사를 적용해 이 값을 만든다.
환경에 의존하는 경로·identity·reparse 검사는 runner가 담당한다.

## 파이프 호환성

Runtime text edge는 byte chunk나 shell object가 아니라 크기가 제한된
`RecordFrame { text, terminated }` 값을 전달한다. Decoding, framing, 최종 encoding,
backpressure, short-circuit, 결과 우선순위는
[텍스트 record·stream 계약](TEXT_STREAM_MODEL.ko.md)이 소유한다. 명령 metadata는
stage가 그 edge에 연결될 수 있는지만 정한다.

카탈로그 메타데이터에는 명령이 앞 단계의 텍스트를 받을 수 있는지, 다음 단계에 텍스트를 낼 수 있는지가 기록된다.

| 명령군 | 텍스트 입력 | 텍스트 출력 |
| --- | --- | --- |
| `cat`, `ls`, `find` | 불가 | 가능 |
| `grep`, `head`, `tail`, `wc`, `sort`, `uniq` | 계약별 | 가능 |
| `mkdir`, `touch`, `cp`, `mv`, `rm`, `clear`, `which` | 불가 | 대체로 불가 |

검증은 실행 전에 `rm temp | grep error`, `grep TODO app.txt | mkdir logs` 같은 불가능하거나 약속하지 않은 조합을 거부한다.

## 준비된 runner 요청

```text
PreparedRequestV1 {
  protocol: "wingman.run",
  version: 1,
  kind:
      Reject  { diagnostic, exit_code: 2 }
    | Execute { plan: ExecutionPlan }
    | Control { response, exit_code }
}
```

Rust는 이 값을 요청 ID 아래에 저장한다. 활성 셸에는 고정 runner 호출과 ID만
전달한다. Runner가 연결하면 broker가 항목을 원자적으로 한 번 소비하고 로컬 세션
pipe를 통해 `PreparedRequestV1`을 직렬화한다. 프론트엔드는 이 값을 전달하거나
직렬화하지 않는다.

Familiar control은 Rust host가 검증된 앱 상태 변경을 적용하고 셸에 보여줄 응답과
종료 상태만 준비한다. Reject는 runner가 준비된 진단을 출력하고 계획 실행 없이
`2`를 반환한다. Execute는 runner가 계획을 다시 검증한다.

요청에는 의도적으로 현재 작업 폴더 필드가 없다. runner는 활성 셸의 자식 프로세스로 시작해 실제 현재 파일 시스템 폴더, 환경, `PATH`, 접근 토큰을 상속한다.
