# Mutation·복수 operand 실행 계약 (초안)

상태: 통합 발견 C5를 해결하기 위한 합의된 설계 방향. 이 문서는 구현을 허가하지
않는다.

영문판: [MUTATION_EXECUTION_CONTRACT.md](MUTATION_EXECUTION_CONTRACT.md)

## 범위와 권위

이 계약은 P0 `mkdir`, `touch`, `cp`, `mv`, `rm`, stdout redirection의 global
preflight, operand 순서, 안전 실패, 실행 실패 뒤 계속 여부, staging, 취소, 진단,
부분 작업을 정한다.

Windows 경로 계약은 path form, identity, reparse point, root, containment, race의
기준이다. 명령 계약은 유효 operand·option을 정한다. 이 문서는 검증된 작업을 언제
시작할 수 있고 실행이 멈춘 뒤 무엇이 남는지 정한다.

## 핵심 구분

```text
ValidationOrSafetyFailure -> exit 2, 요청 안 mutation 없음
SafetyCannotBeEstablished -> exit 1, 요청 안 mutation 없음
OperationalRuntimeFailure -> exit 1, 문서화한 부분 작업 가능
UserCancellation          -> exit 130, 문서화한 부분 작업 가능
Success                    -> exit 0
```

요청은 모든 operand와 최종 redirection을 포함한 Wingman 소유 제출 한 줄 전체다.
안전하지 않은 operand 하나를 빼고 나머지만 실행하지 않는다.

## 두 단계 경계

### A단계: global preflight

첫 mutation 전에 runner가 요청 전체를 검증한다.

1. request·protocol·schema, 명령 grammar, option 조합, operand 수, pipeline 호환성
2. 모든 `ValidatedPathSpec`, effective destination, redirection target, root,
   containment, lexical alias 규칙
3. 필요한 existing-object identity, input·output alias, hard-link alias, ancestor,
   destination-inside-source, reparse 정책
4. 금지 reparse item, root, cwd ancestor, unsafe boundary가 없음을 확인할 만큼 모든
   recursive source·삭제 tree 검사
5. 작업 안전에 필요한 source·destination type과 시작 file handle

알려진 grammar·shape·안전 위반은 `2`이고 아무것도 바꾸지 않는다. Access, sharing,
offline storage, race 등으로 identity·reparse·ancestor·recursive 안전을 검사할 수
없으면 요청은 `1`로 끝나며 아무것도 바꾸지 않는다. 변하는 filesystem은 추측한
target으로 계속할 권한이 아니다.

Preflight에서 missing target 같은 일반 실행 사실을 볼 수 있다. 명령별 규칙을
B단계에 기록하지만 global safety를 약화하지 않는다. 예를 들어
`rm -f missing safe-file`은 missing operand를 건너뛸 수 있어도 두 번째 operand가
unsafe면 모든 삭제를 막는다.

### B단계: 순서 실행

Global preflight가 성공한 뒤 문서화한 operand 순서로 실행한다. Mutation할 수 있는
commit·descent·delete·create·open 바로 전에 모든 identity·reparse 전제를 다시
검사한다.

- Runtime safety mismatch는 남은 작업을 중단하고 `1`이다.
- 일반 operand 실행 실패는 기록하고 아래 명령 규칙이 허용하면 뒤의 독립 operand를
  계속 처리한다.
- 완료 작업을 transaction이나 자동 rollback 가능한 것으로 표현하지 않는다.
- 취소 뒤 새 operand를 시작하지 않는다.

## 안정된 순서와 진단

Top-level operand는 입력한 왼쪽부터 실행한다. Recursive traversal은 이름의
case-insensitive Unicode ordinal 순서와 case-sensitive ordinal tiebreaker를 쓰며,
삭제는 child를 parent보다 먼저 방문한다.

진단도 같은 안정 operand·traversal 순서를 따른다. 첫 실행 실패가 primary 진단이며
추가 실패는 크기를 제한하고 순서를 유지한다. 나중 cleanup·취소 부수 오류가 primary
원인을 대체하지 않는다.

문법·안전 종료 `2`는 상한 안에서 preflight 위반 여러 개를 알릴 수 있지만 B단계가
시작되지 않았으므로 완료 mutation을 주장하는 출력은 없다.

## `mkdir` 복수 operand 동작

- Operand는 왼쪽부터 실행한다.
- `-p` 없이 existing directory를 지정하면 그 operand 실행 실패 `1`이고 뒤 operand는
  계속한다.
- `-p`에서는 existing directory가 성공 no-op이다. Missing component는 parent부터
  child 순서로 만든다.
- Component 생성이 실패하면 해당 operand에서 이미 만든 component는 남는다. 그
  operand를 멈추고 다음 top-level operand를 처리한다.
- Directory가 필요한 곳의 existing file, access denial, name collision, lock, race는
  실행 실패 `1`이다.
- B단계 전 reparse 불확실성은 모든 mutation을 막고, B단계 중 reparse·identity
  변화는 남은 모든 작업을 `1`로 중단한다.

## `touch` 복수 operand 동작

- Runner는 preflight 뒤 UTC operation timestamp 하나를 잡고 모든 성공 operand에
  같은 timestamp를 쓴다.
- Operand는 왼쪽부터 실행한다. Missing leaf는 빈 regular file로 만들고 existing
  regular file은 captured `LastWriteTime`을 받는다.
- Existing file 내용은 truncate하거나 다시 쓰지 않는다.
- 한 operand 실패는 `1`로 기록하고 뒤 독립 operand를 계속한다.
- 새 file을 만든 뒤 timestamp 설정이 실패하면 file은 남지만 그 operand는 실패다.
- Directory, missing parent, reparse path, access denial, lock, race는 공통 path·실행
  규칙을 따른다.

## `cp` staging과 commit

`cp`는 source 하나와 effective destination 하나를 가진다. Preflight 뒤:

1. staging 전에 `-n` 적용; existing destination이면 성공 no-op
2. global temp가 아니라 검증한 destination parent 안에 예측하기 어려운 staging
   sibling 생성
3. complete file·recursive directory tree를 staging으로 복사·검증
4. 필요한 staging handle flush·close
5. source, parent, destination, identity, containment, reparse 전제 재검사
6. 가능한 가장 좁은 same-directory Windows rename·replace로 effective destination에
   staging commit

Commit 전 staging 실패는 existing destination을 건드리지 않는다. Staging은 best
effort로 지운다. Cleanup 실패는 application-owned staging item을 남길 수 있어
진단하지만 성공 destination으로 취급하지 않는다.

P0 recursive copy는 directory tree를 merge하지 않는다. Replace·merge될 existing
destination directory는 명령 계약의 reject·conflict다. `-f`는 교체 가능한
read-only·hidden destination attribute를 지우려고 시도할 수 있지만 ACL, sharing,
encryption, quota, volume 규칙을 우회하지 않는다.

Commit이 성공하면 `cp`는 완료다. 나중 진단 cleanup 실패 때문에 committed
destination을 지우지 않는다.

## `mv` commit과 cross-volume 부분 상태

Same-volume move는 마지막 identity·reparse 재검사 뒤 direct Windows rename·replace를
쓴다. Windows가 그 보장을 제공하는 범위에서 성공은 destination commit과 source
제거를 한 filesystem operation으로 수행한다.

Cross-volume move:

1. `cp`와 같은 staged copy·destination commit
2. destination commit 성공 뒤에만 검증된 non-following 규칙으로 source 제거
3. source 제거 실패 또는 commit 뒤 취소는 source·destination이 모두 남을 수 있고
   `1` 또는 `130`

Rollback처럼 보이게 committed destination을 지우지 않는다. Concurrent source
변경 뒤 유일한 complete copy를 잃을 수 있기 때문이다. 진단은 destination commit
확인 여부와 source 제거 미완료를 알린다.

`-n`은 copy·move 전에 skip한다. Destination commit 전 실패는 가능한 staging
cleanup artifact 외에는 source와 old destination을 바꾸지 않는다.

## `rm` 전체 target 안전과 부분 삭제

첫 삭제 전에 모든 target·recursive tree가 global safety 검사를 통과해야 한다. Root,
cwd·ancestor, forbidden path, reparse traversal, unknown identity, 검사 불가 safety
boundary 하나라도 있으면 어떤 target도 삭제하지 않는다.

Preflight 뒤:

- top-level target은 왼쪽부터 처리
- recursive target은 deterministic child-before-parent traversal
- explicit leaf reparse point는 link object만 삭제
- `-f`는 missing-target와 교체 가능한 attribute 동작만 바꾸며 ACL, sharing, lock,
  race, I/O 실패는 숨기지 않음
- `-f` 없이 missing이면 실행 `1`을 기록하고 뒤 safe target 계속
- `-f` missing은 성공 no-op
- target 하나의 runtime 실패는 이미 삭제한 entry를 남긴다. 안전이 허용하는 지점에서
  그 target을 멈추고, 실패가 global identity·reparse 상태를 무효화하지 않았을 때만
  다음 독립 target을 계속
- runtime safety mismatch는 남은 모든 삭제 중단

삭제는 Recycle Bin이 아닌 영구 삭제다. Undo log와 rollback 약속은 없다.

## Redirection mutation

Redirection은 text stream open 순서와 path 계약을 따른다.

- 모든 grammar·safety·identity·explicit input-open 검사를 먼저 끝냄
- 그 뒤 `>` create·truncate, `>>` create·append, 이후 stage output 시작
- output-open 실패면 stage 시작 안 함
- 나중 decoding·traversal·stage·sink·취소 실패는 empty·partial target을 남길 수 있음
- input identity와 같은 output alias는 target mutation 없이 `2`
- P0 redirection은 atomic final replacement staging을 하지 않음

## 취소

Traversal, copy, write, wait, commit 경계에서 cancellation을 확인한다.

- 취소를 받은 뒤 새 top-level operand를 시작하지 않는다.
- `mkdir`, `touch`, `rm`은 완료 mutation을 유지한다.
- Commit 전 `cp`·cross-volume `mv` staging tree는 best effort cleanup하고 이미 committed
  destination은 유지한다.
- Same-volume move는 filesystem commit 전 또는 후 상태로 관찰되며 Wingman이 쪼개지
  않는다.
- Terminal completion 발표 전 받은 취소는 `130`이며 shutdown의 secondary
  closed-handle 오류가 이를 바꾸지 않는다.

## 종료 집계

```text
global syntax 또는 known safety 위반       -> 2, mutation 없음
global safety 검사 불가                     -> 1, mutation 없음
완료 전 accepted cancellation               -> 130, 부분 작업 가능
B단계 실행 실패 하나 이상                   -> 1, 부분 작업 가능
모든 operand 성공 또는 문서화된 no-op       -> 0
```

복수 operand 명령은 모든 operand가 성공하거나 `rm -f` missing, `mkdir -p` existing
directory, `cp -n` skip 같은 문서화 no-op여야 성공이다. 뒤 성공이 앞 실행 실패를
숨기지 않는다.

## 필수 검증 matrix

테스트는 적어도 다음을 다룬다.

1. safe operand와 뒤 syntax·wildcard·root·cwd ancestor·same-file·inside-source·금지
   reparse operand를 함께 넣어 mutation이 없음을 증명
2. 실행 전 identity·reparse 검사 불가가 `1`·no mutation임을 증명
3. 첫·중간·마지막 실행 실패가 있는 left-to-right `mkdir`, `touch`, `rm`, 안정 진단,
   계속 처리, 최종 상태
4. `mkdir -p` 부분 component·collision, captured `touch` timestamp 하나, new-file
   timestamp 실패, existing content 불변
5. file·recursive `cp` staging 성공, copy·flush·cleanup 실패, destination race, `-n`,
   `-f`, existing destination, no merge
6. 가능한 same-volume atomic move, cross-volume copy 실패, destination commit,
   source-delete 실패, commit 전후 cancellation, 양쪽 copy 진단
7. 모든 target·tree의 `rm` preflight, deterministic traversal, `-f` 유무 missing,
   explicit reparse leaf, mid-tree ACL·lock·race 실패, 계속 처리와 global safety stop,
   영구 부분 삭제
8. target open 전 redirection missing input, target-open 실패, 나중 실패 뒤 empty target,
   partial write, same-identity 거부, cancellation
9. 모든 stage 경계 cancellation과 정확한 `0/1/2/130` 집계
10. staging 이름·cleanup artifact·진단에 request secret이나 필요한 operand 이름 밖의
    user-data copy가 없는지 확인
