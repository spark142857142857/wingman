# 보안·신뢰 모델 (초안)

상태: 계획 중인 공통 해석기를 위한 보안 계약 제안. 이 문서는 구현을 허가하지 않는다.

영문판: [SECURITY_MODEL.md](SECURITY_MODEL.md)

## 보안 목표

Wingman은 터미널이지 sandbox가 아니다. 사용자가 의도적으로 제출한 네이티브
명령은 Wingman 프로세스의 Windows 접근 토큰이 허용하는 파일을 읽고, 바꾸고,
삭제할 수 있다. Wingman의 책임은 그 권한을 몰래 높이거나, 명령 대상을 바꾸거나,
UI 경계를 통해 터미널 데이터를 노출하거나, 공격자가 악용할 실행 경로를 추가하지
않는 것이다.

> 호환 명령은 활성 네이티브 셸과 같은 Windows 권한만 가지며, 그보다 더 큰
> 권한을 갖지 않는다.

## 보호할 대상

- 사용자의 파일, 자격 증명, 환경 변수, 클립보드, 명령 의도
- 터미널 입력·출력·스크롤백·히스토리
- 해석기, 실행 계획, runner, 설치 파일의 무결성
- 현재 셸의 작업 폴더, 환경, 종료 상태
- 아직 실행되지 않은 runner 요청의 무결성과 기밀성

## 신뢰 경계

```text
로컬 번들 UI / xterm
  -> 제한된 Tauri invoke·event 경계
신뢰하는 Rust host와 공통 해석기
  -> 세션 로컬 인증 broker
신뢰하는 wingman-runner
  -> 상속한 사용자 토큰으로 Windows 파일 시스템 사용

네이티브 PowerShell/cmd와 그 자식 프로세스
  -> 사용자 지시로 실행하지만 Wingman P0 의미를 결정하는 주체는 아님

원격 콘텐츠, 터미널 출력, 붙여넣은 텍스트, 경로, 패턴, 환경,
호환성 업데이트
  -> 신뢰하지 않는 데이터
```

WebView는 터미널 입력을 보낼 권한이 반드시 필요하다. 따라서 UI가 침해되면
임의의 네이티브 명령을 입력할 수 있다. 이를 막기 위해 Wingman은 번들된 로컬
UI만 불러오고, 제한적인 Content Security Policy를 적용하고, 불필요한 네이티브
기능을 노출하지 않아야 한다. 임의의 네이티브 통과 명령을 Rust에서 모두
안전하게 검증하려 하면 더 이상 일반 터미널이 아니게 된다.

이 모델은 잘못된 입력, 원격 콘텐츠 침해, 명령 문자열 조립 주입, 다른 사용자나
세션의 broker 접근, 오래되거나 재사용된 요청, 의도하지 않은 권한·대상 확대를
방어한다. 같은 Windows 사용자 권한으로 이미 실행 중인 악성 코드, 침해된 관리자
계정, 침해된 운영체제로부터 사용자를 격리한다고 약속하지는 않는다.

## 권한과 관리자 실행 계약

- PowerShell, `cmd`, `wingman-runner`는 Wingman의 현재 Windows 접근 토큰을
  상속한다. Familiar 모드는 이 토큰을 바꾸지 않는다.
- Wingman은 UAC를 우회하거나 관리자 자격 증명을 저장하거나, 호환 명령 때문에
  `runas`를 실행하거나, 사용자 모르게 관리자 권한으로 재시작하지 않는다.
- 사용자가 Wingman 자체를 명시적으로 관리자 권한으로 실행했다면 터미널 세션
  전체가 관리자 권한이다. UI는 일회성 경고가 아니라 지속적이고 명확한 관리자
  상태 표시를 보여줘야 한다.
- 네이티브 명령이 Windows의 정상적인 권한 상승 절차를 스스로 요청할 수는 있다.
  Wingman은 그 절차를 막거나 흉내 내지 않는다.
- `-f`, 재귀 순회 등 어떤 P0 옵션도 Windows ACL, 공유 위반, 백신 통제,
  사용 중인 파일 제한을 우회하지 않는다.
- Familiar OFF와 네이티브 통과 명령도 P0와 동일한 활성 셸 토큰을 사용한다.

## WebView와 Tauri 경계

배포 빌드는 번들된 로컬 프론트엔드 자산만 불러온다. 원격 페이지 이동, 원격
스크립트, 임의 iframe, 개발 서버, 실행 중 내려받는 UI 코드를 금지한다.

배포 설정은 다음을 지켜야 한다.

- null CSP를 번들 UI에 필요한 최소 CSP로 교체한다.
- 명시적으로 등록한 Tauri command와 event만 허용한다.
- 범용 프로세스 실행, 임의 파일 시스템 접근, 제한 없는 URL 열기 API를
  프론트엔드 JavaScript에 노출하지 않는다.
- TypeScript에서 이미 검사했더라도 모든 Rust command가 입력 타입, 길이,
  범위, 활성 세션 ID, 현재 상태를 다시 검증한다.
- 비동기 PTY event에 세션 ID를 붙이고 이전 세션의 데이터는 버린다.
- 터미널 링크는 명시적인 사용자 동작과 허용된 scheme에서만 열고, 터미널
  출력을 신뢰하는 HTML로 처리하지 않는다.
- 별도의 사용자 동작 없이 클립보드 쓰기, 프로세스 실행, WebView 이동,
  권한 기능 호출이 가능한 터미널 escape 기능은 끈다.

외부 링크 열기는 범용 shell 권한이 아니다. 기능을 유지한다면 전용 scheme
allowlist와 운영체제의 외부 브라우저를 사용한다. URL 처리와 xterm 링크 동작은
별도의 보안 테스트 대상이다.

## 입력과 명령 조립 계약

- 네이티브 통과 입력은 사용자가 제출한 원문을 보존한다. 숨겨진 셸 문법을
  덧붙이거나 신뢰할 수 없게 복원된 입력을 다시 해석하지 않는다.
- Prompt 증거, 편집 reliability, 불확실 입력 fallback, paste, 셸 전환은
  [터미널 제출·세션 계약](TERMINAL_SESSION_CONTRACT.ko.md)을 따른다. WebView는
  신뢰 가능한 prompt나 줄이라고 주장할 수 없다.
- Wingman이 소유한 P0 입력은 typed value와 검증된 실행 계획으로 바꾼다.
  경로, 패턴, 텍스트를 PowerShell 또는 `cmd` 소스 문자열에 이어 붙이지 않는다.
- WebView는 기준 원문 통과 또는 불투명한 일회용 요청 ID와 display line만 받는다.
  계획과 준비된 진단은 Rust에 남고 세션 broker를 통해서만 이동한다.
- 내부 셸 호출에는 [runner 전달 계약](RUNNER_TRANSPORT.ko.md)에 정의한 고정
  설치 runner 경로, 고정 전달 필드, 예측 불가능한 요청 ID만 포함한다.
- runner는 검증된 P0 동작을 직접 구현한다. 구현을 위해 셸을 다시 실행하거나
  계획 데이터로 명령 문자열을 만들지 않는다.
- 소유한 문법이 미지원이거나 모호하면 종료 코드 `2`로 안전하게 실패한다.
  일부만 변환한 뒤 통과시키지 않는다.
- parsing, normalization, 실제 파일 시스템 사용 사이에서 한 경로를 허가하고
  다른 경로를 조작하지 않는다. reparse point 동작은 `rm`의 링크 비순회 규칙을
  포함한 각 명령 계약을 따른다.
- 모든 경로 형태, runner 측 해석, object identity 비교, root, reparse 동작은
  공통 [Windows 경로 계약](WINDOWS_PATH_CONTRACT.ko.md)을 따른다.

## Broker와 runner 계약

- Wingman 세션마다 broker 하나만 두고 현재 사용자와 로컬 로그인 세션만
  허용한다. 원격 named-pipe 접근은 거부한다.
- 별도의 owner-only 로컬 named pipe가 크기 제한 editor-readiness frame을
  전달한다. Worker는 bounded inbox와 stop state만 소유하며 app session,
  interpreter, PTY writer, request broker를 소유하거나 lock하지 않는다.
- Readiness nonce와 pipe 이름은 integration의 첫 동작에서 PowerShell process
  environment에서 제거해 이후 native child가 상속하지 못하게 한다. Windows
  PowerShell은 현재 `-Command` integration bootstrap보다 먼저 사용자 profile을
  로드하므로, 그 profile은 P0 신뢰 경계 안에 둔다. 더 강한 profile 격리는 별도
  startup 설계 검토가 필요하다.
- Readiness queue overflow, 인증 뒤 malformed 입력, replay, timeout, disconnect,
  worker 실패는 네이티브 입력으로 fail-closed한다. 입력이 이미 전달된 editor
  cycle은 늦은 readiness frame으로 승격하지 않는다.
- 요청 ID는 예측 불가능한 bearer capability다. 수명이 짧고, 한 번만 원자적으로
  소비하며, 셸 재시작이나 앱 종료 때 무효화한다.
- 요청에는 protocol version, 엄격한 schema, 제한된 직렬화 크기, 검증된
  enum·길이·범위 필드가 있다. 알 수 없는 필드나 버전을 우연히 실행하지 않는다.
- runner는 요청을 다시 검증하고 알 수 없거나, 만료됐거나, 재사용됐거나,
  잘못됐거나, 세션이 맞지 않는 요청은 파일 접근 전에 거부한다.
- 설치 애플리케이션이 통제하는 runner 절대 경로를 사용한다. `PATH` 검색으로
  실행할 `wingman-runner.exe`를 고르지 않는다.
- 패키징한 바이너리와 실행 지원 파일은 서명하고 일반 설치 폴더 ACL로 보호한다.
  쓰기 가능한 임시 스크립트를 배포 실행 방식으로 사용하지 않는다.
- broker 메시지와 진단은 전체 직렬화 계획, 요청 secret, 환경을 출력하지 않는다.

Named-pipe ACL과 요청 ID는 의도하지 않은 다른 세션 사용과 재사용을 막는다.
같은 사용자 권한으로 실행 중인 적대적 프로세스까지 격리한다고 약속하지는 않는다.

Application launch는 [CLI 실행 계약](CLI_LAUNCH_CONTRACT.ko.md)의 보호된 same-binary
handoff를 쓴다. Allowlist한 handoff handle만 상속하고 내부 GUI role은 missing·stale·
replayed·parent-mismatched message를 거부한다. Launch는 권한을 높이지 않고 path·
environment value를 shell source나 child command line에 복사하지 않는다.

## 파괴적 작업과 사용자 의도

Wingman은 모든 네이티브 명령에 확인 창을 추가하지 않는다. 이는 신뢰할 수 있는
보안 경계를 만들지 못하면서 일반 터미널 동작만 바꾼다. 네이티브 명령은 사용자의
책임 아래 원문 그대로 통과한다.

Wingman이 소유한 P0 명령은 제한된 문법과 문서화된 의미로 안전성을 확보한다.

- 지원하지 않는 옵션이나 wildcard는 거부한다.
- 변환 과정에서 명시적인 대상 집합을 넓히지 않는다.
- `rm`은 파괴적 실행 전에 drive root, share root, 현재 폴더, 그 상위 폴더,
  reparse point 규칙을 적용한다.
- 진단은 영구 삭제를 휴지통 동작처럼 표현하지 않는다.
- 완료한 파일 변경을 transaction이나 자동 복구가 가능한 작업처럼 표현하지 않는다.

전체 무변경 안전 경계, staged replacement, 결정적 순서, 부분 실패, 취소 동작은
[mutation 실행 계약](MUTATION_EXECUTION_CONTRACT.ko.md)을 구속력 있게 따른다.

## 터미널 데이터, 히스토리, 클립보드, 로그

- 터미널 입력과 출력은 기본적으로 민감한 정보다. 배포 진단 로그에 원문 명령,
  PTY 출력, 환경 변수, 작업 경로, 클립보드 내용, 직렬화된 실행 계획을 기록하지
  않는다.
- Wingman이 소유한 scrollback만 현재 세션 메모리에 둔다. P0는 command recall
  목록을 추가하지 않고 활성 viewport를 제외한 터미널 세션마다 scrollback을 최대
  4,000줄 보존한다. Wingman은
  활성 셸의 설정된 네이티브 history를 바꾸지 않는다. 이 history는 영구 저장될 수
  있고 불투명한 runner 호출을 담을 수 있다. 향후 Wingman 영구 history는 저장 위치,
  보존·삭제 설정이 보이는 별도 opt-in 기능으로 만든다.
- 임의의 터미널 데이터는 정확히 분류할 수 없으므로 자동 secret 가림을 충분한
  보호 수단으로 보지 않는다.
- 복사는 사용자가 터미널 텍스트를 선택하고 명시적으로 실행할 때만 한다. 한 줄
  paste는 제출 없이 삽입한다. 줄바꿈이 있는 paste는 Send/Cancel 확인을 한 번 거친
  뒤 줄별 Wingman 판정 없이 하나의 네이티브 paste로 보낸다.
- crash report나 telemetry를 나중에 추가하더라도 opt-in으로 만들고 원문
  터미널 데이터와 요청 내용을 제외한다.

## 업데이트와 호환성 정의

- 애플리케이션과 runner 업데이트는 인증·서명·버전 관리·rollback이 가능해야
  한다. 서명이 잘못됐거나 protocol이 맞지 않으면 안전하게 실패한다.
- 원격으로 받은 호환성 정의는 실행 스크립트가 아니라 데이터다. 크기가 제한된
  서명 schema를 사용하며 Tauri 권한 확대, 임의 프로그램 실행, hard-coded
  안전 규칙 덮어쓰기를 할 수 없다.
- Windows, shell, Tauri, xterm, dependency 업데이트는
  [호환성 유지보수 계약](COMPATIBILITY_MAINTENANCE.ko.md)의 검증을 실행한다.
- 보안 제한은 patch release에서도 강화할 수 있지만 이유와 영향받는 동작을
  문서화해야 한다.

## 자원 제한과 서비스 거부

입력 줄, 요청 메시지, pipeline 단계, 경로 개수, 재귀 순회, 메모리에 담는 sort
입력, scrollback, 진단 크기에 명시적인 구현 제한을 둔다. 구조화된 record
streaming, UTF-8 실패, newline framing, backpressure, short-circuit, 부분 출력,
취소는 [텍스트 record·stream 계약](TEXT_STREAM_MODEL.ko.md)을 따른다. 제한 초과는
크기가 제한된 진단과 결정적인 종료 코드를 내며 host를 종료하거나 일부 다른 계획을
몰래 실행하지 않는다.

정확한 제한값은 프론트엔드에서 따로 만들지 않고 구현 검토 단계에서 정해 테스트한다.

## 필수 보안 검증

배포 전 최소한 다음을 테스트한다.

1. CSP와 배포 로컬 자산 로딩, 차단돼야 하는 원격 script·페이지 이동 시도
2. Tauri 권한 목록과 잘못된 값, 과대 입력, 범위 초과, 이전 세션 invoke 거부
3. 터미널 escape sequence, link scheme, clipboard 접근, 신뢰하지 않는 PTY
   출력 렌더링
4. 경로·패턴이 셸 소스가 되지 않음을 증명하는 P0 인용부호·metacharacter 사례
5. 다른 사용자·세션의 named-pipe 접근, 요청 추측·재사용·만료·취소·앱 종료,
   protocol 불일치
6. 일반 세션과 명시적으로 관리자 실행한 세션에서 숨은 권한 상승이 없고 관리자
   상태가 지속적으로 표시되는지 확인
7. `rm`, reparse point, root, 현재 폴더의 상위 폴더, ACL 거부, 사용 중인 파일,
   경로 변경 race 사례
8. 로그와 crash report에 원문 터미널 데이터와 요청 secret이 없는지 확인
9. 서명된 패키징, runner 절대 경로 선택, 업데이트 서명 실패, 호환 정의 schema 거부

## 배포 전에 제거할 현재 초안의 차이

현재 초안은 동작 검증 자료이지 배포 보안 기준이 아니다. 구현 검토에서 다음을
명시적으로 교체하거나 필요성을 입증해야 한다.

- 현재의 null CSP
- 실제로 필요하고 테스트된 사용자 기능이 없는 URL 열기 권한 등 넓은 Tauri 권한
- 쓰기 가능한 임시 PowerShell profile과 느슨한 bootstrap 경로
- 프론트엔드가 소유한 호환 parsing과 셸 명령 문자열 조립
- 크기 제한이 없는 bridge 입력, 터미널 데이터, 요청 저장소
- 명시적인 위협 테스트가 없는 clickable link와 terminal escape 동작

이 항목들은 마이그레이션 요구사항이며, 프로젝트 구현 게이트 승인 전에 제품
코드를 수정해도 된다는 뜻은 아니다.
