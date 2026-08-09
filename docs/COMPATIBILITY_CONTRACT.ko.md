# Wingman 호환성 계약 (v0.1)

상태: MVP를 위한 확정 제품 방향입니다.

영문 원본: [COMPATIBILITY_CONTRACT.md](COMPATIBILITY_CONTRACT.md)

## 제품의 역할

> **Windows 셸은 그대로, Unix 명령 습관도 그대로.**

Wingman은 Windows의 기본 셸 위에서 동작하는, 세션 한정 Unix 명령 친숙성
계층입니다. PowerShell 또는 `cmd`를 프로세스 실행, 파일 시스템, 권한, 명령
실행의 기반으로 유지합니다. Linux 배포판, Bash, POSIX 런타임을 제공하는
제품은 아닙니다.

대상 사용자는 Unix 명령 습관은 있지만, 실제 작업은 Windows 환경에서 해야
하며 셸 전환 때문에 흐름이 끊기는 것을 원하지 않는 사람입니다.

## 입력 처리 약속

Familiar 모드를 켜면 다음 규칙으로 동작합니다.

1. 지원하는 Unix 문법은 현재 셸에 맞는 동작으로 변환합니다.
2. PowerShell, `cmd`, Windows 관리 명령은 수정하지 않고 그대로 전달합니다.
3. 지원 범위를 벗어난 Unix 문법은 추측하거나 일부만 변환하지 않습니다.
   Wingman이 아직 명령을 가로채지 않았다면 현재 셸이 처리하고, 이미
   Wingman이 해석을 시작한 명령이라면 지원하지 않는 문법이라는 오류를
   반환합니다.

이 규칙은 검증된 셸 prompt에서 신뢰성 있게 mirror한 한 줄에만 적용한다.
Completion, 여러 줄 paste, foreground interactive 입력, 알 수 없는 편집은
[터미널 제출·세션 계약](TERMINAL_SESSION_CONTRACT.ko.md)의 보수적인 fallback을
따른다.

Familiar 모드를 끄면 모든 명령 입력을 Wingman 해석 없이 현재 셸로 전달한다. 세션
격리와 줄바꿈이 있는 paste 확인은 계속 적용한다.

`find`, `sort`처럼 Windows와 Unix에 모두 있는 이름은 Familiar 모드에서만
Wingman의 Unix 호환 의미를 우선합니다. 원래 Windows 셸의 의미가 필요하면
모드를 끄면 됩니다.

### P0 셸 지원 범위

현재 production cutover는 Familiar 기본값을 `PAUSED`로 유지합니다. Windows
PowerShell 5.1에는 별도 검토한 OOB editor-readiness 채널이 연결됐고 PTY 출력에는
readiness 권한이 없습니다. `cmd.exe`는 계속 네이티브 통과입니다. 명령을 실제로
`familiar on`으로 켤 수 있는 현재 Rust-runner preview는 Familiar control, `pwd`,
소유한 미지원 `grep` option의 결정적 거부뿐입니다. 나머지 명령 활성화는
resource·shell transition gate를 더 통과해야 합니다. Catalog와 runner 기반이
있다는 사실만으로 아래 모든 Familiar 명령이 활성화됐다는 뜻은 아닙니다.

## P0: MVP에서 반드시 지원할 범위

| 작업 | 지원하는 Unix 입력 | 보장하는 동작 |
| --- | --- | --- |
| 목록·위치 | `ls`, `ll`, `ls -a`, `ls -l`, `pwd`, `clear` | 파일 목록, 현재 경로, 화면 정리 |
| 파일 읽기 | `cat FILE`, `cat -n FILE` | 텍스트 파일 내용 출력. [텍스트 스트림 명령 계약](commands/TEXT_STREAM.ko.md) 참고 |
| 파일·폴더 만들기 | `touch FILE`, `mkdir -p PATH` | 파일 생성 또는 수정 시각 갱신, 중첩 폴더 생성 |
| 복사·이동 | `cp SOURCE DEST`, `cp -r SOURCE DEST`, `mv SOURCE DEST` | 예측 가능한 덮어쓰기 규칙 아래 원본·대상 경로를 하나씩 처리. [cp·mv 명령 계약](commands/COPY_MOVE.ko.md) 참고 |
| 삭제 | `rm FILE`, `rm -r DIR`, `rm -rf DIR` | Wingman 안전 규칙 아래 명시적인 Windows 경로를 영구 삭제. [rm 명령 계약](commands/RM.ko.md) 참고 |
| 명령 찾기 | `which NAME` | 현재 Windows 환경에서 실행 가능한 명령의 경로 확인 |
| 텍스트 검색 | `grep [-i,-n,-v,-F,-r] PATTERN FILE` | 파일 또는 파이프 입력의 텍스트 검색. [grep 명령 계약](commands/GREP.ko.md) 참고 |
| 줄 단위 처리 | `head -n N`, `tail -n N`, `tail -f FILE`, `wc -l` | 파일 또는 지원되는 파이프 입력의 줄 처리. [텍스트 스트림 명령 계약](commands/TEXT_STREAM.ko.md) 참고 |
| 정렬·중복 제거 | `sort [-r,-n,-u]`, `uniq` | 기본 텍스트 정렬과 연속 중복 줄 처리. [sort·uniq 명령 계약](commands/SORT_UNIQ.ko.md) 참고 |
| 텍스트 조합 | `COMMAND \| COMMAND [\| COMMAND ...]`, `>`, `>>` | P0 명령을 파이프로 연결하고 결과 저장 |

### 범위를 제한한 `find`

`find`는 Unix 작업에서 자주 쓰이므로 P0에 포함합니다. 다만 처음에는 아래
문법만 지원합니다. 전체 규칙은 [find 명령 계약](commands/FIND.ko.md)을
참고합니다.

```text
find PATH [-type f|d] [-name PATTERN|-iname PATTERN] [-mindepth N] [-maxdepth N] [-print]
```

논리식, 권한 조건, `-exec`, 그 밖의 부수 효과는 P0 범위에 넣지 않습니다.

## P0 입력 문법

Wingman은 Bash라는 프로그래밍 언어 전체가 아니라, 자주 쓰는 한 줄 Unix 명령
문법의 작은 부분만 의도적으로 인식합니다.

```text
line     = pipeline [redirect]
pipeline = command ("|" command)*
command  = P0-command argument*
redirect = (">" | ">>") output-path
```

이 범위에서 P0는 공백으로 나눈 인자, 큰따옴표와 작은따옴표로 감싼 인자,
지원하는 명령 옵션, 옵션의 끝을 뜻하는 `--`, P0 명령끼리의 파이프, 마지막에
한 번 쓰는 `>` 또는 `>>` 리다이렉션을 허용합니다. 경로의 역슬래시는 일반
문자로 처리하므로 `src\main.ts` 같은 Windows 경로도 그대로 입력할 수
있습니다.

P0는 `grep TODO *.txt` 같은 셸 glob 확장, 환경 변수 확장, 명령 치환, `&&`와
`||` 같은 명령 체인, `;`, 입력·오류 스트림 리다이렉션, 그 밖의 Bash 제어
문법을 해석하지 않습니다. 단, `find -name "*.ts"`처럼 명령 인자로 직접
전달한 wildcard 패턴은 파일 목록 확장이 아니므로 허용합니다.

## P1: P0가 안정된 뒤 검토할 항목

- `cut`, `tr`, 제한된 `sed`
- `xargs`
- include/exclude 패턴·바이너리 파일 처리 등 고급 재귀 `grep` 조건
- `-size`, `-mtime` 등 추가 `find` 조건

이 기능들은 Familiar 해석을 활성화하는 모든 셸에서 예측 가능한 동작을 만들 수
있을 때에만 정식 호환 범위에 넣습니다. 셸 지원 범위는 명령 의미와 별도로
평가합니다.

## 의도적으로 지원하지 않는 영역

- Bash 스크립트 전체 문법, 명령 치환, 배열, 함수, job control
- Linux 권한·소유권: `chmod`, `chown`, `umask`
- Linux 프로세스·시그널 동작: `kill`, `nohup`, `jobs`, `fg`, `bg`
- Linux 특수 경로·장치: `/dev/null`, socket, FIFO
- Linux 패키지 관리자와 Linux 배포판
- 심볼릭 링크 호환: `ln -s`

이는 Windows 관리 기능을 막는다는 뜻이 아닙니다. `icacls`, `taskkill`,
`Stop-Process`, `Get-Acl`, `Set-Acl` 같은 Windows 명령은 일반 셸 입력으로
그대로 실행되며, 권한은 Wingman을 실행한 사용자 권한을 따릅니다.

## 안전성과 일관성 원칙

- 모든 P0 경로와 마지막 redirection은 공통
  [Windows 경로·파일 시스템 계약](WINDOWS_PATH_CONTRACT.ko.md)을 따른다.
- 모든 P0 text file, 생성 record, pipeline, stdout sink는 공통
  [텍스트 record·stream 계약](TEXT_STREAM_MODEL.ko.md)을 따른다.
- 지원하지 않는 옵션을 추측하거나 의미를 조용히 바꾸지 않습니다.
- Linux 파일 시스템, 접근 제어, 시그널, 마운트 동작을 제공한다고 주장하지
  않습니다.
- P0 범위를 벗어난 glob 확장이나 복잡한 리다이렉션은 보장하지 않습니다.
- 위험하거나 예상 밖의 변환보다, 예측 가능한 실패를 우선합니다.
- Windows PowerShell의 Familiar 명령은 패키지 adapter를 사용합니다. P0의 `cmd`
  입력은 네이티브 원문 통과이며, interception을 사용할 수 없을 때 두 셸 모두
  원문 입력을 정확히 보존해야 합니다.

## MVP 확인 예시

아래 명령은 Windows PowerShell 5.1에서 Familiar 모드를 켰을 때 예측 가능하게
동작해야 합니다. `cmd`에서는 같은 text를 바꾸지 않고 네이티브 셸로 전달해야
합니다.

```text
ls -a
pwd
cat README.md | grep Wingman | head -n 5
grep -in TODO src\main.ts
find src -type f -name "*.ts" | wc -l
mkdir -p temp\a\b
rm -rf temp
```

## P0 세부 계약

- [경로 탐색·생성 계약](commands/NAVIGATION_CREATION.ko.md)

## 네이티브 셸 상태 명령

Wingman은 `cd`, `chdir`, `pushd`, `popd`, PowerShell의 `Set-Location`을 변환하지 않는다. 공통 폴더 이동 방식은 이미 익숙하고, 네이티브 셸에는 `cmd`의 드라이브 처리나 PowerShell provider처럼 유용한 추가 기능이 있기 때문이다. 두 모드 모두 원래 셸로 그대로 전달한다.

Wingman의 파일 중심 명령은 이렇게 변경된 현재 파일 시스템 폴더를 기준으로 동작한다. PowerShell이 파일 시스템이 아닌 위치에 있으면 Linux식 경로를 억지로 만들지 않고, 명확한 오류를 낸다.
