use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;
use wingman_lib::interpreter::ActiveShell;
use wingman_lib::session_runtime::{
    apply_familiar_effect, execute_terminal_input, TerminalExecutionOutcomeV1,
};
use wingman_lib::terminal_session::TerminalSessionV1;
use wingman_lib::transport::{EditorReadinessBrokerV1, EditorReadinessFrameV1, SessionBrokerV1};

#[test]
fn powershell_oob_readiness_reaches_the_real_runner_and_next_editor_cycle() {
    let integration_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("powershell_runner_transport.ps1");
    let mut session = TerminalSessionV1::new(701, ActiveShell::WindowsPowerShell);
    let nonce = session.integration_nonce().to_string();
    let readiness_pipe_id = unique_pipe_id("wingman-oob-readiness");
    let readiness_pipe_name = format!(r"\\.\pipe\{readiness_pipe_id}");
    let request_pipe_name = format!(r"\\.\pipe\{}", unique_pipe_id("wingman-oob-request"));
    let readiness = EditorReadinessBrokerV1::start(&readiness_pipe_name, nonce.clone())
        .expect("start readiness broker");
    let requests = SessionBrokerV1::start(&request_pipe_name).expect("start request broker");

    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("open PowerShell PTY");
    let mut command = CommandBuilder::new("powershell.exe");
    command.args([
        "-NoLogo",
        "-NoProfile",
        "-NoExit",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        ". $env:WINGMAN_INTEGRATION_SCRIPT",
    ]);
    command.env("WINGMAN_INTEGRATION_SCRIPT", integration_path);
    command.env("WINGMAN_SESSION_NONCE", &nonce);
    command.env("WINGMAN_READINESS_PIPE", &readiness_pipe_id);
    command.env("WINGMAN_RUNNER_PATH", env!("CARGO_BIN_EXE_wingman-runner"));
    command.env("WINGMAN_BROKER_PIPE", &request_pipe_name);
    command.cwd(std::env::current_dir().expect("current directory"));
    let mut child = pair
        .slave
        .spawn_command(command)
        .expect("spawn integrated PowerShell");
    let mut reader = pair.master.try_clone_reader().expect("clone PTY reader");
    let mut writer = pair.master.take_writer().expect("take PTY writer");
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = [0u8; 4096];
        while let Ok(count) = reader.read(&mut bytes) {
            if count == 0 || sender.send(bytes[..count].to_vec()).is_err() {
                break;
            }
        }
    });

    let first = receive_readiness(&readiness, 1);
    assert!(
        session.apply_editor_readiness(&first),
        "first readiness frame was not accepted"
    );
    let mut familiar_enabled = false;
    let outcome = execute_terminal_input(
        &mut session,
        ActiveShell::WindowsPowerShell,
        &requests,
        writer.as_mut(),
        "familiar on\r",
        familiar_enabled,
    )
    .expect("execute Familiar control");
    let TerminalExecutionOutcomeV1::Prepared {
        familiar_effect: Some(effect),
        ..
    } = outcome
    else {
        panic!("Familiar control did not produce its host effect");
    };
    apply_familiar_effect(effect, &mut familiar_enabled, |_| Ok(()))
        .expect("apply Familiar control effect");
    assert!(familiar_enabled);

    let second = receive_readiness(&readiness, 2);
    assert!(
        session.apply_editor_readiness(&second),
        "next readiness frame was not accepted"
    );
    let outcome = execute_terminal_input(
        &mut session,
        ActiveShell::WindowsPowerShell,
        &requests,
        writer.as_mut(),
        "pwd\r",
        familiar_enabled,
    )
    .expect("execute prepared pwd");
    assert!(matches!(
        outcome,
        TerminalExecutionOutcomeV1::Prepared { .. }
    ));
    let third = receive_readiness(&readiness, 3);
    assert!(
        session.apply_editor_readiness(&third),
        "third readiness frame was not accepted"
    );
    let expected_cwd = std::env::current_dir()
        .expect("current directory")
        .display()
        .to_string();
    let output = receive_output_containing(&receiver, &expected_cwd);

    let sandbox = std::env::temp_dir().join(format!(
        "wingman-oob-readonly-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    ));
    fs::create_dir(&sandbox).unwrap();
    let input = sandbox.join("입력 파일.txt");
    let redirected = sandbox.join("출력 파일.txt");
    fs::write(&input, "첫 줄\n둘째 줄\n").unwrap();
    let line = format!(
        "cat \"{}\" | head -n 1 > \"{}\"\r",
        input.display(),
        redirected.display()
    );
    let outcome = execute_terminal_input(
        &mut session,
        ActiveShell::WindowsPowerShell,
        &requests,
        writer.as_mut(),
        &line,
        familiar_enabled,
    )
    .expect("execute prepared read-only pipeline");
    assert!(matches!(
        outcome,
        TerminalExecutionOutcomeV1::Prepared { .. }
    ));
    let fourth = receive_readiness(&readiness, 4);
    assert!(
        session.apply_editor_readiness(&fourth),
        "fourth readiness frame was not accepted"
    );
    assert_eq!(fs::read(&redirected).unwrap(), "첫 줄\r\n".as_bytes());

    let count_output = sandbox.join("줄 수.txt");
    let line = format!(
        "wc -l \"{}\" > \"{}\"\r",
        input.display(),
        count_output.display()
    );
    let outcome = execute_terminal_input(
        &mut session,
        ActiveShell::WindowsPowerShell,
        &requests,
        writer.as_mut(),
        &line,
        familiar_enabled,
    )
    .expect("execute prepared wc line count");
    assert!(matches!(
        outcome,
        TerminalExecutionOutcomeV1::Prepared { .. }
    ));
    let fifth = receive_readiness(&readiness, 5);
    assert!(
        session.apply_editor_readiness(&fifth),
        "fifth readiness frame was not accepted"
    );
    assert_eq!(fs::read(&count_output).unwrap(), b"2\r\n");

    let tail_output = sandbox.join("tail-output.txt");
    let line = format!(
        "tail -n 1 \"{}\" > \"{}\"\r",
        input.display(),
        tail_output.display()
    );
    let outcome = execute_terminal_input(
        &mut session,
        ActiveShell::WindowsPowerShell,
        &requests,
        writer.as_mut(),
        &line,
        familiar_enabled,
    )
    .expect("execute prepared finite tail");
    assert!(matches!(
        outcome,
        TerminalExecutionOutcomeV1::Prepared { .. }
    ));
    let sixth = receive_readiness(&readiness, 6);
    assert!(
        session.apply_editor_readiness(&sixth),
        "sixth readiness frame was not accepted"
    );
    assert_eq!(fs::read(&tail_output).unwrap(), "둘째 줄\r\n".as_bytes());

    let grep_output = sandbox.join("grep-output.txt");
    let line = format!(
        "grep -n \"둘째\" \"{}\" > \"{}\"\r",
        input.display(),
        grep_output.display()
    );
    let outcome = execute_terminal_input(
        &mut session,
        ActiveShell::WindowsPowerShell,
        &requests,
        writer.as_mut(),
        &line,
        familiar_enabled,
    )
    .expect("execute prepared grep");
    assert!(matches!(
        outcome,
        TerminalExecutionOutcomeV1::Prepared { .. }
    ));
    let seventh = receive_readiness(&readiness, 7);
    assert!(
        session.apply_editor_readiness(&seventh),
        "seventh readiness frame was not accepted"
    );
    assert_eq!(fs::read(&grep_output).unwrap(), "2:둘째 줄\r\n".as_bytes());

    let _ = child.kill();
    drop(writer);
    requests.stop().expect("stop request broker");
    readiness.stop().expect("stop readiness broker");
    fs::remove_dir_all(&sandbox).unwrap();

    assert!(
        !output.contains("wingman-prompt"),
        "OOB readiness leaked into PTY output: {output:?}"
    );
}

fn unique_pipe_id(prefix: &str) -> String {
    format!(
        "{prefix}-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    )
}

fn receive_readiness(
    broker: &EditorReadinessBrokerV1,
    expected_sequence: u64,
) -> EditorReadinessFrameV1 {
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        for frame in broker.drain().expect("drain readiness broker") {
            if frame.sequence == expected_sequence {
                return frame;
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for readiness sequence {expected_sequence}");
}

fn receive_output_containing(receiver: &mpsc::Receiver<Vec<u8>>, needle: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut output = String::new();
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(remaining) {
            Ok(bytes) => {
                output.push_str(&String::from_utf8_lossy(&bytes));
                if output.contains(needle) {
                    return output;
                }
            }
            Err(_) => break,
        }
    }
    panic!("timed out waiting for {needle:?}; output: {output:?}");
}
