use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;
#[cfg(windows)]
use windows_sys::Win32::System::Console::{GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT};
#[cfg(windows)]
use windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP;
use wingman_lib::interpreter::{
    ExecutionPlanV1, PreparedRequestKindV1, PreparedRequestV1, RedirectModeV1, StagePlanV1,
    ValidatedRedirectPlanV1,
};
use wingman_lib::transport::OneShotBrokerV1;
use wingman_lib::windows_path::validate_path_value;

#[test]
fn runner_without_a_broker_fails_without_starting_the_gui() {
    let output = Command::new(env!("CARGO_BIN_EXE_wingman-runner"))
        .arg("0123456789abcdef0123456789abcdef")
        .env_remove("WINGMAN_BROKER_PIPE")
        .output()
        .expect("start packaged runner binary");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"wingman-runner: broker endpoint is unavailable\r\n"
    );
}

#[test]
fn runner_consumes_a_prepared_rejection_over_a_local_named_pipe() {
    let request_id = Uuid::new_v4().as_simple().to_string();
    let pipe_name = format!(
        r"\\.\pipe\wingman-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let broker = OneShotBrokerV1::bind(
        &pipe_name,
        request_id.clone(),
        PreparedRequestV1 {
            protocol: "wingman.run".to_string(),
            version: 1,
            kind: PreparedRequestKindV1::Reject {
                diagnostic: "wingman grep: unsupported option -z".to_string(),
                exit_code: 2,
            },
        },
    )
    .expect("bind one-shot broker");
    let server = thread::spawn(move || broker.serve());

    let output = Command::new(env!("CARGO_BIN_EXE_wingman-runner"))
        .arg(&request_id)
        .env("WINGMAN_BROKER_PIPE", &pipe_name)
        .output()
        .expect("start packaged runner binary");

    server
        .join()
        .expect("broker thread")
        .expect("serve prepared request");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, b"wingman grep: unsupported option -z\r\n");
}

#[test]
fn runner_inherits_the_launching_shell_working_directory() {
    let request_id = Uuid::new_v4().as_simple().to_string();
    let pipe_name = format!(
        r"\\.\pipe\wingman-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let broker = OneShotBrokerV1::bind(
        &pipe_name,
        request_id.clone(),
        PreparedRequestV1 {
            protocol: "wingman.run".to_string(),
            version: 1,
            kind: PreparedRequestKindV1::Execute {
                plan: ExecutionPlanV1 {
                    stages: vec![StagePlanV1::PrintWorkingDirectory],
                    redirect: None,
                },
            },
        },
    )
    .expect("bind one-shot broker");
    let server = thread::spawn(move || broker.serve());

    let inherited_cwd = std::env::temp_dir();
    let expected_cwd = inherited_cwd
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_string();
    let output = Command::new(env!("CARGO_BIN_EXE_wingman-runner"))
        .arg(&request_id)
        .env("WINGMAN_BROKER_PIPE", &pipe_name)
        .current_dir(&inherited_cwd)
        .output()
        .expect("start packaged runner binary");

    server
        .join()
        .expect("broker thread")
        .expect("serve prepared request");
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, format!("{expected_cwd}\r\n").as_bytes());
}

#[test]
fn runner_cannot_consume_an_expired_prepared_request() {
    let request_id = Uuid::new_v4().as_simple().to_string();
    let pipe_name = format!(
        r"\\.\pipe\wingman-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let broker = OneShotBrokerV1::bind_with_ttl(
        &pipe_name,
        request_id.clone(),
        PreparedRequestV1 {
            protocol: "wingman.run".to_string(),
            version: 1,
            kind: PreparedRequestKindV1::Reject {
                diagnostic: "must not be delivered".to_string(),
                exit_code: 2,
            },
        },
        Duration::ZERO,
    )
    .expect("bind expiring broker");
    let server = thread::spawn(move || broker.serve());

    let output = Command::new(env!("CARGO_BIN_EXE_wingman-runner"))
        .arg(&request_id)
        .env("WINGMAN_BROKER_PIPE", &pipe_name)
        .output()
        .expect("start packaged runner binary");

    let broker_error = server
        .join()
        .expect("broker thread")
        .expect_err("expired request must not be served");
    assert_eq!(broker_error.kind(), std::io::ErrorKind::TimedOut);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"wingman-runner: transport is unavailable\r\n"
    );
}

#[test]
fn runner_process_rejects_terminal_control_output_from_a_forged_request() {
    let request_id = Uuid::new_v4().as_simple().to_string();
    let pipe_name = format!(
        r"\\.\pipe\wingman-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let broker = OneShotBrokerV1::bind(
        &pipe_name,
        request_id.clone(),
        PreparedRequestV1 {
            protocol: "wingman.run".to_string(),
            version: 1,
            kind: PreparedRequestKindV1::Control {
                response: "safe\u{1b}[2Junsafe".to_string(),
                exit_code: 0,
            },
        },
    )
    .expect("bind forged request broker");
    let server = thread::spawn(move || broker.serve());

    let output = Command::new(env!("CARGO_BIN_EXE_wingman-runner"))
        .arg(&request_id)
        .env("WINGMAN_BROKER_PIPE", &pipe_name)
        .output()
        .expect("start packaged runner binary");

    server
        .join()
        .expect("broker thread")
        .expect("serve forged request");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"wingman-runner: prepared request was rejected\r\n"
    );
    assert!(!output.stderr.contains(&0x1b));
}

#[test]
fn runner_process_streams_cat_into_head_without_decoding_the_suffix() {
    let sandbox = std::env::temp_dir().join(format!(
        "wingman-runner-process-readonly-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    ));
    fs::create_dir(&sandbox).unwrap();
    let input = sandbox.join("input.txt");
    fs::write(&input, [b"first\n".as_slice(), &[0xff, 0xfe]].concat()).unwrap();

    let request_id = Uuid::new_v4().as_simple().to_string();
    let pipe_name = format!(
        r"\\.\pipe\wingman-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let broker = OneShotBrokerV1::bind(
        &pipe_name,
        request_id.clone(),
        PreparedRequestV1 {
            protocol: "wingman.run".to_string(),
            version: 1,
            kind: PreparedRequestKindV1::Execute {
                plan: ExecutionPlanV1 {
                    stages: vec![
                        StagePlanV1::ReadTextFiles {
                            paths: vec![validate_path_value(&input.to_string_lossy()).unwrap()],
                            number_lines: false,
                        },
                        StagePlanV1::HeadLines {
                            count: 1,
                            path: None,
                        },
                    ],
                    redirect: None,
                },
            },
        },
    )
    .expect("bind read-only request broker");
    let server = thread::spawn(move || broker.serve());

    let output = Command::new(env!("CARGO_BIN_EXE_wingman-runner"))
        .arg(&request_id)
        .env("WINGMAN_BROKER_PIPE", &pipe_name)
        .output()
        .expect("start packaged runner binary");

    server
        .join()
        .expect("broker thread")
        .expect("serve read-only request");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"first\r\n");
    assert!(output.stderr.is_empty());
    fs::remove_dir_all(&sandbox).unwrap();
}

#[cfg(windows)]
#[test]
fn runner_process_accepts_a_group_control_event_as_cancellation() {
    const RECORD: &[u8] = b"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n";
    const RECORD_COUNT: usize = 400_000;

    let sandbox = std::env::temp_dir().join(format!(
        "wingman-runner-process-cancel-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    ));
    fs::create_dir(&sandbox).unwrap();
    let input = sandbox.join("input.txt");
    let output_path = sandbox.join("output.txt");
    let mut input_writer = BufWriter::new(File::create(&input).unwrap());
    for _ in 0..RECORD_COUNT {
        input_writer.write_all(RECORD).unwrap();
    }
    input_writer.flush().unwrap();
    drop(input_writer);

    let request_id = Uuid::new_v4().as_simple().to_string();
    let pipe_name = format!(
        r"\\.\pipe\wingman-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let broker = OneShotBrokerV1::bind(
        &pipe_name,
        request_id.clone(),
        PreparedRequestV1 {
            protocol: "wingman.run".to_string(),
            version: 1,
            kind: PreparedRequestKindV1::Execute {
                plan: ExecutionPlanV1 {
                    stages: vec![StagePlanV1::ReadTextFiles {
                        paths: vec![validate_path_value(&input.to_string_lossy()).unwrap()],
                        number_lines: false,
                    }],
                    redirect: Some(ValidatedRedirectPlanV1 {
                        mode: RedirectModeV1::Overwrite,
                        path: validate_path_value(&output_path.to_string_lossy()).unwrap(),
                    }),
                },
            },
        },
    )
    .expect("bind cancellation request broker");
    let server = thread::spawn(move || broker.serve());

    let mut child = Command::new(env!("CARGO_BIN_EXE_wingman-runner"))
        .arg(&request_id)
        .env("WINGMAN_BROKER_PIPE", &pipe_name)
        .creation_flags(CREATE_NEW_PROCESS_GROUP)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start cancellable runner process");

    let output_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if fs::metadata(&output_path)
            .map(|metadata| metadata.len() > 0)
            .unwrap_or(false)
        {
            break;
        }
        if let Some(status) = child.try_wait().unwrap() {
            panic!("runner completed before cancellation with {status}");
        }
        assert!(
            Instant::now() < output_deadline,
            "runner did not begin streaming before the deadline"
        );
        thread::sleep(Duration::from_millis(5));
    }

    let generated = unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, child.id()) };
    assert_ne!(
        generated,
        0,
        "send CTRL_BREAK_EVENT to runner process group: {}",
        std::io::Error::last_os_error()
    );

    let exit_deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= exit_deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("runner did not stop after cancellation");
        }
        thread::sleep(Duration::from_millis(5));
    };
    let mut stderr = Vec::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_end(&mut stderr)
        .unwrap();
    let mut stdout = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut stdout)
        .unwrap();
    server
        .join()
        .expect("broker thread")
        .expect("serve cancellation request");

    let output_length = fs::metadata(&output_path).unwrap().len() as usize;
    let complete_length = RECORD_COUNT * (RECORD.len() + 1);
    assert_eq!(status.code(), Some(130));
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
    assert!(output_length > 0);
    assert!(output_length < complete_length);
    fs::remove_dir_all(&sandbox).unwrap();
}

#[cfg(windows)]
#[test]
fn idle_tail_follow_process_observes_group_cancellation() {
    let sandbox = std::env::temp_dir().join(format!(
        "wingman-runner-follow-cancel-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    ));
    fs::create_dir(&sandbox).unwrap();
    let input = sandbox.join("idle.log");
    fs::write(&input, b"").unwrap();

    let request_id = Uuid::new_v4().as_simple().to_string();
    let pipe_name = format!(
        r"\\.\pipe\wingman-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let broker = OneShotBrokerV1::bind(
        &pipe_name,
        request_id.clone(),
        PreparedRequestV1 {
            protocol: "wingman.run".to_string(),
            version: 1,
            kind: PreparedRequestKindV1::Execute {
                plan: ExecutionPlanV1 {
                    stages: vec![StagePlanV1::FollowFile {
                        count: 0,
                        path: validate_path_value(&input.to_string_lossy()).unwrap(),
                    }],
                    redirect: None,
                },
            },
        },
    )
    .expect("bind follow cancellation broker");
    let server = thread::spawn(move || broker.serve());

    let mut child = Command::new(env!("CARGO_BIN_EXE_wingman-runner"))
        .arg(&request_id)
        .env("WINGMAN_BROKER_PIPE", &pipe_name)
        .creation_flags(CREATE_NEW_PROCESS_GROUP)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start idle follow runner");

    server
        .join()
        .expect("broker thread")
        .expect("serve follow request");
    assert!(child.try_wait().unwrap().is_none());
    thread::sleep(Duration::from_millis(100));
    assert!(child.try_wait().unwrap().is_none());

    let generated = unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, child.id()) };
    assert_ne!(
        generated,
        0,
        "cancel idle follow process: {}",
        std::io::Error::last_os_error()
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("idle follow runner did not stop after cancellation");
        }
        thread::sleep(Duration::from_millis(5));
    };
    let mut stdout = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut stdout)
        .unwrap();
    let mut stderr = Vec::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_end(&mut stderr)
        .unwrap();

    assert_eq!(status.code(), Some(130));
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
    fs::remove_dir_all(&sandbox).unwrap();
}
