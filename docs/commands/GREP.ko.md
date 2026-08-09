# `grep` 명령 계약 (P0)

상태: MVP 확정 범위입니다.

영문 원본: [GREP.md](GREP.md)

모든 파일·폴더 operand는 공통
[Windows 경로 계약](../WINDOWS_PATH_CONTRACT.ko.md)을 따른다. Grep pattern은
별도 값이며 경로로 처리하지 않는다.
Text decoding, record termination, pipeline 전달, 최종 출력은 공통
[텍스트 record·stream 계약](../TEXT_STREAM_MODEL.ko.md)을 따른다.

## 지원 문법

```text
grep [OPTIONS] PATTERN FILE...
grep [OPTIONS] PATTERN <pipeline input>
```

지원하는 짧은 옵션과 긴 옵션은 다음과 같습니다.

| 옵션 | 의미 |
| --- | --- |
| `-i`, `--ignore-case` | 내용 검색에서 대소문자를 구분하지 않음 |
| `-n`, `--line-number` | 결과 앞에 줄 번호 표시 |
| `-v`, `--invert-match` | 일치하지 않는 줄을 대신 출력 |
| `-F`, `--fixed-strings` | 패턴을 정규식이 아닌 일반 문자열로 처리 |
| `-r`, `--recursive` | 명시한 폴더 아래의 파일을 재귀 검색 |
| `--` | 이후 인자를 옵션으로 해석하지 않음 |

짧은 옵션은 `grep -in TODO app.log`처럼 묶어서 쓸 수 있습니다.

## 입력과 출력 규칙

- 파일 경로와 파이프 입력은 동시에 쓸 수 없습니다. 둘 중 하나는 반드시
  제공해야 합니다.
- `-r`은 하나 이상의 명시적인 폴더 경로가 필요하며, 파이프 입력을 받을 수
  없습니다.
- 최상위 operand는 왼쪽부터 처리합니다. 재귀 검색은 depth-first이며 각 폴더에서
  basename의 case-insensitive Unicode ordinal 순서와 case-sensitive ordinal
  tiebreaker를 씁니다. Reparse point는 따라가지도 파일로 검색하지도 않습니다.
- 비재귀 파일 하나는 선택된 텍스트 줄만 출력합니다. 여러 파일 또는 재귀 검색이면
  `PATH:`를 앞에 붙입니다. `-n`은 `PATH:LINE:` 형식이며 파이프 입력에서는
  `LINE:`입니다. 줄 번호는 1부터이고 파일마다 다시 시작합니다. 파이프 입력은
  하나의 1-based count를 씁니다.
- 표시하는 `PATH`는 operand의 lexical normalized native form입니다. Separator는
  `\`가 되고 상대 operand는 상대 경로로, `.` 아래는 `.\`로, 절대 operand는
  절대 경로로 남습니다. 재귀 결과는 표시 root에 발견한 basename을 붙입니다.
- 일치하는 내용이 있으면 종료 코드 `0`, 없으면 result 상태 `1`이다. 접근 불가
  경로, read·decode 실패, NUL, resource 상한은 실행 상태 `1`, 잘못된 문법·pattern·
  입력 형태는 `2`다.
- Startup open, runtime read, 진단은 operand·traversal 순서를 씁니다. 실행 중
  read·decode 실패는 그 파일만 중단하고 이후 독립 파일은 계속 검색합니다.
  따라서 match 출력이 있어도 최종 실행 상태는 `1`일 수 있습니다.

## 패턴 규칙

기본 패턴은 P0의 이식성 있는 정규식 부분집합만 지원합니다.

```text
.  *  ^  $  []  \
```

`-F`를 쓰면 정규식 해석을 끕니다. `C:\temp\file.txt`처럼 정확한 문자열을
검색할 때 사용합니다.

Match는 한 logical record의 Unicode scalar value를 대상으로 하며 newline은 pattern
입력에 들어가지 않습니다. 정확한 grammar는 다음과 같습니다.

- 일반 scalar는 자기 자신, `.`은 scalar 하나와 match합니다.
- escape하지 않은 `^`는 pattern의 첫 token, `$`는 마지막 token일 때만 유효하며
  record 시작·끝을 고정합니다.
- `*`는 바로 앞 literal, `.`, bracket class를 0번 이상 반복합니다. 선두 `*`,
  반복된 `*`, anchor 뒤 `*`는 잘못된 pattern입니다.
- `[abc]`, `[a-z]`, `[^a-z]`를 지원합니다. Class에는 member가 하나 이상 있어야
  하고 range의 두 scalar endpoint는 ordinal 오름차순이어야 합니다. `-`는 첫째나
  마지막일 때만 literal, `^`는 첫째일 때만 negation입니다.
- `\`는 `.`, `*`, `^`, `$`, `[`, `]`, `\`, `-`만 escape합니다. 끝의 escape나
  다른 scalar escape는 잘못된 pattern입니다. Class 안에서는 `]`, `-`, `^`, `\`를
  escape할 수 있습니다.
- Group, alternation, `+`, `?`, brace, backreference, named class, locale collation은
  grammar가 아니며 pattern 종료 `2`입니다.

`-i`는 locale과 무관한 Unicode simple case folding을 scalar 비교에 적용하고
multi-scalar expansion은 하지 않습니다. `-F`는 모든 pattern scalar를 literal로
보되 요청했다면 같은 `-i` 규칙을 씁니다. 빈 pattern은 유효하며 모든 record와
match합니다.

P0는 확장·Perl 정규식, 복수 패턴, 앞뒤 문맥 출력, 색상 출력, glob 확장,
include/exclude 필터, 바이너리 파일 처리를 지원하지 않습니다. 특히 아래
입력은 일부만 변환하지 않고 지원하지 않는 문법 오류를 내야 합니다.

```text
grep -E "foo|bar" app.log
grep -P "(?<=id=)\d+" app.log
grep -e foo -e bar app.log
grep -C 3 ERROR app.log
grep TODO *.txt
grep --include="*.ts" -r TODO src
```

`find -name "*.ts"`는 다릅니다. 이 wildcard는 셸이 파일 목록으로 확장하는
것이 아니라 `find` 명령으로 전달한 인자이므로, `find` 계약에서 지원합니다.

## 필수 확인 예시

```text
grep TODO app.log
grep -in TODO app.log
grep -F "C:\temp\file.txt" app.log
cat app.log | grep -n ERROR
grep -r "TODO" src
```
