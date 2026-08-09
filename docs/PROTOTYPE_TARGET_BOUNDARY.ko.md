# 프로토타입·목표 경계

상태: 구속력 있는 문서·migration 경계. 이 문서는 구현을 허가하지 않는다.

영문판: [PROTOTYPE_TARGET_BOUNDARY.md](PROTOTYPE_TARGET_BOUNDARY.md)

## 이 경계가 필요한 이유

현재 repository의 애플리케이션은 prototype이다. 지금까지 쌓은 P0 계약은 앞으로의
공통 해석기 target을 설명한다. 계획·migration 동안 둘은 함께 존재하지만 prototype
동작이 자동으로 target 약속이 되지는 않고, target 계약이 현재 코드가 이미 구현했다는
뜻도 아니다.

## 기준 문서 우선순위

향후 공통 해석기 release의 충돌은 다음 순서로 해결한다.

1. [구현 시작 게이트](IMPLEMENTATION_GATE.ko.md)와 승인된 전체 계획 통합 재검토
2. 경로, 터미널 세션, text stream, mutation, runner 전달·실행, 보안, 성능, CLI
   launch의 공통 target 계약
3. `docs/commands/` 아래 P0 명령 계약
4. 공통 해석기 acceptance test plan
5. 통제된 cutover 뒤 README와 release·support 자료

현재 `README.ko.md`, `docs/TEST_MATRIX.md`, `docs/MANUAL_SMOKE_TEST.md`, 기존 제품
test는 cutover 전까지 prototype 증거다. Windows 10, `sed`·`xargs` 같은 P1 명령,
입력 redirection, shell별 mapping, 그 밖의 P0 밖 동작을 적더라도 target 계약보다
우선하지 않는다.

## 구현 승인 전

- 제품 코드와 동작을 바꾸는 test는 건드리지 않는다.
- 계획 문서에서 prototype·target 지위를 표시하고 상호 link할 수 있다.
- 마지막 통합 재검토에서 C1-C10을 해결하고 영문·한글판을 맞춘 뒤 구현 게이트에
  따른 사용자의 명시적 승인을 받아야 한다.
- 성능값은 제안 상태다. 측정하지 않은 prototype·debug build가 통과했다고 문서만으로
  주장하면 안 된다.

## Migration test 분리

명시적 구현 승인 뒤 legacy prototype suite 옆에 새 `contract-v1` suite를 추가한다.
새 동작을 green처럼 보이게 하려고 legacy 기대값을 그 자리에서 고치지 않는다.

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
[ ] 기록한 기준 환경에서 승인된 성능 배포 상한 통과
[ ] prototype 전용 mapping과 쓰기 가능한 임시 transport 제거
[ ] README와 support matrix가 관찰한 target 동작만 설명
[ ] installer가 계약한 wingman 명령과 보호된 runner 제공
[ ] 영문·한글 사용자 문서 일치
[ ] 사용자의 명시적 최종 승인
```

Cutover 때 README를 공개 target 요약으로 바꾸고 상세 계약에 연결한다. 그전에는
prototype banner를 계속 보이게 둔다.
