use uuid::Uuid;
use wingman_lib::interpreter::{ActiveShell, FamiliarControlEffectV1};
use wingman_lib::session_runtime::{
    apply_familiar_effect, execute_terminal_input, write_session_input, SessionWriteOutcomeV1,
    TerminalExecutionOutcomeV1,
};
use wingman_lib::terminal_session::TerminalSessionV1;
use wingman_lib::transport::SessionBrokerV1;

#[test]
fn stale_session_input_writes_zero_bytes() {
    let mut writer = Vec::new();

    let outcome = write_session_input(42, 41, &mut writer, "whoami\r")
        .expect("stale input should be rejected without an I/O error");

    assert_eq!(outcome, SessionWriteOutcomeV1::Stale);
    assert!(writer.is_empty());
}

#[test]
fn current_session_input_is_written_exactly_once() {
    let mut writer = Vec::new();

    let outcome = write_session_input(42, 42, &mut writer, "whoami\r")
        .expect("current input should be written");

    assert_eq!(outcome, SessionWriteOutcomeV1::Written);
    assert_eq!(writer, b"whoami\r");
}

#[test]
fn native_submission_is_forwarded_exactly_once_by_the_atomic_input_path() {
    let pipe_name = unique_pipe_name();
    let broker = SessionBrokerV1::start(&pipe_name).expect("start broker");
    let mut session = TerminalSessionV1::new(42, ActiveShell::WindowsPowerShell);
    let mut writer = Vec::new();

    let outcome = execute_terminal_input(
        &mut session,
        ActiveShell::WindowsPowerShell,
        &broker,
        &mut writer,
        "whoami\r",
        true,
    )
    .expect("forward native input");

    assert_eq!(outcome, TerminalExecutionOutcomeV1::Native);
    assert_eq!(writer, b"whoami\r");
    broker.stop().expect("stop broker");
}

#[test]
fn prepared_submission_registers_before_one_fixed_editor_write() {
    let pipe_name = unique_pipe_name();
    let broker = SessionBrokerV1::start(&pipe_name).expect("start broker");
    let mut session = TerminalSessionV1::new(42, ActiveShell::WindowsPowerShell);
    let marker = format!(
        "\u{1b}]777;wingman-prompt;1;{};1;powershell;0;filesystem;psreadline-replace-v1\u{7}",
        session.integration_nonce()
    );
    assert_eq!(session.ingest_pty_output(&marker), "");
    let mut writer = Vec::new();

    let outcome = execute_terminal_input(
        &mut session,
        ActiveShell::WindowsPowerShell,
        &broker,
        &mut writer,
        "pwd\r",
        true,
    )
    .expect("execute prepared input");
    let TerminalExecutionOutcomeV1::Prepared {
        request_id,
        familiar_effect: None,
    } = outcome
    else {
        panic!("expected a prepared submission");
    };

    assert_eq!(
        writer,
        format!("pwd\u{18}\u{17}Invoke-WingmanPrepared -RequestId '{request_id}'\r").as_bytes()
    );
    broker.stop().expect("stop broker");
}

#[test]
fn familiar_control_reports_the_host_state_effect_after_the_fixed_write() {
    let pipe_name = unique_pipe_name();
    let broker = SessionBrokerV1::start(&pipe_name).expect("start broker");
    let mut session = TerminalSessionV1::new(42, ActiveShell::WindowsPowerShell);
    let marker = format!(
        "\u{1b}]777;wingman-prompt;1;{};1;powershell;0;filesystem;psreadline-replace-v1\u{7}",
        session.integration_nonce()
    );
    assert_eq!(session.ingest_pty_output(&marker), "");
    let mut writer = Vec::new();

    let outcome = execute_terminal_input(
        &mut session,
        ActiveShell::WindowsPowerShell,
        &broker,
        &mut writer,
        "familiar off\r",
        true,
    )
    .expect("execute familiar control");
    let TerminalExecutionOutcomeV1::Prepared {
        familiar_effect: Some(effect),
        ..
    } = outcome
    else {
        panic!("expected a prepared control effect");
    };

    assert_eq!(effect.enabled(), Some(false));
    broker.stop().expect("stop broker");
}

#[test]
fn familiar_state_changes_only_after_persistence_succeeds() {
    let mut enabled = true;
    let failure = apply_familiar_effect(FamiliarControlEffectV1::Set(false), &mut enabled, |_| {
        Err(std::io::Error::other("persistence failed"))
    });
    assert!(failure.is_err());
    assert!(enabled);

    let result =
        apply_familiar_effect(
            FamiliarControlEffectV1::Set(false),
            &mut enabled,
            |_| Ok(()),
        )
        .expect("persist familiar state");
    assert!(!result);
    assert!(!enabled);
}

fn unique_pipe_name() -> String {
    format!(
        r"\\.\pipe\wingman-session-input-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    )
}
