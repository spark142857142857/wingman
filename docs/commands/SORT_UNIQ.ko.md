# `sort`와 `uniq` 명령 계약 (P0)

상태: MVP 확정 범위입니다.

영문 원본: [SORT_UNIQ.md](SORT_UNIQ.md)

구현 상태 (2026-08-09): `sort`와 `uniq`는 Reliable PowerShell Familiar 경로에
공개됐다.

모든 파일 operand는 공통
[Windows 경로 계약](../WINDOWS_PATH_CONTRACT.ko.md)을 따른다.
Text decoding, record termination, bounded materialization, 출력은 공통
[텍스트 record·stream 계약](../TEXT_STREAM_MODEL.ko.md)을 따른다.

## `sort`

```text
sort [OPTIONS] FILE
sort [OPTIONS] <pipeline input>
```

| 옵션 | 의미 |
| --- | --- |
| `-r`, `--reverse` | 최종 정렬 순서를 반전 |
| `-n`, `--numeric-sort` | 단순 10진수를 숫자값으로 정렬 |
| `-u`, `--unique` | 완전히 같은 줄을 하나만 남김 |

- 파일 하나 또는 파이프 입력 하나만 허용합니다.
- Materialization은 최대 262,144개 record와 64 MiB의 record text로 제한합니다.
  어느 상한이든 넘으면 sorted stdout 없이 종료 `1`입니다.
- 기본 비교는 전체 텍스트 줄의 대소문자를 구분한 Unicode ordinal 비교입니다.
  현재 셸의 locale 기본값에 따라 결과가 바뀌지 않습니다.
- `-n`은 parsing할 때만 ASCII space와 tab을 양끝에서 제거합니다. Trim 뒤 빈 줄은
  숫자 0이고, 그 밖의 줄은 `[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)`와 정확히
  일치해야 합니다. Exponent, `NaN`, infinity, comma, Unicode digit, 내부 whitespace는
  잘못된 runtime data입니다.
- 숫자 비교는 floating point가 아니라 정확한 sign·coefficient·scale 연산을 씁니다.
  앞뒤 0은 값을 바꾸지 않고 모든 zero spelling은 같습니다. 같은 숫자값은 `-r`
  아래에서도 입력 순서를 유지합니다. 잘못된 숫자 data는 부분 sorted stdout 없이
  `1`입니다.
- `-u`는 전체 줄을 대소문자 구분 기준으로 비교합니다.
- `-n -u`도 text가 완전히 같은 줄만 제거합니다. `1`, `1.0`, `+01`처럼 숫자값만
  같은 spelling은 각각 안정된 record로 남습니다.
- key·필드 구분자·human/version 숫자 모드·대소문자 무시·locale 제어·출력 파일·
  여러 파일 입력은 범위 밖입니다.

## `uniq`

```text
uniq [OPTIONS] FILE
uniq [OPTIONS] <pipeline input>
```

| 옵션 | 의미 |
| --- | --- |
| `-c`, `--count` | 각 출력 그룹 앞에 `COUNT LINE` 형식으로 개수를 붙임 |
| `-d`, `--repeated` | 인접해 두 번 이상 나온 그룹만 출력 |
| `-u`, `--unique` | 한 번만 나온 그룹만 출력 |

- 파일 하나 또는 파이프 입력 하나만 허용합니다.
- 기본 `uniq`는 연속해서 같은 줄이 나온 그룹마다 첫 줄 하나만 남깁니다. 떨어져
  있는 중복은 제거하지 않습니다.
- 비교는 전체 줄을 대소문자 구분 기준으로 합니다.
- `-c`는 `-d` 또는 `-u`와 조합할 수 있고, `-d`와 `-u`를 함께 쓰면 오류입니다.
- 대소문자 무시·필드/문자 건너뛰기·all-repeated 출력·출력 파일 인자·그 밖의
  옵션은 범위 밖입니다.

## 공통 규칙

- wildcard 경로와 파일 경로·파이프 입력의 동시 사용은 거부합니다.
- 파일·접근·decode·NUL·data·materialization 실패는 `1`, 잘못된 문법·입력 source
  형태는 `2`, 정상 완료는 `0`이다.

## 필수 확인 예시

```text
sort names.txt
grep ERROR app.log | sort
sort -r names.txt
sort -n numbers.txt
find src -type f | sort -u
sort names.txt | uniq
sort names.txt | uniq -c
sort names.txt | uniq -d
sort names.txt | uniq -u
```
