# 호환성 유지보수 계약 (초안)

상태: 합의된 운영 방향. 정확한 지원 릴리스는 제품 릴리스마다 선택한다.

## 원칙

외부 Windows·셸 업데이트는 호환성 검증의 계기다. Wingman이 소유한 P0 명령 의미를 조용히 다시 정하는 이유가 아니다.

## 권장 초기 지원 프로필

| 영역 | 초기 지원 |
| --- | --- |
| Windows | 24H2 이상인 지원 중 Windows 11 일반 배포 릴리스 |
| 아키텍처 | x64 |
| 셸 | `cmd.exe`, Windows PowerShell 5.1 (`powershell.exe`) |
| PowerShell 7 | 자체 실행·테스트 매트릭스가 생기기 전까지 지원 약속 없음 |
| Windows Server | 초기 지원 약속 없음 |
| Windows 10 | 새 지원 범위에서 제외 |

Wingman 릴리스마다 계약 버전, runner 프로토콜 버전, 테스트한 Windows 릴리스, 테스트한 셸,
아키텍처, 테스트 날짜를 기록한다. 기본 테스트 매트릭스는 현재 지원 Windows 릴리스와 바로 이전
지원 릴리스로 구성한다.

## 업데이트 계기

| 변화 | 필수 검증 |
| --- | --- |
| 월간 Windows 업데이트 | 실행·prompt marker·PTY·UTF-8·현재 폴더·Ctrl+C 통로 smoke test |
| Windows 기능 업데이트 | 전체 P0 계약·터미널 세션·셸 통합 매트릭스 |
| Windows PowerShell 5.1 업데이트 | 전체 PowerShell 통로·편집 fallback 테스트 |
| 향후 PowerShell 7 지원 | 지원 선언 전에 별도 버전 매트릭스 |
| Rust·Tauri·터미널 의존성 업데이트 | 재현 가능한 빌드, PTY·입력·출력 회귀, 성능 예산 카나리 |
| Wingman 릴리스 | 전체 현재 지원 매트릭스 |

## 테스트 계층

1. **계약 테스트**: 셸 없이 lexer, parser, 카탈로그 검증, 진단, 종료 동작을 확인한다.
2. **runner 파일 시스템 테스트**: 임시 Windows 폴더에서 경로, 리다이렉션, 읽기 전용·접근 실패,
   재분석 지점 안전을 확인한다.
3. **텍스트 stream 테스트**: split UTF-8·BOM decoding, record, final newline,
   bounded pipeline flow, redirection, short-circuit, 결과 우선순위, 부분 출력이
   [텍스트 record·stream 계약](TEXT_STREAM_MODEL.ko.md)을 따르는지 확인한다.
4. **셸 통로 테스트**: 지원 셸마다 상속된 현재 폴더, `PATH`, UTF-8, 스트리밍 출력, 취소를 확인한다.
5. **터미널 세션 테스트**: prompt 증거, Unicode 편집, completion fallback, paste,
   recall, foreground 입력, 셸 전환이
   [터미널 제출·세션 계약](TERMINAL_SESSION_CONTRACT.ko.md)을 따르는지 확인한다.
6. **업데이트 카나리**: 지원 Windows·셸 조합마다 대표 P0 흐름을 실행한다.

카나리에는 한국어 경로·공백 경로·숨김 파일·읽기 전용 파일을 포함한다. 단지 영문 ASCII 정상 경로가
아니라 Windows 경로와 UTF-8 전달을 검증한다.

## 분류

| 분류 | 처리 |
| --- | --- |
| Wingman 계약 결함 | runner 수정과 회귀 테스트 추가 |
| 셸 통로 결함 | 전달 shim 수정과 통합 테스트 추가 |
| Windows 환경 차이 | 지원 범위 검토 및 필요 시 안전 규칙 강화 |
| 네이티브 패스스루 동작 | Wingman P0 결함이 아님 |

## 변경 정책

- 버그 수정은 문서화된 계약을 복구하며 patch 릴리스가 될 수 있다.
- 새 명령·옵션은 계약 확장이므로 문서화된 기능 릴리스가 필요하다.
- 출력·종료 코드·안전 의미 변경은 계약 버전, 테스트, 릴리스 노트, 사용자 검토가 필요하다.
- 긴급 안전 제한은 더 빠르게 반영할 수 있지만, 이전 동작이 왜 위험했는지 기록해야 한다.

지원하지 않는 Windows·셸 버전에서 Wingman 실행을 막지는 않는다. 다만 best effort로 처리하며
릴리스 차단 테스트 대상은 아니다.

## 의존성 흐름

의존성 업그레이드는 P0 의미 변경과 분리한다. 후보마다 잠금, 재현 빌드, 터미널 회귀 테스트,
P0 카나리를 거친 뒤 채택한다.
