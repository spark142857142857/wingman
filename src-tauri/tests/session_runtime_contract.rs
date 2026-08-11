use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;
use uuid::Uuid;
use wingman_lib::interpreter::ActiveShell;
use wingman_lib::session_runtime::{execute_terminal_input, TerminalExecutionOutcomeV1};
use wingman_lib::terminal_session::TerminalSessionV1;
use wingman_lib::transport::SessionBrokerV1;

#[test]
fn validated_pwd_dispatches_from_mirrored_input_through_the_real_runner() {
    let mut session = TerminalSessionV1::new(601, ActiveShell::WindowsPowerShell);
    let marker = format!(
        "\u{1b}]777;wingman-prompt;1;{};1;powershell;0;filesystem;psreadline-replace-v1\u{7}",
        session.integration_nonce()
    );
    assert_eq!(session.ingest_pty_output(&marker), "");
    let pipe_name = format!(
        r"\\.\pipe\wingman-runtime-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );

    let broker = SessionBrokerV1::start(&pipe_name).expect("start session broker");
    let mut terminal_wire = Vec::new();
    let outcome = execute_terminal_input(
        &mut session,
        ActiveShell::WindowsPowerShell,
        &broker,
        &mut terminal_wire,
        "pwd\r",
        true,
    )
    .expect("dispatch validated terminal input");
    let TerminalExecutionOutcomeV1::Prepared { request_id, .. } = outcome else {
        panic!("expected a prepared runner dispatch");
    };
    assert!(String::from_utf8(terminal_wire)
        .expect("UTF-8 terminal write")
        .ends_with(&format!(
            "Invoke-WingmanPrepared -RequestId '{request_id}'\r"
        )));
    let output = Command::new(env!("CARGO_BIN_EXE_wingman-runner"))
        .arg(&request_id)
        .env("WINGMAN_BROKER_PIPE", &pipe_name)
        .output()
        .expect("start packaged runner binary");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert_eq!(
        output.stdout,
        format!("{}\r\n", std::env::current_dir().unwrap().display()).as_bytes()
    );
    broker.stop().expect("stop session broker");
}

#[test]
fn familiar_control_runs_through_the_real_runner_and_reports_its_host_effect() {
    let mut session = TerminalSessionV1::new(602, ActiveShell::WindowsPowerShell);
    let marker = format!(
        "\u{1b}]777;wingman-prompt;1;{};1;powershell;0;filesystem;psreadline-replace-v1\u{7}",
        session.integration_nonce()
    );
    assert_eq!(session.ingest_pty_output(&marker), "");
    let pipe_name = format!(
        r"\\.\pipe\wingman-runtime-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let broker = SessionBrokerV1::start(&pipe_name).expect("start session broker");
    let mut terminal_wire = Vec::new();

    let outcome = execute_terminal_input(
        &mut session,
        ActiveShell::WindowsPowerShell,
        &broker,
        &mut terminal_wire,
        "familiar off\r",
        true,
    )
    .expect("dispatch familiar control");
    let TerminalExecutionOutcomeV1::Prepared {
        request_id,
        familiar_effect: Some(effect),
    } = outcome
    else {
        panic!("expected a prepared familiar control");
    };
    assert_eq!(effect.enabled(), Some(false));

    let output = Command::new(env!("CARGO_BIN_EXE_wingman-runner"))
        .arg(&request_id)
        .env("WINGMAN_BROKER_PIPE", &pipe_name)
        .output()
        .expect("start packaged runner binary");

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"Familiar: OFF\r\n");
    assert!(output.stderr.is_empty());
    broker.stop().expect("stop session broker");
}

#[test]
fn failed_pty_write_unregisters_the_prepared_request() {
    struct FlushFailureWriter {
        bytes: Vec<u8>,
    }

    impl Write for FlushFailureWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "simulated PTY failure",
            ))
        }
    }

    let mut session = TerminalSessionV1::new(603, ActiveShell::WindowsPowerShell);
    let marker = format!(
        "\u{1b}]777;wingman-prompt;1;{};1;powershell;0;filesystem;psreadline-replace-v1\u{7}",
        session.integration_nonce()
    );
    assert_eq!(session.ingest_pty_output(&marker), "");
    let pipe_name = format!(
        r"\\.\pipe\wingman-runtime-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let broker = SessionBrokerV1::start(&pipe_name).expect("start session broker");
    let mut writer = FlushFailureWriter { bytes: Vec::new() };

    let error = execute_terminal_input(
        &mut session,
        ActiveShell::WindowsPowerShell,
        &broker,
        &mut writer,
        "pwd\r",
        true,
    )
    .expect_err("PTY flush must fail");
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);

    let wire = String::from_utf8(writer.bytes).expect("UTF-8 terminal wire");
    let prefix = "Invoke-WingmanPrepared -RequestId '";
    let start = wire.find(prefix).expect("prepared invocation") + prefix.len();
    let request_id = &wire[start..start + 32];
    let output = Command::new(env!("CARGO_BIN_EXE_wingman-runner"))
        .arg(request_id)
        .env("WINGMAN_BROKER_PIPE", &pipe_name)
        .output()
        .expect("try unregistered request");
    assert_eq!(
        output.stderr,
        b"wingman-runner: transport is unavailable\r\n"
    );
    broker.stop().expect("stop session broker");
}

#[test]
fn reliable_cat_head_redirection_runs_through_the_real_broker_and_sidecar() {
    let sandbox = std::env::temp_dir().join(format!(
        "wingman-runtime-readonly-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    ));
    fs::create_dir(&sandbox).unwrap();
    let input = sandbox.join("입력 파일.txt");
    let output_path = sandbox.join("출력 파일.txt");
    fs::write(&input, "첫째\n둘째\n").unwrap();

    let mut session = TerminalSessionV1::new(604, ActiveShell::WindowsPowerShell);
    let marker = format!(
        "\u{1b}]777;wingman-prompt;1;{};1;powershell;0;filesystem;psreadline-replace-v1\u{7}",
        session.integration_nonce()
    );
    assert_eq!(session.ingest_pty_output(&marker), "");
    let pipe_name = format!(
        r"\\.\pipe\wingman-runtime-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let broker = SessionBrokerV1::start(&pipe_name).expect("start session broker");
    let mut terminal_wire = Vec::new();
    let line = format!(
        "cat \"{}\" | head -n 1 > \"{}\"\r",
        display_path(&input),
        display_path(&output_path)
    );

    let outcome = execute_terminal_input(
        &mut session,
        ActiveShell::WindowsPowerShell,
        &broker,
        &mut terminal_wire,
        &line,
        true,
    )
    .expect("dispatch reliable read-only pipeline");
    let TerminalExecutionOutcomeV1::Prepared { request_id, .. } = outcome else {
        panic!("expected a prepared read-only dispatch");
    };
    assert!(String::from_utf8(terminal_wire)
        .expect("UTF-8 terminal write")
        .ends_with(&format!(
            "Invoke-WingmanPrepared -RequestId '{request_id}'\r"
        )));

    let process = Command::new(env!("CARGO_BIN_EXE_wingman-runner"))
        .arg(&request_id)
        .env("WINGMAN_BROKER_PIPE", &pipe_name)
        .output()
        .expect("start packaged runner binary");

    assert_eq!(process.status.code(), Some(0));
    assert!(process.stdout.is_empty());
    assert!(process.stderr.is_empty());
    assert_eq!(fs::read(&output_path).unwrap(), "첫째\r\n".as_bytes());
    broker.stop().expect("stop session broker");
    fs::remove_dir_all(&sandbox).unwrap();
}

#[test]
fn reliable_find_pipeline_runs_through_the_real_broker_and_sidecar() {
    let sandbox = std::env::temp_dir().join(format!(
        "wingman-runtime-find-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    ));
    fs::create_dir(&sandbox).unwrap();
    fs::create_dir(sandbox.join("nested")).unwrap();
    fs::write(sandbox.join("one.ts"), b"").unwrap();
    fs::write(sandbox.join("nested").join("two.ts"), b"").unwrap();
    fs::write(sandbox.join("nested").join("note.txt"), b"").unwrap();

    let mut session = TerminalSessionV1::new(605, ActiveShell::WindowsPowerShell);
    let marker = format!(
        "\u{1b}]777;wingman-prompt;1;{};1;powershell;0;filesystem;psreadline-replace-v1\u{7}",
        session.integration_nonce()
    );
    assert_eq!(session.ingest_pty_output(&marker), "");
    let pipe_name = format!(
        r"\\.\pipe\wingman-runtime-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let broker = SessionBrokerV1::start(&pipe_name).expect("start session broker");
    let mut terminal_wire = Vec::new();
    let line = "find . -type f -name \"*.ts\" | wc -l\r";
    let outcome = execute_terminal_input(
        &mut session,
        ActiveShell::WindowsPowerShell,
        &broker,
        &mut terminal_wire,
        line,
        true,
    )
    .expect("dispatch reliable find pipeline");
    let TerminalExecutionOutcomeV1::Prepared { request_id, .. } = outcome else {
        panic!("expected a prepared find dispatch");
    };
    assert!(String::from_utf8(terminal_wire)
        .expect("UTF-8 terminal write")
        .ends_with(&format!(
            "Invoke-WingmanPrepared -RequestId '{request_id}'\r"
        )));

    let process = Command::new(env!("CARGO_BIN_EXE_wingman-runner"))
        .arg(&request_id)
        .env("WINGMAN_BROKER_PIPE", &pipe_name)
        .current_dir(&sandbox)
        .output()
        .expect("start packaged runner binary");
    assert_eq!(process.status.code(), Some(0));
    assert_eq!(process.stdout, b"2\r\n");
    assert!(process.stderr.is_empty());

    broker.stop().expect("stop session broker");
    fs::remove_dir_all(&sandbox).unwrap();
}

#[test]
fn reliable_mkdir_runs_through_the_real_broker_and_sidecar() {
    let sandbox = std::env::temp_dir().join(format!(
        "wingman-runtime-mkdir-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    ));
    fs::create_dir(&sandbox).unwrap();
    let target = sandbox.join("한글 폴더").join("nested");

    let mut session = TerminalSessionV1::new(606, ActiveShell::WindowsPowerShell);
    let marker = format!(
        "\u{1b}]777;wingman-prompt;1;{};1;powershell;0;filesystem;psreadline-replace-v1\u{7}",
        session.integration_nonce()
    );
    assert_eq!(session.ingest_pty_output(&marker), "");
    let pipe_name = format!(
        r"\\.\pipe\wingman-runtime-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let broker = SessionBrokerV1::start(&pipe_name).expect("start session broker");
    let mut terminal_wire = Vec::new();
    let line = format!("mkdir -p \"{}\"\r", display_path(&target));
    let outcome = execute_terminal_input(
        &mut session,
        ActiveShell::WindowsPowerShell,
        &broker,
        &mut terminal_wire,
        &line,
        true,
    )
    .expect("dispatch reliable mkdir");
    let TerminalExecutionOutcomeV1::Prepared { request_id, .. } = outcome else {
        panic!("expected a prepared mkdir dispatch");
    };
    assert!(String::from_utf8(terminal_wire)
        .expect("UTF-8 terminal write")
        .ends_with(&format!(
            "Invoke-WingmanPrepared -RequestId '{request_id}'\r"
        )));

    let process = Command::new(env!("CARGO_BIN_EXE_wingman-runner"))
        .arg(&request_id)
        .env("WINGMAN_BROKER_PIPE", &pipe_name)
        .output()
        .expect("start packaged runner binary");
    assert_eq!(process.status.code(), Some(0));
    assert!(process.stdout.is_empty());
    assert!(process.stderr.is_empty());
    assert!(target.is_dir());

    broker.stop().expect("stop session broker");
    fs::remove_dir_all(&sandbox).unwrap();
}

#[test]
fn reliable_touch_runs_through_the_real_broker_and_sidecar() {
    let sandbox = std::env::temp_dir().join(format!(
        "wingman-runtime-touch-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    ));
    fs::create_dir(&sandbox).unwrap();
    let target = sandbox.join("한글 파일.txt");

    let mut session = TerminalSessionV1::new(607, ActiveShell::WindowsPowerShell);
    let marker = format!(
        "\u{1b}]777;wingman-prompt;1;{};1;powershell;0;filesystem;psreadline-replace-v1\u{7}",
        session.integration_nonce()
    );
    assert_eq!(session.ingest_pty_output(&marker), "");
    let pipe_name = format!(
        r"\\.\pipe\wingman-runtime-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let broker = SessionBrokerV1::start(&pipe_name).expect("start session broker");
    let mut terminal_wire = Vec::new();
    let line = format!("touch \"{}\"\r", display_path(&target));
    let outcome = execute_terminal_input(
        &mut session,
        ActiveShell::WindowsPowerShell,
        &broker,
        &mut terminal_wire,
        &line,
        true,
    )
    .expect("dispatch reliable touch");
    let TerminalExecutionOutcomeV1::Prepared { request_id, .. } = outcome else {
        panic!("expected a prepared touch dispatch");
    };
    assert!(String::from_utf8(terminal_wire)
        .expect("UTF-8 terminal write")
        .ends_with(&format!(
            "Invoke-WingmanPrepared -RequestId '{request_id}'\r"
        )));

    let process = Command::new(env!("CARGO_BIN_EXE_wingman-runner"))
        .arg(&request_id)
        .env("WINGMAN_BROKER_PIPE", &pipe_name)
        .output()
        .expect("start packaged runner binary");
    assert_eq!(process.status.code(), Some(0));
    assert!(process.stdout.is_empty());
    assert!(process.stderr.is_empty());
    assert!(target.is_file());
    assert!(fs::read(&target).unwrap().is_empty());

    broker.stop().expect("stop session broker");
    fs::remove_dir_all(&sandbox).unwrap();
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
