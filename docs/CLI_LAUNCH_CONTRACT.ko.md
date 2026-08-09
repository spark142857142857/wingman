# 애플리케이션 CLI 실행 계약 (초안)

상태: 합의된 제품 방향. 이 계약은 구현을 허가하지 않는다.

## 실행 파일 이름

```text
wingman.exe         사용자가 실행하는 터미널 애플리케이션
wingman-runner.exe  내부 P0 sidecar이며 사용자 명령이 아님
```

설치된 애플리케이션은 일반 PowerShell과 `cmd` 세션에서 `wingman`으로 실행할 수 있다.
`PATHEXT` 또는 Windows App Execution Alias 덕분에 `.exe`는 생략할 수 있다.

## P0 공개 grammar

```text
wingman [--shell powershell|cmd] [--] [PATH]
wingman --help
wingman --version
```

`--shell`은 최대 한 번이며 선택적 `--`와 path보다 앞에 와야 한다. Path는 최대
하나다. `--help`, `--version`은 단독 argument여야 하며 short option,
`--shell=value`, 추가 operand는 문법 종료 `2`다.

선택적인 시작 폴더는 공통
[Windows 경로·파일 시스템 계약](WINDOWS_PATH_CONTRACT.ko.md)을 따른다.
Drive-relative, root-relative, device namespace, ADS, wildcard, 모호한 경로는
GUI process를 시작하기 전에 거부한다.

- 경로가 없으면 호출한 셸의 현재 Windows 파일 시스템 폴더에서 새 Wingman 창을 연다.
- 상대 경로는 호출자의 현재 폴더를 기준으로 계산하고, 절대 경로는 Windows 네이티브 경로를 유지한다.
- P0는 실제로 존재하는 폴더만 받는다. 없는 경로나 파일 경로는 짧은 stderr 진단을 출력하고 창을 열지 않으며 `1`로 종료한다.
- 잘못된 문법이나 미지원 옵션은 `2`로 종료한다.
- `--shell`이 없으면 저장된 셸 설정을 사용하고, 저장값도 없으면 Windows PowerShell을 쓴다.
- GUI 전달이 성공하면 Wingman 창이 닫힐 때까지 기다리지 않고 launcher가 `0`으로 종료한다.
- 호출할 때마다 새 Wingman 창을 요청한다. 나중에 single-instance 조정을 넣더라도 이 화면 동작은 유지해야 한다.

## P0 process topology

목표는 서명된 `wingman.exe` 하나가 두 process role을 맡고 runner는 별도로 두는
구조다. 이는 필수 경계 기술 검증 결정이며 구현 승인이 아니다.

```text
PowerShell/cmd
  -> wingman.exe 공개 console launcher
       -> 같은 wingman.exe의 보호된 내부 GUI role
            -> Wingman 창, WebView/renderer, PTY, 선택 shell

Wingman P0 제출
  -> wingman-runner.exe one-shot sidecar
```

공개 binary는 Windows console-subsystem application이라 `cmd`와 PowerShell이 launcher
status를 기다린다. 지원 Windows matrix에서는 packaged manifest의 detached console
allocation policy로 Explorer·shortcut 실행 때 불필요한 console을 만들지 않아야 하며,
이는 가정이 아니라 경계 기술 검증 대상이다. 공개 호출은 항상 launcher role이다. 전체 공개 grammar를 parse·validate하고 호출자의
filesystem cwd·environment·access token을 snapshot하며 시작 폴더를 resolve한다.
`--help`, `--version`, 실패에는 GUI initialization을 하지 않는다.

유효한 창 요청이면 launcher가 같은 설치 binary를 내부 GUI role의 새 process로
만든다. GUI child는 console window가 없고 일반 launcher handle·stdio를 상속하지
않으며 같은 unelevated/elevated access token을 쓰고 launcher 정상 종료 뒤에도 산다.
정확한 Windows creation flag는 경계 기술 검증에서 이 성질을 증명해야 한다. 초기
후보는 explicit inherited-handle allowlist와
`CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP`다.

내부 GUI role은 공개 CLI가 아니다. Command line에는 고정 internal marker와 상속한
handoff handle 번호만 둔다. Handle은 예측 불가능한 nonce, parent identity, resolve한
시작 폴더, 선택 shell을 담은 크기 제한·versioned message를 전달한다. 살아 있는 상속
handle 없는 직접 호출이나 replay는 창을 열지 않고 `2`다. Path, environment value,
shell source는 child command line에 직렬화하지 않는다.

## Readiness와 오류 전달

Launcher와 child는 양방향 handoff를 끝낸다.

1. Child가 internal message를 검증하고 directory를 재검증한다.
2. Top-level window를 만들고 선택한 PTY shell을 시작한다.
3. Handoff channel로 크기가 제한된 `Ready` 또는 `Failed`를 보낸다.
4. Launcher가 `Ready`를 acknowledge한 뒤 `0`으로 끝난다. 이 acknowledge 뒤에만
   GUI child가 독립적으로 session을 소유한다.

`Ready`는 window가 존재하고 initial shell·PTY가 ownership을 받았다는 뜻이며 사용자가
창을 닫을 때까지 기다린다는 뜻이 아니다. Readiness 전 filesystem·asset·WebView·PTY·
shell 시작 실패는 usable window 없이 launcher stderr 진단 하나와 `1`이다. 문법은
`2`, 기다리는 중 Ctrl+C는 `130`이다. Handoff timeout은 10초이고 operational `1`이다.
Launcher acknowledgement가 없으면 child는 보고되지 않은 orphan이 되지 않고 종료해야
한다. Ready acknowledge 뒤 GUI 실패는 이미 반환한 launcher 상태를 바꾸지 못한다.

경계 기술 검증은 `cmd`와 Windows PowerShell 5.1에서 space·한글·UNC path, missing
path, invalid combination, missing asset, shell 시작 실패, timeout, Ctrl+C, 일반·관리자
token, 반복 실행, internal-role 직접 악용을 증명해야 한다. 같은 binary로 console-free
child, 신뢰 가능한 status, 독립 lifetime을 만족하지 못하면 별도 signed internal GUI
binary를 추가하기 전에 계약을 다시 검토한다. 공개 `wingman.exe`와
`wingman-runner.exe` 이름은 조용히 바꾸지 않는다.

## Wingman 내부의 네이티브 통과

`wingman`은 P0 호환 명령이 아니다. Wingman 셸 안에서 입력해도 네이티브 실행 파일 호출로 그대로 전달되며,
같은 계약에 따라 Wingman 창을 하나 더 연다.

## 설치 등록

- NSIS/MSI 형태 설치는 machine-wide 범위를 요구하지 않고 현재 사용자의 명령 검색 경로에 앱 명령을 등록한다.
- 향후 MSIX·Store 패키지는 `wingman.exe` Windows App Execution Alias를 사용한다.
- 제거할 때는 해당 설치가 만든 등록만 제거한다.
- 내부 `wingman-runner.exe`는 일반 PATH 명령으로 등록하지 않는다.

CLI 경로와 옵션 값은 애플리케이션 인자로 파싱하며 셸 소스로 다시 조립하지 않는다.

## 조사 근거

- Microsoft는 console-subsystem application이 `cmd`·PowerShell을 기다리게 하고
  Windows 11 24H2의 detached console-allocation manifest policy가 기존 console 밖에서
  console 할당을 피할 수 있다고 설명한다:
  [Console Allocation Policy](https://learn.microsoft.com/en-us/windows/console/console-allocation-policy).
- `CREATE_NO_WINDOW`는 console child를 console 없이 실행하고
  `CREATE_NEW_PROCESS_GROUP`은 별도 process group을 만들며 그 child의 inherited Ctrl+C
  handling을 끈다:
  [Process Creation Flags](https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags).
- 특정 handle만 상속할 때 `STARTUPINFOEX`와 `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`를
  쓰라는 Microsoft 지침:
  [Create processes](https://learn.microsoft.com/en-us/windows/win32/procthread/creating-processes).
