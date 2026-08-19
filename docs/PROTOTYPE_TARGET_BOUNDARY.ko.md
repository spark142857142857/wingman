# 프로토타입·목표 경계

상태: 과거 migration 경계다. 코드 cutover는 끝났으며 최종 수동·외부 matrix·signing·
사용자 수락 gate는 계속 열려 있다.

영문판: [PROTOTYPE_TARGET_BOUNDARY.md](PROTOTYPE_TARGET_BOUNDARY.md)

## 이 경계가 필요한 이유

현재 repository의 애플리케이션은 P0 릴리스 후보다. 과거 prototype 동작은 README와
test snapshot에서 과거 자료로 명시해 보존하지만 현재 target 약속은 아니다. 구현된 P0
계약과 현재 release matrix가 후보의 기준이다.

## 기준 문서 우선순위

현재 공통 해석기 후보의 충돌은 다음 순서로 해결한다.

1. [구현 시작 게이트](IMPLEMENTATION_GATE.ko.md)와 승인된 전체 계획 통합 재검토
2. 경로, 터미널 세션, text stream, mutation, runner 전달·실행, 보안, 성능, CLI
   launch의 공통 target 계약
3. `docs/commands/` 아래 P0 명령 계약
4. 공통 해석기 acceptance test plan
5. 현재 README와 release·support 자료

`README.ko.md`의 과거 절, `docs/TEST_MATRIX.md`, `docs/MANUAL_SMOKE_TEST.md`는
prototype 증거로 남는다. Windows 10, `sed`·`xargs` 같은 P1 명령, 입력 redirection,
shell별 mapping, 그 밖의 P0 밖 동작을 적더라도 target 계약보다 우선하지 않는다.

## 과거 구현 승인 전 gate

- 제품 코드와 동작을 바꾸는 test는 건드리지 않는다.
- 계획 문서에서 prototype·target 지위를 표시하고 상호 link할 수 있다.
- 마지막 통합 재검토에서 C1-C10을 해결하고 영문·한글판을 맞춘 뒤 구현 게이트에
  따른 사용자의 명시적 승인을 받아야 한다.
- 성능값은 제안 상태다. 측정하지 않은 prototype·debug build가 통과했다고 문서만으로
  주장하면 안 된다.

## Migration test 분리

명시적 구현 승인 뒤 contract suite를 legacy prototype 증거 옆에 추가했다. 새 동작을
green처럼 보이게 하려고 legacy 기대값의 이름만 바꾸지 않았다.

- Legacy test: 계획한 cutover 전에 migration이 옛 prototype을 뜻하지 않게 깨뜨렸는가
- Contract-v1 test: target P0가 공통 Rust core, runner, 두 shell transport,
  application 경계를 통과하는가
- 의도된 차이는 target 계약과 이를 소유한 cutover phase를 migration ledger에 남긴다.

Target suite 통과와 통제된 cutover 승인 뒤에만 legacy compatibility mapping과 test를
제거한다. 과거 test 증거는 archive할 수 있지만 target acceptance로 이름만 바꾸지 않는다.

## 한 번의 성능 보정

1단계 경계 기술 검증에서 기준 장비의 최적화 release build를 성능 계약으로 측정한다.
기록한 raw data와 설명한 이유를 근거로 현재 잠정 target·ceiling을 한 번 조정하는
제안을 만들 수 있다. 사용자는 이를 통합 구현 계획과 함께 검토한다. 승인 뒤 값은 P0
acceptance까지 고정하며 이후 변경은 조용한 test 완화가 아니라 명시적 성능 계약
결정이 필요하다.

## 통제된 cutover checklist

다음을 모두 만족할 때만 cutover한다.

```text
[ ] contract-v1 자동·shell·application·보안·manual suite 통과
[x] 기록한 로컬 기준 환경에서 승인된 성능 배포 상한 통과
[x] prototype 전용 mapping과 쓰기 가능한 임시 transport 제거
[x] README와 support matrix가 현재 동작과 과거 증거를 구분
[x] installer가 계약한 wingman 명령과 보호된 runner 제공
[x] 영문·한글 현재 계약 문서 일치
[ ] 사용자의 명시적 최종 승인
```

코드와 문서 cutover는 끝났다. 첫 항목은 release matrix의 실제 수동 UI 확인과 외부
variant 때문에 열려 있으며 최종 release 수락은 아직 주장하지 않는다.
