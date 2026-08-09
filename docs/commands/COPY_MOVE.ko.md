# `cp`와 `mv` 명령 계약 (P0)

상태: MVP 확정 범위입니다.

영문 원본: [COPY_MOVE.md](COPY_MOVE.md)

모든 source·destination·identity·containment·reparse 검사는 공통
[Windows 경로 계약](../WINDOWS_PATH_CONTRACT.ko.md)을 따른다.
전체 사전 검증, staging, commit, 부분 결과, 취소 동작은 공통
[mutation 실행 계약](../MUTATION_EXECUTION_CONTRACT.ko.md)을 따른다.

## 지원 문법

```text
cp [OPTIONS] SOURCE DESTINATION
mv [OPTIONS] SOURCE DESTINATION
```

| 명령 | 지원 옵션 |
| --- | --- |
| `cp` | `-r`, `-R`, `--recursive`, `-f`, `--force`, `-n`, `--no-clobber`, `--` |
| `mv` | `-f`, `--force`, `-n`, `--no-clobber`, `--` |

각 작업은 원본 하나와 대상 하나만 받습니다. 파이프 입력과 `*`, `?`가 든
wildcard 경로는 지원하지 않습니다. `cp`에서 폴더 원본은 재귀 옵션이 필요하고,
`mv`는 재귀 옵션 없이 파일과 폴더를 이동할 수 있습니다. `-f`와 `-n`은 함께
쓸 수 없습니다.

## 대상 경로 규칙

- 새 대상 경로는 새 파일 또는 폴더 이름이 됩니다.
- 대상이 기존 폴더라면 Wingman은 `DESTINATION\basename(SOURCE)`를 실제 대상
  경로로 사용합니다.
- 그 실제 대상이 이미 폴더라면 폴더 트리를 병합하지 않고 작업을 거부합니다.
- 부모 폴더가 없으면 오류입니다. 먼저 `mkdir -p`를 사용해야 합니다.

## 덮어쓰기와 플랫폼 규칙

- 기본 덮어쓰기는 같은 부모의 staging 항목에 복사하고 flush·재검사를
  마친 뒤 commit 시점에만 기존 대상 파일을 교체합니다.
- `-n`은 staging 전에 기존 대상을 건너뛰고 변경 없이 성공합니다.
- `-f`는 읽기 전용·숨김 대상 항목도 교체하려고 시도합니다.
- `-f`도 Windows ACL, 열린 파일, 암호화, 볼륨 제약을 우회하지 못합니다.
- 같은 볼륨의 `mv`는 직접 rename 또는 replace로 commit합니다. 다른 볼륨의
  `mv`는 복사본을 staging·commit한 뒤 원본을 삭제합니다. commit 뒤 취소되거나
  원본 삭제가 실패하면 원본과 대상이 모두 남을 수 있으며, rollback을 위해
  이미 commit한 대상을 삭제하지 않습니다.

첫 파일시스템 변경 전에 전체 원본과 실제 대상을 검증합니다. 알려진 안전 위반은
아무것도 바꾸지 않고 `2`, 필요한 identity·순회 안전성을 확정할 수 없으면
아무것도 바꾸지 않고 `1`입니다. commit 전 복사 실패는 기존 대상을 그대로 두고
staging 데이터는 최선의 노력으로 정리합니다.

## 반드시 거부할 경우

다음은 `cp` 또는 `mv`를 실행하기 전에 거부해야 합니다.

- 원본과 실제 대상이 같은 경로로 정규화되는 경우
- 재귀 복사의 대상이 원본 폴더 내부로 정규화되는 경우
- 이동 대상이 원본 폴더 내부로 정규화되는 경우
- wildcard 경로, 파이프 입력, 원본·대상 외의 추가 인자
- `-f`와 `-n`의 동시 사용
- 원본 또는 재귀 탐색 중 symbolic link, junction, 그 밖의 reparse point가 있는
  경우

## 종료 코드 규칙

- 복사 또는 이동에 성공하면 종료 코드 `0`입니다.
- `-n`이 기존 대상을 건너뛰어도 `0`입니다.
- 원본 없음, 접근 거부, 대상 충돌, 열린 파일은 `1`입니다.
- 잘못된 문법, 지원하지 않는 옵션, 거부 규칙 위반은 `2`입니다.
- 취소는 `130`이며 문서화한 commit 이후 상태가 남을 수 있습니다.

## 필수 확인 예시

```text
cp app.json backup.json
cp -r src backup
cp -n config.json backup\config.json
mv old-name.txt new-name.txt
mv build dist
```
