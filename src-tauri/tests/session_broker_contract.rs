use std::fs::OpenOptions;
use std::io::Write;
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use uuid::Uuid;
use wingman_lib::interpreter::{PreparedRequestKindV1, PreparedRequestV1};
use wingman_lib::transport::{fetch_prepared_request_channel, SessionBrokerV1};

#[test]
fn active_request_receives_one_broker_cancellation_signal() {
    let pipe_name = format!(
        r"\\.\pipe\wingman-session-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let request_id = Uuid::new_v4().as_simple().to_string();
    let broker = SessionBrokerV1::start(&pipe_name).expect("start session broker");
    broker
        .register(
            request_id.clone(),
            PreparedRequestV1 {
                protocol: "wingman.run".to_string(),
                version: 1,
                kind: PreparedRequestKindV1::Reject {
                    diagnostic: "cancel me".to_string(),
                    exit_code: 2,
                },
            },
        )
        .expect("register cancellable request");

    let channel = fetch_prepared_request_channel(pipe_name.as_ref(), &request_id)
        .expect("fetch request while retaining its cancellation channel");
    let (wire, cancellation) = channel.into_parts();
    assert!(!wire.is_empty());
    assert_eq!(
        broker
            .cancel_current_requests()
            .expect("signal current request"),
        1
    );
    assert!(cancellation.wait().expect("receive cancellation signal"));

    broker.stop().expect("stop session broker");
}

#[test]
fn stopping_a_session_signals_its_active_request_before_disconnect() {
    let pipe_name = format!(
        r"\\.\pipe\wingman-session-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let request_id = Uuid::new_v4().as_simple().to_string();
    let broker = SessionBrokerV1::start(&pipe_name).expect("start session broker");
    broker
        .register(
            request_id.clone(),
            PreparedRequestV1 {
                protocol: "wingman.run".to_string(),
                version: 1,
                kind: PreparedRequestKindV1::Reject {
                    diagnostic: "cancel on stop".to_string(),
                    exit_code: 2,
                },
            },
        )
        .expect("register active request");
    let channel = fetch_prepared_request_channel(pipe_name.as_ref(), &request_id)
        .expect("fetch active request");
    let (_, cancellation) = channel.into_parts();
    let waiter = thread::spawn(move || cancellation.wait());

    broker.stop().expect("stop session broker");

    assert!(waiter
        .join()
        .expect("cancellation waiter")
        .expect("receive cancellation before disconnect"));
}

#[test]
fn wrong_id_does_not_prevent_the_next_valid_one_shot_request() {
    let pipe_name = format!(
        r"\\.\pipe\wingman-session-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let request_id = Uuid::new_v4().as_simple().to_string();
    let broker = SessionBrokerV1::start(&pipe_name).expect("start session broker");
    broker
        .register(
            request_id.clone(),
            PreparedRequestV1 {
                protocol: "wingman.run".to_string(),
                version: 1,
                kind: PreparedRequestKindV1::Reject {
                    diagnostic: "prepared request reached".to_string(),
                    exit_code: 2,
                },
            },
        )
        .expect("register one-shot request");

    let wrong = Command::new(env!("CARGO_BIN_EXE_wingman-runner"))
        .arg("00000000000000000000000000000000")
        .env("WINGMAN_BROKER_PIPE", &pipe_name)
        .output()
        .expect("run wrong request");
    assert_eq!(wrong.status.code(), Some(2));
    assert_eq!(
        wrong.stderr,
        b"wingman-runner: transport is unavailable\r\n"
    );

    let valid = Command::new(env!("CARGO_BIN_EXE_wingman-runner"))
        .arg(&request_id)
        .env("WINGMAN_BROKER_PIPE", &pipe_name)
        .output()
        .expect("run valid request");
    assert_eq!(valid.status.code(), Some(2));
    assert_eq!(valid.stderr, b"prepared request reached\r\n");

    broker.stop().expect("stop session broker");
}

#[test]
fn a_silent_connected_client_cannot_block_session_shutdown() {
    let pipe_name = format!(
        r"\\.\pipe\wingman-session-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let broker = SessionBrokerV1::start(&pipe_name).expect("start session broker");
    let silent_client = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&pipe_name)
        .expect("connect silent named-pipe client");
    let (sender, receiver) = mpsc::channel();
    let stopper = thread::spawn(move || {
        let result = broker.stop();
        let _ = sender.send(result);
    });

    let stopped_promptly = receiver.recv_timeout(Duration::from_secs(2));
    let stopped_within_deadline = stopped_promptly.is_ok();
    drop(silent_client);
    let stop_result = match stopped_promptly {
        Ok(result) => result,
        Err(_) => receiver
            .recv_timeout(Duration::from_secs(6))
            .expect("broker eventually stops after client closes"),
    };
    stopper.join().expect("stop thread");

    assert!(
        stopped_within_deadline,
        "silent local client blocked broker shutdown"
    );
    stop_result.expect("clean broker shutdown");
}

#[test]
fn duplicate_registration_never_replaces_the_original_request() {
    let pipe_name = format!(
        r"\\.\pipe\wingman-session-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let request_id = Uuid::new_v4().as_simple().to_string();
    let broker = SessionBrokerV1::start(&pipe_name).expect("start session broker");
    broker
        .register(
            request_id.clone(),
            PreparedRequestV1 {
                protocol: "wingman.run".to_string(),
                version: 1,
                kind: PreparedRequestKindV1::Reject {
                    diagnostic: "original request".to_string(),
                    exit_code: 2,
                },
            },
        )
        .expect("register original request");
    assert_eq!(
        broker
            .register(
                request_id.clone(),
                PreparedRequestV1 {
                    protocol: "wingman.run".to_string(),
                    version: 1,
                    kind: PreparedRequestKindV1::Reject {
                        diagnostic: "replacement request".to_string(),
                        exit_code: 2,
                    },
                },
            )
            .expect_err("duplicate ID must be rejected")
            .kind(),
        std::io::ErrorKind::AlreadyExists
    );

    let output = Command::new(env!("CARGO_BIN_EXE_wingman-runner"))
        .arg(&request_id)
        .env("WINGMAN_BROKER_PIPE", &pipe_name)
        .output()
        .expect("run original request");
    assert_eq!(output.stderr, b"original request\r\n");
    broker.stop().expect("stop session broker");
}

#[test]
fn consumed_request_cannot_be_replayed_and_the_broker_keeps_serving() {
    let pipe_name = format!(
        r"\\.\pipe\wingman-session-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let first_id = Uuid::new_v4().as_simple().to_string();
    let second_id = Uuid::new_v4().as_simple().to_string();
    let broker = SessionBrokerV1::start(&pipe_name).expect("start session broker");
    for (request_id, diagnostic) in [(&first_id, "first request"), (&second_id, "second request")] {
        broker
            .register(
                request_id.clone(),
                PreparedRequestV1 {
                    protocol: "wingman.run".to_string(),
                    version: 1,
                    kind: PreparedRequestKindV1::Reject {
                        diagnostic: diagnostic.to_string(),
                        exit_code: 2,
                    },
                },
            )
            .expect("register request");
    }

    let first = run_runner(&pipe_name, &first_id);
    assert_eq!(first.stderr, b"first request\r\n");

    let replay = run_runner(&pipe_name, &first_id);
    assert_eq!(
        replay.stderr,
        b"wingman-runner: transport is unavailable\r\n"
    );

    let second = run_runner(&pipe_name, &second_id);
    assert_eq!(second.stderr, b"second request\r\n");
    broker.stop().expect("stop session broker");
}

#[test]
fn a_runner_disconnect_after_fetch_does_not_stop_later_requests() {
    let pipe_name = format!(
        r"\\.\pipe\wingman-session-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let abandoned_id = Uuid::new_v4().as_simple().to_string();
    let valid_id = Uuid::new_v4().as_simple().to_string();
    let broker = SessionBrokerV1::start(&pipe_name).expect("start session broker");
    for (request_id, diagnostic) in [
        (&abandoned_id, "abandoned request"),
        (&valid_id, "still serving"),
    ] {
        broker
            .register(
                request_id.clone(),
                PreparedRequestV1 {
                    protocol: "wingman.run".to_string(),
                    version: 1,
                    kind: PreparedRequestKindV1::Reject {
                        diagnostic: diagnostic.to_string(),
                        exit_code: 2,
                    },
                },
            )
            .expect("register request");
    }

    let mut abandoned = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&pipe_name)
        .expect("connect abandoning runner");
    abandoned
        .write_all(format!("{abandoned_id}\n").as_bytes())
        .expect("fetch abandoned request");
    abandoned.flush().expect("flush abandoned request ID");
    drop(abandoned);

    let valid = run_runner(&pipe_name, &valid_id);
    assert_eq!(valid.stderr, b"still serving\r\n");
    broker.stop().expect("stop session broker");
}

#[test]
fn expired_request_is_rejected_without_stopping_the_session_broker() {
    let pipe_name = format!(
        r"\\.\pipe\wingman-session-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let expired_id = Uuid::new_v4().as_simple().to_string();
    let valid_id = Uuid::new_v4().as_simple().to_string();
    let broker = SessionBrokerV1::start_with_ttl(&pipe_name, Duration::from_millis(20))
        .expect("start short-lived session broker");
    broker
        .register(
            expired_id.clone(),
            PreparedRequestV1 {
                protocol: "wingman.run".to_string(),
                version: 1,
                kind: PreparedRequestKindV1::Reject {
                    diagnostic: "must expire".to_string(),
                    exit_code: 2,
                },
            },
        )
        .expect("register expiring request");
    thread::sleep(Duration::from_millis(40));

    let expired = run_runner(&pipe_name, &expired_id);
    assert_eq!(
        expired.stderr,
        b"wingman-runner: transport is unavailable\r\n"
    );

    broker
        .register(
            valid_id.clone(),
            PreparedRequestV1 {
                protocol: "wingman.run".to_string(),
                version: 1,
                kind: PreparedRequestKindV1::Reject {
                    diagnostic: "still serving".to_string(),
                    exit_code: 2,
                },
            },
        )
        .expect("register valid request after expiry");
    let valid = run_runner(&pipe_name, &valid_id);
    assert_eq!(valid.stderr, b"still serving\r\n");
    broker.stop().expect("stop session broker");
}

fn run_runner(pipe_name: &str, request_id: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_wingman-runner"))
        .arg(request_id)
        .env("WINGMAN_BROKER_PIPE", pipe_name)
        .output()
        .expect("run packaged runner")
}
