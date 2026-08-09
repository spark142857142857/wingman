# `find` 명령 계약 (P0)

상태: MVP 확정 범위입니다.

영문 원본: [FIND.md](FIND.md)

시작 operand는 공통 [Windows 경로 계약](../WINDOWS_PATH_CONTRACT.ko.md)을
따른다. `-name`, `-iname` 값은 경로가 아니라 명령 pattern이다.
생성한 path record와 최종 encoding은 공통
[텍스트 record·stream 계약](../TEXT_STREAM_MODEL.ko.md)을 따른다.

## 지원 문법

```text
find PATH
  [-type f|d]
  [-name PATTERN | -iname PATTERN]
  [-mindepth N]
  [-maxdepth N]
  [-print]
```

- `PATH`는 필수 시작 경로 하나입니다. Wingman이 몰래 `.`으로 대체하지
  않습니다.
- `-type f`는 일반 파일만, `-type d`는 폴더만 선택합니다.
- `-name`은 대소문자를 구분하고, `-iname`은 구분하지 않습니다.
- `-mindepth`, `-maxdepth`에는 0 이상의 정수가 필요합니다.
- `-print`는 기본 출력 동작을 명시적으로 쓴 형태로 허용합니다.
- 지원하는 조건은 순서와 관계없이 쓸 수 있지만, 각 조건은 한 번만 쓸 수
  있습니다. `-name`과 `-iname`은 함께 쓸 수 없습니다.

`PATTERN`은 전체 경로가 아니라 발견한 basename만 검사합니다. Unicode scalar
wildcard grammar로 `*`는 scalar 0개 이상, `?`는 하나를 match하며 bracket class는
P0 `grep` 계약과 같은 member·range·negation·escape 규칙을 씁니다. `\`는 `*`, `?`,
`[`, `]`, `\`, `-`, `^`만 escape할 수 있고 다른 escape나 끝의 escape는 잘못된
문법입니다. Pattern 안의 path separator와 `:`는 유효하지 않습니다. 빈 pattern은
유효합니다. `-iname`은 multi-scalar expansion 없는 locale-independent Unicode
simple case folding을 씁니다.

## 경로와 출력 규칙

- `.`과 상대 경로를 지원합니다. 상대 경로에는 `./src`, `.\src`처럼 `/` 또는
  `\`를 사용할 수 있습니다.
- 드라이브 절대 경로와 UNC 경로는 공통 경로 계약을 만족해야 합니다.
  `/home/user/project` 같은 Linux식 또는 루트 상대 입력은 변환하지 않고 거부합니다.
- 결과는 lexical normalized native path로 한 줄에 하나씩 출력합니다. Separator는
  `\`가 되고, 상대 start는 상대 경로로 남으며, `.`은 `.`, 그 아래는 `.\name`으로
  표시합니다. Drive-absolute·UNC start는 절대 경로로 남고 내부 `\\?\` namespace는
  표시하지 않습니다.
- 시작 경로도 depth `0`에서 검사합니다. 따라서 `-maxdepth 0`은 시작 경로를
  출력할 수 있고, `-mindepth 1`은 시작 경로를 제외합니다.
- 숨김 파일·폴더를 포함합니다. Reparse entry는 `-type`이 없으면 match할 수 있지만
  P0 `-type f|d`에서는 일반 파일도 폴더도 아니며 절대 따라가지 않습니다.
- 결과는 결정적 depth-first pre-order입니다. Start를 먼저 검사하고 각 폴더의
  child는 basename의 case-insensitive Unicode ordinal 순서와 case-sensitive ordinal
  tiebreaker로 방문합니다. Depth 제한은 descent만 막고 sibling 순서를 바꾸지 않습니다.

## 파이프와 종료 코드 규칙

`find`는 결과를 내보내는 시작 명령입니다. P0 파이프라인의 앞에 올 수는 있지만
앞 명령의 파이프 입력을 받을 수는 없습니다.

```text
find src -type f -name "*.ts" | wc -l
find . -type f | grep "test"
```

검색이 정상적으로 끝나면 결과가 없어도 종료 코드 `0`입니다. 시작 경로가 없거나
접근할 수 없으면 `1`, 잘못된 문법·지원하지 않는 조건·잘못된 depth 값이면 `2`로
끝납니다.

## 의도적으로 지원하지 않는 문법

```text
find . -size +10M
find . -mtime -7
find . -path "*/node_modules/*"
find . -regex ".*\\.ts"
find . -o -name "*.tsx"
find . -exec rm {} \;
find . -delete
```

P0는 추가 메타데이터 조건, 논리식, 정규식·전체 경로 조건, 부수 효과가 있는
동작을 지원하지 않습니다.

## 필수 확인 예시

```text
find . -type f -name "*.ts"
find src -iname "*test*" -type f
find . -mindepth 1 -maxdepth 2 -type d
find "C:\work\project" -type f
find src -type f -name "*.ts" | wc -l
```
