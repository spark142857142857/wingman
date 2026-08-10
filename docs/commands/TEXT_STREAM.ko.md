# `cat`, `head`, `tail`, `wc` 명령 계약 (P0)

상태: MVP 확정 범위입니다.

영문 원본: [TEXT_STREAM.md](TEXT_STREAM.md)

모든 파일 operand는 공통
[Windows 경로 계약](../WINDOWS_PATH_CONTRACT.ko.md)을 따른다.
모든 decoding, BOM, newline, final terminator, pipeline, 출력 동작은 공통
[텍스트 record·stream 계약](../TEXT_STREAM_MODEL.ko.md)을 따른다.

## `cat`

```text
cat [-n | --number] FILE...
```

- 하나 이상의 명시적인 텍스트 파일을 인자 순서대로 출력합니다.
- 파일마다 UTF-8·BOM 검증을 위해 따로 decode한 뒤 record framing 전에 이어
  붙인다. 따라서 앞 파일의 unterminated suffix는 다음 파일 prefix와 합쳐진다.
- `-n`, `--number`는 빈 줄을 포함한 모든 출력 줄에 번호를 붙이며, 여러 파일
  사이에서도 번호가 이어집니다.
- `cat`은 결과를 내보내는 시작 명령이며 파이프 입력을 받을 수 없습니다.
- Startup open 실패는 redirection target을 열기 전에 operand 순서로 모으며 stage는
  시작하지 않습니다. Streaming 중 read·decode 실패는 그 source를 fault 지점에서
  멈추고 operational `1`을 기록한 뒤, 취소나 downstream normal stop이 없으면 이후
  독립 operand를 계속합니다. 이미 출력한 text는 되돌리지 않습니다.
- 대화형 표준 입력 읽기, binary byte 의미, `-A`, `-b`, `-s` 같은 옵션은 범위
  밖입니다.

## `head`

```text
head [-n N] FILE
head [-n N] <pipeline input>
```

- 기본 개수는 10줄이고 `N`은 0 이상의 정수여야 합니다.
- 파일 하나 또는 파이프 입력 하나만 허용합니다.
- `head -n 0`은 출력 없이 성공합니다.
- byte 단위 개수, header, 옛 문법인 `-5`는 범위 밖입니다.

## `tail`

```text
tail [-n N] FILE
tail [-n N] <pipeline input>
tail [-n N] [-f | --follow] FILE
```

- 기본 개수는 10줄이고 `N`은 0 이상의 정수여야 합니다.
- 유한 모드는 최대 65,536개 record와 16 MiB의 record text만 보관합니다.
  어느 materialization 상한이든 넘으면 tail output 없이 종료 `1`입니다.
- `tail -n 0`은 명시한 input을 열지만 payload는 decode하지 않습니다.
- `-f`, `--follow`는 파일 하나가 필요합니다. Wingman은 현재 마지막 N줄을
  출력한 뒤 사용자가 `Ctrl+C`로 중지할 때까지 추가되는 줄을 출력합니다.
- Follow mode에서는 현재 unterminated suffix를 append한 LF가 끝낼 때까지
  pending으로 둔다. `Ctrl+C`는 그 fragment를 flush하지 않는다.
- 열린 파일이 이미 읽은 offset보다 작아진 것이 관찰되면 seek하거나 다시 열지
  않고 실행 실패를 보고한다.
- 파일 rotation 추적, `-F`, byte 단위 개수, 역순 출력, `+N` 문법은 범위
  밖입니다.

## `wc`

```text
wc -l FILE
wc -l <pipeline input>
```

- P0는 `-l`, `--lines`만 지원합니다.
- 파일 하나 또는 파이프 입력 하나만 허용합니다.
- 결과는 terminated 입력 record frame 수만 출력합니다. 마지막에 LF·CRLF가 없는
  비어 있지 않은 줄은 `wc -l` 의미와 같이 세지 않습니다.
- bare `wc`, 단어·byte·문자·최대 줄 길이, 여러 파일 합계는 범위 밖입니다.

## 공통 규칙

- wildcard 경로와 파일 경로·파이프 입력의 동시 사용은 거부합니다.
- P0는 텍스트 줄 단위 동작만 보장하며 binary 데이터·byte 정확 인코딩 동작은
  보장하지 않습니다.
- Invalid UTF-8, NUL, record·resource 상한, 파일 없음·접근 등 runtime 입력 실패는
  `1`, 잘못된 문법·입력 source 형태는 `2`, 정상 완료는 `0`, follow 취소는 `130`이다.

## 필수 확인 예시

```text
cat README.md
cat package.json tsconfig.json
cat -n app.log | grep ERROR
head -n 20 app.log
cat app.log | grep ERROR | head -n 10
tail -f server.log
grep ERROR app.log | tail -n 5
wc -l README.md
find src -type f -name "*.ts" | wc -l
```
