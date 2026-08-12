# 성능 예산·측정 계약 (초안)

상태: P0 성능 계약 제안. 수치는 통합 구현 계획을 재검토하고 승인할 때부터
구속력을 가진다. 이 문서는 구현을 허가하지 않는다.

영문판: [PERFORMANCE_BUDGET.md](PERFORMANCE_BUDGET.md)

## 제품 수준 결정

Wingman은 대화형 터미널답게 즉각적으로 느껴져야 하지만, 단독 `cmd.exe`와 같은
자원만 사용해야 하는 것은 아니다. Wingman에는 터미널 renderer, WebView2
process group, Rust host, PTY, 활성 네이티브 셸이 포함된다. 따라서 같은 셸을
실행하는 완전한 터미널 host가 공정한 비교 대상이다.

성능은 안정적인 구조 경계에서 측정한다. 나중에 xterm/WebView2를 네이티브
renderer로 교체하더라도 같은 시험을 그대로 적용할 수 있어야 한다.

## 기준 환경

배포를 막을 수 있는 공식 측정은 최적화된 release build와 지원 Windows 11 24H2 이상 x64
matrix에서 다음 최소 기준 장비로 수행한다.

- 물리 CPU 4 core / logical processor 8개
- RAM 8 GiB, SSD, 내장 또는 entry-level GPU
- 100% 또는 125% 배율에서 기본 1100 x 720 Wingman 창
- Windows 균형 조정 전원 모드, 전원 연결, Microsoft Defender 활성화
- 최신 Evergreen WebView2 Runtime
- 로컬 창 하나, WebView 하나, 활성 셸 세션 하나

`cmd.exe`와 Windows PowerShell 5.1을 각각 측정한다. 결과마다 CPU, storage,
Windows build, shell build, WebView2 version, 전원 모드, Wingman commit을
기록한다. 전원 분리·절전 조건은 회귀 확인용 보조 결과로 기록하되 초기 P0 배포
게이트로 사용하지 않는다.

## 비교 기준선

모든 benchmark에서 다음을 구분해 기록한다.

1. 표준 console host에서 실행한 bare shell: 하한 참고값일 뿐 합격 기준은 아님
2. 같은 셸을 tab 하나에서 실행한 Windows Terminal: 공정한 terminal-host 비교
3. Familiar OFF 상태의 Wingman
4. Familiar ON에서 네이티브 명령을 통과시키는 Wingman
5. `wingman-runner`로 P0 명령을 실행하는 Wingman

Wingman이 단독 `cmd.exe`보다 무겁다는 이유만으로 실패하지 않는다. 절대 배포
상한을 넘거나, 마지막으로 승인된 Wingman 기준선보다 확인된 회귀가 생기면 실패한다.

## P0 성능 예산

| 지표 | 목표 | 배포 상한 | 측정 경계 |
| --- | ---: | ---: | --- |
| cold launch 후 셸 입력 가능 | 1.5초 이하 | 3.0초 이하 | process 시작부터 prompt 준비 후 probe 입력이 수락·echo될 때까지 |
| warm launch 후 셸 입력 가능 | 0.8초 이하 | 1.5초 이하 | runtime과 OS cache가 준비된 상태의 동일 경계 |
| 로컬 key 입력부터 셸 echo | p95 50 ms 이하 | p95 100 ms 이하 | xterm 입력 event부터 PTY 출력이 화면에 render될 때까지 |
| 신뢰 가능한 입력의 통과 판정 | p95 2 ms 이하 | p95 10 ms 이하 | 검증된 `Editing/Reliable` 상태의 Enter부터 `PassThrough` 결정까지 |
| P0 runner 전달 overhead | p95 75 ms 이하 | p95 150 ms 이하 | 한 줄 제출부터 runner 작업 시작까지, 실제 작업 시간 제외 |
| 터미널 resize 안정화 | p95 100 ms 이하 | p95 300 ms 이하 | resize event부터 renderer fit과 PTY 크기 반영까지 |
| Ctrl+C 취소 | p95 200 ms 이하 | p95 500 ms 이하 | interrupt 입력부터 runner 종료 `130`과 셸 제어권 반환까지 |
| 안정화 후 idle CPU | median 0.2%·p95 1% 이하 | median 0.5%·p95 2% 이하 | 10초 안정화 후 Wingman 전체 process tree |
| 안정화 후 idle private working set | 250 MiB 이하 | 350 MiB 이하 | host, WebView2 group, PTY 지원, 활성 셸 하나; runner 없음 |
| 30분 idle memory 증가 | 10% 및 25 MiB 이하 | 20% 및 50 MiB 이하 | 안정화된 idle 기준값보다 증가한 양 |
| 설치된 Wingman 파일 | 30 MiB 이하 | 60 MiB 이하 | 앱, runner, asset; 공유 Evergreen WebView2 제외 |
| 깨끗한 실행 100회 후 로컬 앱 데이터 | 25 MiB 이하 | 100 MiB 이하 | 사용자가 만든 export를 제외한 Wingman/WebView profile 데이터 |

한 행에 비율과 절대량 조건이 함께 있으면 둘 다 충족해야 한다. 1단계 경계 기술
검증에서 최적화 release build의 raw data와 설명한 이유로 한 번의 보정 제안을 만들
수 있다. 사용자는 이를 통합 구현 계획과 함께 검토한다. 승인 뒤 target·ceiling은
P0 acceptance까지 고정하며, 이후 변경은 조용한 목표 이동·test 완화가 아니라
명시적 성능 계약 결정이 필요하다.

## 준비 완료와 process 계산

창이 보였다는 것만으로 시작이 끝난 것은 아니다. 실제 입력 가능 상태는 renderer에
focus가 있고, PTY와 선택한 셸이 실행 중이며, 유효 prompt marker가
`Editing/Reliable`을 만들고, 정상 터미널 입력 경로로 보낸 probe가 정상 PTY
rendering 경로를 통해 돌아온 상태다.

메모리와 CPU에는 Wingman이 소유한 전체 process tree를 포함한다.

```text
wingman.exe
  + 연관된 WebView2 browser/renderer/GPU/utility process
  + PTY 또는 console 지원 process
  + 활성 powershell.exe 또는 cmd.exe 하나
  + P0 요청 실행 중의 wingman-runner.exe
```

Launch 시간은 공개 launcher process부터 시작하며 [CLI 실행 계약](CLI_LAUNCH_CONTRACT.ko.md)의
보호된 same-binary GUI handoff를 포함한다. Launcher는 살아 있는 동안 계산하고,
readiness를 acknowledge하고 종료한 뒤에만 settled 측정을 시작한다. 남은 GUI-role
process도 이름은 `wingman.exe`이며 아래 runtime tree를 소유한다.

공유 working set을 더하면 공유 runtime page를 중복 계산할 수 있으므로 private
working set을 주 메모리 지표로 사용한다. 진단을 위해 total working set, private
bytes, process 수, JavaScript heap, Rust allocation도 함께 기록한다. 공유 설치된
WebView2 Runtime은 Wingman 설치 용량에서 제외하지만 실행 중 WebView2 process는
자원 사용량에 포함한다.

## 필수 workload

### 시작과 상호작용

- 정상 파일 시스템 폴더에서 두 지원 셸로 각각 실행
- 정상 UI 경로로 입력, 삭제, 붙여넣기, 제출, Familiar 전환, resize, session restart
- [터미널 세션 계약](TERMINAL_SESSION_CONTRACT.ko.md)에 따른 reliable prompt 편집과
  completion·알 수 없는 편집·foreground 통과 비교
- Familiar OFF, 네이티브 통과, P0 수락, P0 거부 비교
- ASCII뿐 아니라 한글, 공백, 긴 Windows 경로 입력

### 출력과 반응성

결정적인 helper가 실제 PTY를 통해 최소 10 MiB, 100,000줄의 UTF-8 데이터를
출력한다. Wingman은 byte 손실, process 실패, 응답 없는 창 없이 전체 stream을
render해야 한다. 출력 중 표본 입력 지연은 p95 200 ms 미만을 유지한다. 전체
render 시간은 같은 Windows Terminal 기준의 2배 이하가 목표이며 3배가 배포 상한이다.

출력을 지우고 안정화한 뒤 private working set은 이전 idle보다 목표 25 MiB,
상한 50 MiB 이상 남지 않는다. scrollback에는 명시적인 제한이 있으며 무제한
터미널 히스토리는 P0 기능이 아니다.

Pipeline benchmark에는 channel capacity `1`, 의도적으로 느린 consumer, 최대 크기
record 하나, read 사이에서 나뉜 invalid UTF-8, early `head` stop도 넣는다. 취소·memory
상한을 만족하면서 [텍스트 record·stream 계약](TEXT_STREAM_MODEL.ko.md)을 보존해야 한다.
Throughput을 이유로 unbounded channel이나 raw-byte 우회를 허용하지 않는다.

### Runner와 파일 시스템

Runner 시험은 cache가 있는 경우와 없는 경우를 구분한다.

- 결정적인 UTF-8 100 MiB corpus의 `grep`
- 항목 20,000개 directory tree의 `find`
- renderer 비용을 제외하기 위해 redirect한 100 MiB `cat`
- materialization 비용이 드러나는 200,000줄 redirect `sort`
- 재귀 순회 취소와 아무 변경이 없는 `tail -f`

첫 구현 기준 측정에서 각 명령을 수락하기 전에 작업별 throughput 목표를 확정한다.
storage 속도와 관계없이 모든 경우가 취소 가능하고 자원 제한을 지키며 idle 상태에서
busy polling하지 않아야 한다. 파일 변화가 없는 `tail -f`는 일반 idle CPU 상한을
충족해야 한다.

재현 가능한 component 측정은 [성능 기준 측정 기록](PERFORMANCE_BASELINES.ko.md)에
보관하며 전체 process-tree 배포 게이트를 대신하지 않는다.

### 지속 실행

30분 동안 출력, clear, resize, P0 시작·취소, shell restart를 반복한다. 메모리
증가 상한과 입력 반응성을 지키고, 소유 세션이 끝난 뒤 runner나 broker 요청을
남기지 않아야 한다.

## 측정 절차

- release build를 사용한다. 개발 서버, DevTools, debugger, hot reload 결과는
  배포 근거로 인정하지 않는다.
- 관계없는 foreground 앱은 닫되 백신과 일반 Windows service는 사용자 기본
  상태로 유지한다.
- warm launch는 3회 예열한 뒤 최소 20회를 기록한다. cold launch는 최소 5회의
  통제된 restart 또는 문서화한 동등 cold-runtime 조건에서 첫 실행을 기록한다.
- 상호작용 분포는 최소 100개 표본으로 median, p95, maximum, raw data를 기록한다.
  멈춤 현상을 숨기는 평균만으로 합격시키지 않는다.
- 각 비교군에서 같은 셸, 폴더, 창 크기, corpus, 전원 상태를 사용한다.
- Wingman 경계에는 monotonic in-process marker를 사용하고 전체 시스템 startup,
  CPU, disk, WebView2 진단에는 ETW/WPR/WPA를 사용한다. 계측은 개발 측정 경로이지
  제품 telemetry가 아니다.
- benchmark 정의와 요약 결과를 release 기록에 보관한다. 경로나 터미널 데이터가
  들어간 raw trace는 로컬에만 두고 [보안 모델](SECURITY_MODEL.ko.md)을 따른다.

## 회귀 정책

절대 배포 상한을 넘으면 P0 수락을 막는다. 상한 이내라도 마지막 승인 Wingman
기준보다 다음만큼 반복 가능한 회귀가 생기면 원인을 조사한다.

- launch, 입력, resize, runner 전달, 취소 시간: 10%
- 안정화 메모리, idle CPU, 설치 크기, 지속 실행 증가량: 15%
- 대량 출력 또는 runner throughput: 20%

표준 시험을 독립적으로 3회 반복해 재현된 경우만 반복 가능한 회귀로 판단한다.
성능보다 정확성, 보안, 데이터 무결성 문제를 먼저 고친다. 성능을 위해 입력 검증,
CSP, 요청 인증, 취소, 안전 검사를 제거하면 안 된다.

예외는 원인, 사용자가 체감하는 영향, 완화 방법, 명시적 승인이 있어야 한다. 더
빠른 개발 장비를 사용하는 것은 유효한 예외가 아니다.

## WebView 교체 판단 기준

profiling에서 WebView2가 실제 병목이라고 확인되기 전에는 P0 구조를 유지한다.
성능 예산 실패에는 다음 순서로 대응한다.

1. 표준 시험으로 재현한다.
2. trace로 host, WebView, renderer, IPC, PTY, shell, runner 비용을 구분한다.
3. 중복 작업, 불필요한 WebView·IPC, 남은 DOM/JS state, polling을 제거한다.
4. release build를 다시 측정한다.
5. 그래도 launch 또는 idle-memory 상한을 크게 넘으면 별도의 네이티브 renderer
   기술 검증을 수행하고 동일한 black-box 시험으로 비교한다.

측정한 WebView 비용만으로 배포 상한을 20% 넘고 일반 최적화로 해결하지 못할 때
renderer 재작성을 검토한다. 재작성도 구현 게이트에 따른 사용자 승인이 필요하며,
이 성능 계약 자체가 재작성을 허가하지는 않는다.

## 측정 전 현재 초안의 공백

현재 초안에는 안정화된 Windows PowerShell idle 상태를 위한 release 전용 전체
process tree 계산이 있다. 신뢰할 수 있는 startup/readiness marker, 결정적인 PTY
부하 생성, runner timing, 자원 제한, 지속 실행 자동화는 아직 없다. 이는 계획된
측정 요구사항이며 구현 승인 전에 제품 계측을 추가해도 된다는 뜻은 아니다.

## 조사 근거

- [Microsoft: 앱 성능 계획과 측정](https://learn.microsoft.com/en-us/windows/apps/develop/performance/planning-measuring-performance)
- [Microsoft: WebView2 성능 권장 사항](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/performance)
- [Microsoft: WebView2 process model](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-model)
