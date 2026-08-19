# Lexer 계약

상태: 현재 제한된 P0 lexer 계약이며 릴리스 후보에 구현되어 있다.

## 토큰

```text
Token = Word(value) | Pipe | RedirectOverwrite | RedirectAppend
```

lexer는 단어 값과 파이프·마지막 출력 리다이렉션 연산자만 인식한다. 명령 의미는 나중에 카탈로그가 정한다.

## 상태

scanner의 상태는 `Normal`, `SingleQuoted`, `DoubleQuoted` 세 개뿐이다.

| 상태 | 공백 | `|` | `>` / `>>` | `\` |
| --- | --- | --- | --- | --- |
| Normal | 단어 구분 | 파이프 | 리다이렉션 | 일반 문자 |
| SingleQuoted | 일반 문자 | 일반 문자 | 일반 문자 | 일반 문자 |
| DoubleQuoted | 일반 문자 | 일반 문자 | 일반 문자 | 일반 문자 |

인용부호 밖의 ASCII 공백과 탭은 단어를 나눈다. 따옴표는 제거하며, 같은 단어 안에 붙어 있는
비인용·다른 종류 인용 조각과 내용을 이어 붙인다. 빈 따옴표 단어도 빈 인자로 보존한다.

백슬래시는 언제나 일반 문자다. Wingman은 이를 셸 이스케이프로 해석하지 않으므로
`C:\logs\app.log` 같은 Windows 경로가 바뀌지 않는다.

## 따옴표

작은따옴표와 큰따옴표는 모두 인자 하나를 묶는다. 둘 다 환경 변수, 백슬래시, 셸 표현식을 확장하지 않는다.
시작한 따옴표는 같은 종류로 닫아야 한다. P0에는 같은 종류의 따옴표를 그 안에 넣는 escape sequence가 없으며,
가능하면 다른 종류의 따옴표를 쓴다.

```text
grep 'fatal error' app.log  -> 패턴 인자 하나
grep "A|B" app.log          -> `|`는 패턴 글자
grep "unterminated          -> 거부
```

## 연산자와 구조

`|`, `>`, `>>`는 인용부호 밖에서만 연산자다. `>`와 `>>`의 앞뒤에 공백은 없어도 되지만,
리다이렉션은 정확히 하나여야 하고 마지막에 위치하며 출력 경로 단어 하나를 가져야 한다.

```text
grep TODO app.log>out.txt       -> 유효
grep TODO app.log >> "out file" -> 유효
grep TODO app.log > out | head  -> 거부
```

Wingman이 소유한 P0 한 줄에서는 인용부호 밖의 `&&`, `||`, `;`, `&`, `<`, backtick, `$(`,
`2>`, `&>` 같은 스트림 지정 리다이렉션을 지원하지 않는다. `$`, `%`, `^`, `\` 단독은 일반 단어 문자다.
Wingman은 셸 변수 확장을 하지 않는다. 괄호는 `$(` 일부일 때만 특별히 거부하며, 그 밖에는 일반 문자다.

## 한 줄 범위와 오류

P0는 제출된 한 줄만 받는다. 줄 이어쓰기와 기타 제어 문자는 범위 밖이며, 탭은 인용부호 밖에서 공백이다.

```text
LexError =
    UnclosedSingleQuote
  | UnclosedDoubleQuote
  | UnsupportedOperator
  | UnsupportedStreamRedirection
  | ControlCharacter

ParseError =
    EmptyPipelineStage
  | MissingRedirectTarget
  | MultipleRedirects
  | RedirectNotFinal
```

입력 판정기는 전체 P0 lexing 전에 최소한의 첫 명령 스캔을 한다. 따라서 첫 명령이 P0가 아닌
네이티브 입력은 뒷부분 문법이 P0 밖이어도 통과한다. 반면 첫 명령이 P0라서 Wingman이 소유한 줄은
항상 결정적인 lexer 또는 parser 진단을 받는다.
