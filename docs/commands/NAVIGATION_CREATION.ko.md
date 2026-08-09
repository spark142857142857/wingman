# 경로 탐색·생성 계약 (P0)

이 문서는 경로를 확인하고 파일·폴더를 만드는 Wingman의 작은 호환 범위를 정한다. GNU Coreutils 전체나 Unix 권한 모델을 재현한다는 약속은 아니다.

모든 경로 operand와 쓰기 안전 검사는 공통
[Windows 경로 계약](../WINDOWS_PATH_CONTRACT.ko.md)을 따른다.
변경 명령은 공통 [mutation 실행 계약](../MUTATION_EXECUTION_CONTRACT.ko.md)도 따른다.
생성한 `ls`, `pwd`, `which` record와 최종 encoding은 공통
[텍스트 record·stream 계약](../TEXT_STREAM_MODEL.ko.md)을 따른다.

## `ls`와 `ll`

```text
ls [-a] [-l] [-h] [PATH]
ll [PATH]
```

- `ll`은 정확히 `ls -l`의 별칭이다.
- 짧은 옵션 결합을 허용한다. `ls -lah`는 유효하다.
- `PATH`가 없으면 현재 폴더를 본다. 폴더를 주면 바로 아래 항목을, 파일을 주면 그 파일 자신을 출력한다.
- 기본 출력은 파이프에 쓰기 좋은 raw basename 한 줄씩이다. Basename의
  case-insensitive Unicode ordinal 순서와 case-sensitive ordinal tiebreaker를 쓴다.
- `-a`는 Windows `Hidden` 또는 `System` 속성이 있는 항목도 포함한다. `.`으로 시작한다고 숨김 파일이 되는 규칙은 없다.
- `-l`은 정확히 `TYPE ATTRS SIZE MODIFIED NAME`을 출력한다. `TYPE`은 directory
  `d`, regular file `-`, reparse point `l`, 그 밖의 항목 `?`다. `ATTRS`는 고정된
  다섯 글자 `RASHC` mask(ReadOnly, Archive, System, Hidden, Compressed)이며 없는
  속성 자리는 `-`다.
- `SIZE`는 regular file의 부호 없는 10진 byte 수이며 그 밖에는 `-`다. `MODIFIED`는
  last-write time을 whole second 아래로 자르고 활성 Windows time zone에서
  `YYYY-MM-DDTHH:MM:SS±HH:MM`으로 표시한다. `NAME`은 raw basename이며 space를
  quote하지 않는다. Unix 사용자·그룹·inode·`rwx` 권한을 뜻하지 않는다.
- `-h`는 `-l`과 함께만 가능하다. `0B`부터 `1023B`는 그대로 두고 그보다 큰 regular
  size는 `KiB`, `MiB`, `GiB`, `TiB`, `PiB`, `EiB`와 소수 한 자리로 표시한다.
  Integer arithmetic로 half-up rounding하며 `1024.0`으로 반올림되면 다음 unit으로
  올린다.
- 공통 경로 계약이 허용하는 상대·드라이브 절대·UNC 경로에서는 계약이 허용하는
  범위 안에서 `/`와 `\` 구분자를 쓸 수 있다.

재귀, 와일드카드, 필터, 정렬 옵션, 표시 열 조정, 나머지 GNU `ls` 옵션은 P0 밖이다.

## `pwd`

```text
pwd
```

- 인자와 옵션을 받지 않는다.
- `C:\Users\user\ProjectAgent\wingman`처럼 현재 폴더의 Windows 절대 경로를 출력한다.
- POSIX 경로로 변환하지 않으며 GNU `pwd -L/-P`도 구현하지 않는다.

## `clear`

```text
clear
```

- 인자와 옵션을 받지 않는다.
- 현재 터미널 화면을 비운다. 스크롤백 보존 여부와 낮은 수준의 ANSI 커서 동작은 호환성 약속이 아니라 구현 세부 사항이다.

## `mkdir`

```text
mkdir [-p|--parents] PATH...
```

- 하나 이상의 명시적 비와일드카드 경로를 받는다.
- `-p` 없이 이미 있는 폴더를 지정하면 오류다.
- `-p`를 쓰면 없는 상위 폴더도 만들며, 이미 있는 폴더는 성공 no-op이다.
- 폴더가 있어야 할 자리에 기존 파일이 있으면 항상 오류다.
- Unix 권한 모드를 만들지 않는다. `-m`, 파이프 입력, 와일드카드 확장은 P0 밖이다.
- 모든 operand를 먼저 검증한 뒤 왼쪽부터 디렉터리를 만든다. 일반 실행 실패는
  이미 만든 디렉터리와 일부 `-p` 상위 경로를 남길 수 있으며, 이후 독립 operand도
  계속 시도하고 최종 상태는 `1`이다.

## `touch`

```text
touch FILE...
```

- 하나 이상의 명시적 비와일드카드 파일 경로를 받는다.
- 없는 대상은 빈 일반 파일로 만든다. 기존 일반 파일은 `LastWriteTime`을 현재 시각으로 갱신한다.
- 상위 폴더는 자동으로 만들지 않는다.
- 폴더를 대상으로 하는 것은 P0에서 오류다. `-a`, `-m`, `-d`, `-r` 같은 시각 선택 옵션은 지원하지 않는다.
- 파이프 입력과 와일드카드 확장은 P0 밖이다.
- 요청마다 UTC timestamp 하나를 잡아 모든 operand에 적용한다. 전체 사전 검증
  뒤 왼쪽부터 처리하며 일반 실행 실패는 앞선 갱신·생성을 되돌리지 않는다.
  이후 독립 operand도 계속 시도하고 최종 상태는 `1`이다.

## `which`

```text
which NAME
```

- Filename component 하나만 받는다. Separator, drive colon, wildcard, control
  character, 잘못된 Windows filename이 있으면 문법 종료 `2`다.
- Runner는 `PATH`와 `PATHEXT` snapshot을 쓴다. Search directory는 현재 파일시스템
  폴더, 그 뒤 `PATH` component 왼쪽 순서다. 빈 component는 현재 폴더, 상대
  component는 현재 폴더 기준이며 양끝 quote 한 쌍은 제거한다. Percent-variable
  text는 확장하지 않는다. 같은 resolved directory는 case-insensitive하게 건너뛴다.
- `PATHEXT`는 `;`로 나누고 leading dot을 붙인 유효 extension만 순서대로 남기며
  case-insensitive dedup한다. 없거나 유효 entry가 없으면 `.COM;.EXE;.BAT;.CMD`다.
- `NAME`에 extension이 있으면 그 이름만 찾되 extension이 `PATHEXT`에 있어야 한다.
  없으면 search directory마다 `PATHEXT` 순서로 extension을 붙인다. Match는 실제
  존재하는 non-directory file이어야 한다.
- 첫 match를 normalized absolute Windows path로 출력한다. 접근 불가·잘못된 search
  directory는 건너뛴다. Match가 없으면 result `1`, 결론을 낼 수 없게 하는 filesystem
  inspection 실패는 진단이 있는 operational `1`이다.
- Wingman 호환 명령, PowerShell alias/function, `cd` 같은 셸 내장 명령은 찾지 않는다.

## 오류와 조합

`ls` 출력은 지원되는 텍스트 파이프에 넣을 수 있다. `pwd`, `clear`, `mkdir`, `touch`, `which`는 P0에서 파이프 입력을 받지 않는다.

- 성공: 종료 코드 `0`
- 없는 경로, 접근 불가 경로, 파일 시스템 실패: 종료 코드 `1`
- 잘못된 문법, 미지원 옵션, 와일드카드 경로 등 P0에서 거부하는 형태: 종료 코드 `2`

각 명령은 Windows 파일 시스템과 ACL 동작을 그대로 따른다. 읽기 전용 속성, 잠금, 권한은 Windows가 적용하며 Wingman은 Unix 소유권이나 mode 의미를 흉내 내지 않는다.
