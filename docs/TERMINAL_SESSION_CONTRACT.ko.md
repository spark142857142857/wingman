# 터미널 제출·세션 계약 (초안)

상태: 통합 발견 C3과 C7을 해결하기 위한 합의된 설계 방향. 이 문서는 구현을
허가하지 않는다.

영문판: [TERMINAL_SESSION_CONTRACT.md](TERMINAL_SESSION_CONTRACT.md)

## 범위와 권위

이 계약은 Wingman이 제출된 줄을 언제 해석할 수 있는지, 네이티브 줄 편집을 어떻게
거울처럼 추적하는지, 불확실한 입력 뒤에 어떻게 물러나는지, 활성 `cmd.exe` 또는
Windows PowerShell 5.1 세션을 어떻게 추적하는지 정한다. 터미널 입력, completion
fallback, 붙여넣기, 네이티브 history fallback, prompt 동기화, 중첩 prompt, interrupt의 공통
기준이다.

셸은 자체 editor, 네이티브 history, prompt, foreground program, process 상태를
계속 소유한다. Wingman은 검증된 셸 prompt에서 신뢰성 있게 복원한 한 줄만 가로챌
수 있다.

## 절대 원칙

> Wingman이 활성 prompt와 제출된 줄을 모두 증명할 수 없으면 입력을 터미널
> 세션에 전달하고 해석하거나 교체하지 않는다.

터미널 출력을 긁어 명령줄을 추측하지 않는다. prompt처럼 보이는 문자열은 증거가
아니다. 알 수 없는 편집에서 최선 추정 줄을 만들지 않는다.

## 세션 상태

```text
TerminalSessionState =
    AwaitingPrompt { expected_root_shell }
  | Editing {
      shell, shell_depth, command_sequence,
      buffer, cursor, evidence
    }
  | Running { shell, shell_depth, command_sequence }
  | Suspended { reason }
  | Closed

LineEvidence = Reliable | Uncertain { reason }
```

- `AwaitingPrompt`는 시작, interrupt 뒤, 셸 전환 확인 대기에 쓴다.
- `Editing/Reliable`에서만 공통 해석기를 호출할 수 있다.
- `Editing/Uncertain`은 네이티브 입력을 계속 전달하지만 같은 제출 중 다시
  reliable이 될 수 없다.
- `Running`은 제출한 네이티브 명령, Wingman runner, continuation prompt,
  foreground interactive program을 모두 포함한다.
- `Suspended`는 셸 identity나 integration 증거가 없다는 뜻이다. Familiar가
  설정상 켜져 보일 수는 있지만 유효한 동기화 event나 새 세션 전까지 interception은
  멈춘다.

`Running`, `Suspended`, `Closed`에서는 Wingman 창 자체 shortcut을 제외한 모든
터미널 입력이 네이티브 통과다. 특히 `ssh`, REPL, editor, pager 등 foreground
program 입력을 Wingman 명령으로 판정하지 않는다.

## Prompt 동기화 증거

목표 구조는 Windows PowerShell 5.1에 최소한의 패키지 shell-integration hook을
쓴다. Hook은 세션 전용 named pipe로 editor-readiness frame을 보내며, PTY
출력에서는 readiness를 추론하지 않는다. P0의 `cmd.exe`에는 의도적으로 신뢰
editor hook을 두지 않으며 모든 입력을 네이티브로 통과시킨다. 유효 PowerShell
readiness frame은 다음을 담는다.

```text
EditorReadiness {
  protocol_version,
  session_nonce,
  command_sequence,
  shell: WindowsPowerShell,
  shell_depth,
  location_kind: FileSystem | NonFileSystem,
  adapter_capability: PsReadLineReplaceV1
}
```

Readiness broker는 크기가 고정된 ASCII frame만 받고 session nonce를 인증하며
queue 크기를 제한하고 duplicate·replay sequence를 거부한다. 현재 session
nonce와 정확한 예상 sequence가 맞을 때만 세션 상태를 바꾼다. Worker는 app이나
terminal lock을 잡지 않고, `handle_terminal_input`이 활성 session lock 안에서
inbox를 drain한다. 일반 PTY OSC·CSI 데이터에는 production readiness 권한이 없다.

**현재 cutover 상태(2026-08-09):** OOB 채널은 production PowerShell session에
연결됐고, 반복 ConPTY PowerShell → readiness → Rust 판정 → request broker →
실제 sidecar → 다음 readiness 수직 테스트를 통과했다. Production에서는 PTY
readiness parser를 명시적으로 끈다. Familiar 기본값은 `PAUSED`로 유지하지만 명시적
`familiar on`은 이제 pipeline과 최종 output redirection을 포함한 입증된 `cat`·`head`·유한 `tail -n N`·단일 파일 `tail -f`·`wc -l`·`grep`
read-only slice를 활성화한다. Familiar OFF, Uncertain editing, `cmd`는 native를 유지한다.
앞선 경계 테스트에서
폐기한 `prompt` PTY hook과 PTY에 쓰는 `PSConsoleHostReadLine` wrapper는 사용하지
않는다.

Readiness 도착 전에 입력 한 바이트라도 native로 전달했다면 그 editor cycle은
끝까지 dirty이며 늦은 frame으로 `Reliable`에 올라갈 수 없다. Queue overflow,
인증 뒤 malformed frame, replay, worker 실패, connect 실패, 제한 시간 안에
끝나지 않은 PowerShell write는 모두 interception을 중단하고 native editor 경로를
보존한다.

Marker는 우연한 모호성을 막는 동기화 증거이지, 같은 사용자 권한으로 이미 실행
중인 악성 process에 대한 sandbox 경계는 아니다. Integration hook은 보호된 패키지
코드에서 설치하고 사용자의 보이는 prompt 동작을 보존하며 쓰기 가능한 임시
profile을 사용하지 않아야 한다.

제출 뒤 Wingman은 `Running`으로 간다. 다음 유효 readiness frame 또는 아래에서
정한 인증된 동일 process 중첩 depth frame만 새 빈 `Editing/Reliable` 상태를
시작한다. Timeout, prompt처럼 보이는 글자, 터미널 침묵은 근거가 아니다.

## 입력 mirror와 Unicode

`Editing` 동안 일반 입력은 네이티브 line editor로 계속 보내고, 크기가 제한된
mirror가 같은 확정 편집 연산을 기록한다. Mirror는 증거이지 두 번째 셸 editor가
아니다.

- 브라우저·IME pre-edit 문자열은 확정 text로 전달하거나 기록하지 않는다.
  composition 최종 결과만 정확히 한 번 전달하고 삽입한다.
- 유효한 확정 Unicode는 NFC/NFD 정규화 없이 보존한다.
- Mirror는 JavaScript UTF-16 code unit이나 터미널 화면 cell을 index로 사용하면
  안 된다. Text boundary 동작은 지원 셸 adapter와 일치해야 한다.
- 한글 음절·자모, combining mark, surrogate-pair 문자, emoji, double-width CJK는
  필수 경계 기술 검증·승인 사례다. Non-BMP text 삽입 자체는 보존하지만 non-BMP
  scalar를 건드리거나 가로지르는 편집은 `Uncertain`으로 바꾼다. Windows PowerShell
  5.1 PSReadLine이 UTF-16 surrogate 한쪽만 편집할 수 있어 Rust scalar index와
  정확히 일치시킬 수 없기 때문이다.
- Mirror와 네이티브 editor의 편집 경계를 정확히 일치시킬 수 없으면 evidence를
  `Uncertain`으로 만들며, 줄을 추측해 복구하지 않는다.
- Focus report, bracketed-paste delimiter, allowlist에 든 터미널 protocol 응답은
  명령 text가 아니라 transport event다.

Mirror는 UTF-8 입력 줄을 최대 16 KiB 보관한다. 이 상한을 넘으면 mirror를 멈추고
그 제출은 네이티브로 통과시킨다. 줄을 자르거나 재해석하거나 일부 실행하지 않는다.

## 편집 allowlist와 불확실 상태

`Reliable` 증거를 유지할 수 있는 것은 검증된 Windows PowerShell adapter뿐이며
`cmd.exe` 입력은 항상 네이티브다. 해당 adapter의 P0 allowlist는 다음과 같다.

- 현재 cursor 위치에 확정 text 삽입
- 대상 scalar가 UTF-16 code unit 하나인 Backspace와 Delete
- non-BMP scalar를 가로지르지 않는 Left와 Right
- Home과, mirrored line 안에서 실제로 이동하는 End
- `Ctrl+C` 줄 취소

Adapter는 normal/application cursor-key encoding의 효과가 같고 테스트됐을 때만
이를 인식할 수 있다. 그 밖의 편집·control은 네이티브 우선이다. 줄 끝의 Right·End,
Up/Down recall, Tab completion, prediction 수락, reverse/history search,
F7/F8/F9, Ctrl+Arrow, Alt 조합, mouse
위치 변경, selection 교체, Vi command mode, 사용자 PSReadLine binding, 알 수 없는
CSI/SS3/OSC 입력, 미완성·과대 escape sequence는 evidence를 `Uncertain`으로 만들고
원문 그대로 전달한다.

한 번 uncertain이 되면 뒤 입력이 단순해 보여도 해당 제출은 계속 uncertain이다.
Enter에서는 `prepare_submission`을 호출하지 않고 네이티브 editor의 실제 buffer를
받아들이게 한다. 다음 유효 readiness frame이나 세션 재시작 때만 reliability가
돌아온다.

## 제출 알고리즘

일반 편집 입력은 이미 네이티브 editor에 도달했다. `Editing/Reliable`에서만 Enter를
잠깐 보류해 Rust가 소유권을 정한다.

```text
Enter
  -> Editing/Reliable이 아니거나 Familiar OFF
       Enter만 전달; Running으로 전환
  -> Editing/Reliable
       prepare_submission(session, command_sequence, shell, mirrored_line)
         -> PassThrough { raw_line }
              mirror와 정확히 같은지 확인
              Enter만 전달; Running으로 전환
         -> InvokePrepared { request_id, display_line }
              mirror/display 일치와 유효 request 확인
              검증한 shell adapter로 알려진 native edit buffer를
              고정 runner 호출로 교체
              한 번 제출; Running으로 전환
```

`PassThrough.raw_line`은 일관성 확인 값이다. 네이티브 editor가 이미 줄을 가지고
있으므로 프론트엔드는 줄을 다시 보내지 않는다. 준비된 Reject, Execute, Familiar
control에서는 고정 설치 runner 경로, 고정 transport field, 일회용 request ID만
줄을 대체한다. 사용자 text는 셸 source가 되지 않는다.

Buffer 교체와 제출은 직렬화된 adapter operation 하나로 수행한다. 교체 시작 전에
session, sequence, line, request 검사가 실패하면 request를 무효화하고 원래 줄에
네이티브 Enter를 보낸다. 교체 시작 뒤 실패하면 interception을 중단하고 크기가
제한된 내부 오류를 보인다. 재시도하거나 두 번째 명령을 이어 붙이거나 원래 줄이
실행됐다고 가장하지 않는다.

경계 기술 검증은 지원하는 모든 cursor 위치, Unicode·wide text에서 prompt 내용을
지우거나 runner 첫 출력을 잃지 않고 교체함을 증명해야 한다. 짧은 고정 내부 호출이
화면에 보이는 것은 안전한 fallback으로 허용한다. 불안정한 출력 filtering은 필수가
아니다.

## Completion과 네이티브 history

셸 completion과 prediction은 editor 내용을 임의로 바꿀 수 있으므로 사용한 제출은
항상 네이티브 통과한다. Wingman은 화면 출력을 파싱해 completion 결과를 복원하지
않는다.

P0에는 Wingman 소유 command recall 목록이 없다. Up/Down과 모든 history search는
네이티브 셸로 전달하고 현재 제출을 uncertain으로 만든다. 활성 셸은 사용자가 설정한
기존 history 동작을 유지한다. Wingman은 네이티브 history를 지우거나 이동하거나
끄지 않고 비밀 저장소라고 약속하지 않는다. 그 history에는 불투명한 내부 runner
호출이 들어갈 수 있다.

영구 Wingman command history는 P0 밖이며 명시적 opt-in·보존·삭제 control이
필요하다. 세션 재시작은 터미널의 메모리 display와 scrollback을 지우지만 네이티브
셸 history는 바꾸지 않는다.

## 붙여넣기 계약

Clipboard text는 신뢰하지 않는 입력이며 전용 paste 경로를 사용한다.

- CR·LF가 없는 붙여넣기는 즉시 삽입하지만 자체적으로 제출하지 않는다. 일반 확정
  text는 reliable을 유지할 수 있지만 control 문자나 adapter가 mirror할 수 없는
  편집은 uncertain으로 만든다.
- CR이나 LF가 하나라도 있으면, 끝의 줄바꿈 하나뿐이어도 어떤 byte도 PTY에 보내기
  전에 보류한다. 논리적 줄 수와 함께 간결한 Send/Cancel 확인을 한 번 보인다.
- Cancel은 네이티브 edit buffer를 바꾸지 않는다.
- Send는 text와 줄 순서를 보존하고 지원 셸의 정상 입력 형태로 줄 경계를 encoding해
  하나의 네이티브 paste operation으로 보낸다. Wingman은 붙여넣은 각 줄을 나누거나
  판정하거나 변환하거나 따로 실행하지 않는다.
- 줄바꿈 paste를 확인해 보낸 뒤에는 유효 prompt marker가 새 편집 상태를 만들 때까지
  interception을 중단한다.
- 셸 adapter가 지원하고 증명한 bracketed-paste wrapper는 transport metadata이며
  명령 text로 저장하지 않는다.

따라서 여러 줄 paste는 명시적 확인 뒤 네이티브 명령을 실행할 수 있지만, 붙여넣은
block을 Wingman 소유 작업 여러 개로 몰래 바꾸지 않는다.

## 셸 전환

지원 터미널 셸 종류는 `cmd.exe`와 Windows PowerShell 5.1이지만, P0에서
`Editing/Reliable`로 들어갈 수 있는 것은 검증된 PowerShell adapter뿐이다.
`cmd`나 다른 foreground child에 들어가면 Familiar interception을 중단하고 child의
모든 입력을 네이티브로 전달한다. 패키지 adapter는 child 환경에서 readiness pipe와
nonce를 제거하므로 child `powershell.exe`, `cmd.exe`, `pwsh`, `wsl`, `bash`, `ssh`,
언어 REPL, alias, script, profile은 prompt를 인증하거나 Wingman 소유 편집 상태를
얻을 수 없다.

P0는 `$host.EnterNestedPrompt()` 등 Windows PowerShell 동일 process 안의 인증된
중첩 prompt만 추적한다. 첫 유효 readiness frame은 depth `0`이어야 하고 이후 frame은
마지막으로 검증한 depth에서 한 단계만 오르내릴 수 있으며 depth `16`을 넘을 수 없다.
Depth는 persistent out-of-band adapter가 다음 올바른 sequence frame을 보낼 때만
바뀐다. Prompt처럼 보이는 PTY 출력에는 권한이 없다.

인증된 동일 process 중첩 prompt의 네이티브 `exit`은 인접한 낮은 depth의 유효
frame이 도착한 뒤에만 부모로 돌아간 것으로 확정한다. Root 셸 exit은 process 종료로
확인하고 Wingman 세션을 닫는다. 일반 foreground child가 끝난 뒤에는 원래 부모
adapter만 이전에 인증한 depth에서 편집 상태를 다시 열 수 있다. 명령 철자나 화면
prompt를 보고 셸 또는 depth를 추측하지 않는다.

## Interrupt·재시작·오래된 작업

- `Editing`에서 `Ctrl+C`는 전달하고 mirror를 버린 뒤 새 유효 prompt marker를
  기다린다.
- Wingman runner 실행 중 `Ctrl+C`는 runner 취소 계약도 따르며 prompt 확인 전까지
  편집 상태로 돌아가지 않는다.
- 네이티브 또는 알 수 없는 foreground 작업 중 interrupt 입력은 그대로 전달한다.
- 세션 재시작은 새 session nonce를 만들고 셸 stack과 터미널 display를 비우며 준비 중
  작업을 취소하고 request ID를 무효화하고 모든 이전 PTY event·marker를 무시한다.
- 셸·process 종료, PTY write 실패, 잘못된 integration 상태, sequence mismatch는
  마지막으로 알던 줄이나 prompt를 재사용하지 못한다.

## 보안·개인정보 규칙

- 원문 입력, IME text, paste 내용, mirrored buffer, 네이티브 history, PTY 출력은
  production log나 telemetry에 기록하지 않는다.
- Rust만 세션 상태, marker 검증, 준비, request 무효화를 소유한다. WebView는 prompt나
  줄이 reliable이라고 주장할 수 없다.
- 모든 입력·출력·marker·decision에 활성 session ID를 붙이고 stale event를 버린다.
- Marker, escape, 입력 줄, paste, pending request 저장량에 상한을 둔다.
- 터미널 출력 escape sequence는 Tauri 명령 호출, clipboard 접근, 사용자 줄을
  reliable로 만드는 기능을 가질 수 없다.
- Familiar OFF는 준비 과정을 끄지만 안전한 paste 확인과 세션 격리는 끄지 않는다.

## 필수 검증 matrix

최종 앱 test는 Windows PowerShell 5.1에서 다음 전체 matrix를 다룬다.

1. read 사이에서 분리된 prompt marker, 출력과 합쳐진 marker, stale·malformed·replay,
   잘못된 셸·depth, child의 prompt 유사 출력
2. ASCII, 한글 IME, 자모·combining text, CJK width, emoji·surrogate pair, 중간 삽입,
   Backspace, Delete, Home/End, cursor 이동
3. Tab completion, prediction, Ctrl+R, F7/F8/F9, 사용자·알 수 없는 escape 입력,
   필수 네이티브 통과 fallback
4. 네이티브 Up/Down·history search fallback, buffer 교체, 세션 display 초기화,
   네이티브 history에 들어갈 수 있는 불투명 호출
5. 한 줄 paste, CR/LF/CRLF와 끝 newline paste, Send·Cancel, 순서, bracketed paste,
   foreground program 실행 중 paste
6. 네이티브 통과, continuation 입력, 테스트 interactive child, full-screen 형태 child,
   다음 유효 prompt 전 interception 금지
7. 인증된 동일 process PowerShell 중첩 depth push/pop, depth 상한·jump 거부,
   foreground `cmd`·child 네이티브 통과, 중첩 `exit`, frame 누락, 부모 복귀, root exit
8. Familiar ON/OFF, 준비된 Reject·Execute·Control 교체, line·sequence mismatch,
   PTY write 실패, 세션 재시작, `Ctrl+C`

Prompt protocol, Unicode-safe mirror, 보수적 fallback, 고정 호출 교체를 경계 기술
검증이 증명하기 전에는 명령 migration을 시작할 수 없다. 필수 동작을 증명하지
못하면 heuristic interception을 출시하지 않고 그 동작을 네이티브 통과로 좁힌다.

2026-08-08 `cmd.exe` 경계 기술 검증 결과에 따라 P0 범위를 수정했다. 네이티브
`PROMPT`는 고정 marker를 출력할 수 있지만 prompt마다 증가하는 sequence와 중첩
셸 depth를 증명할 수 없고, 사용자의 prompt 변경으로 marker가 사라질 수 있다.
따라서 `cmd` 합격 test는 정확한 원문 보존, stale session 거부, paste 안전성,
foreground 입력, Familiar interception 부재를 검증한다. Marker 기반 mirror나
editor 교체는 검증 대상이 아니다.

## 플랫폼 참고 자료

Microsoft는 pseudoconsole host가 사용자 입력 수집과 출력 표시를 담당하고 그 통로가
UTF-8을 사용한다고 문서화한다:
[Pseudoconsoles](https://learn.microsoft.com/en-us/windows/console/pseudoconsoles).

Windows는 console input에 virtual-terminal sequence가 들어갈 수 있고 sequence가
write 사이에서 나뉠 수 있다고도 문서화한다:
[Console Virtual Terminal Sequences](https://learn.microsoft.com/en-us/windows/console/console-virtual-terminal-sequences).

PowerShell의 네이티브 PSReadLine history는 host별 파일에 저장될 수 있고 기본값은
점진 저장일 수 있다. 따라서 Wingman은 모든 셸 history가 세션 메모리에만 있다고
설명할 수 없다:
[Set-PSReadLineOption](https://learn.microsoft.com/en-us/powershell/module/PSReadline/set-psreadlineoption?view=powershell-5.1).
