use std::fs::OpenOptions;
use std::io::Write;
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;
use wingman_lib::interpreter::ActiveShell;
use wingman_lib::transport::{
    parse_editor_readiness_frame, EditorAdapterCapabilityV1, EditorLocationKindV1,
    EditorReadinessBrokerV1,
};

#[test]
fn parser_accepts_only_the_bounded_versioned_readiness_shape() {
    let nonce = "abcdef0123456789abcdef0123456789";
    let frame = parse_editor_readiness_frame(&format!(
        "1;{nonce};17;powershell;0;filesystem;psreadline-replace-v1"
    ))
    .expect("parse readiness frame");

    assert_eq!(frame.nonce, nonce);
    assert_eq!(frame.sequence, 17);
    assert_eq!(frame.shell, ActiveShell::WindowsPowerShell);
    assert_eq!(frame.shell_depth, 0);
    assert_eq!(frame.location_kind, EditorLocationKindV1::FileSystem);
    assert_eq!(
        frame.adapter_capability,
        EditorAdapterCapabilityV1::PsReadLineReplaceV1
    );

    for invalid in [
        "",
        "2;abcdef0123456789abcdef0123456789;1;powershell;0;filesystem;psreadline-replace-v1",
        "1;short;1;powershell;0;filesystem;psreadline-replace-v1",
        "1;abcdef0123456789abcdef0123456789;0;powershell;0;filesystem;psreadline-replace-v1",
        "1;abcdef0123456789abcdef0123456789;1;bash;0;filesystem;psreadline-replace-v1",
        "1;abcdef0123456789abcdef0123456789;1;powershell;0;filesystem;unknown",
        "1;abcdef0123456789abcdef0123456789;1;powershell;0;filesystem;psreadline-replace-v1\r",
    ] {
        assert!(
            parse_editor_readiness_frame(invalid).is_err(),
            "accepted invalid frame {invalid:?}"
        );
    }
}

#[test]
fn authenticated_persistent_client_delivers_multiple_frames_in_order() {
    let nonce = Uuid::new_v4().as_simple().to_string();
    let pipe_name = readiness_pipe_name();
    let broker =
        EditorReadinessBrokerV1::start(&pipe_name, nonce.clone()).expect("start readiness broker");
    let mut client = connect(&pipe_name);
    for sequence in [1, 2] {
        writeln!(
            client,
            "1;{nonce};{sequence};powershell;0;filesystem;psreadline-replace-v1"
        )
        .expect("write readiness frame");
        client.flush().expect("flush readiness frame");
    }

    let frames = drain_until(&broker, 2);
    assert_eq!(
        frames
            .iter()
            .map(|frame| frame.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    drop(client);
    broker.stop().expect("stop readiness broker");
}

#[test]
fn wrong_nonce_and_early_disconnect_do_not_block_the_next_valid_client() {
    let nonce = Uuid::new_v4().as_simple().to_string();
    let pipe_name = readiness_pipe_name();
    let broker =
        EditorReadinessBrokerV1::start(&pipe_name, nonce.clone()).expect("start readiness broker");

    let mut wrong = connect(&pipe_name);
    writeln!(
        wrong,
        "1;00000000000000000000000000000000;1;powershell;0;filesystem;psreadline-replace-v1"
    )
    .expect("write wrong nonce");
    wrong.flush().expect("flush wrong nonce");
    drop(wrong);

    let mut valid = connect(&pipe_name);
    writeln!(
        valid,
        "1;{nonce};1;powershell;0;filesystem;psreadline-replace-v1"
    )
    .expect("write valid readiness");
    valid.flush().expect("flush valid readiness");

    let frames = drain_until(&broker, 1);
    assert_eq!(frames[0].sequence, 1);
    drop(valid);
    broker.stop().expect("stop readiness broker");
}

#[test]
fn silent_connected_client_does_not_block_shutdown() {
    let nonce = Uuid::new_v4().as_simple().to_string();
    let pipe_name = readiness_pipe_name();
    let broker = EditorReadinessBrokerV1::start(&pipe_name, nonce).expect("start readiness broker");
    let silent = connect(&pipe_name);

    let started = Instant::now();
    broker.stop().expect("stop readiness broker");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "silent readiness client blocked shutdown"
    );
    drop(silent);
}

#[test]
fn authenticated_idle_client_does_not_block_shutdown() {
    let nonce = Uuid::new_v4().as_simple().to_string();
    let pipe_name = readiness_pipe_name();
    let broker =
        EditorReadinessBrokerV1::start(&pipe_name, nonce.clone()).expect("start readiness broker");
    let mut client = connect(&pipe_name);
    writeln!(
        client,
        "1;{nonce};1;powershell;0;filesystem;psreadline-replace-v1"
    )
    .expect("authenticate readiness client");
    client.flush().expect("flush readiness authentication");
    assert_eq!(drain_until(&broker, 1)[0].sequence, 1);

    let started = Instant::now();
    broker.stop().expect("stop readiness broker");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "authenticated idle client blocked shutdown"
    );
    drop(client);
}

#[test]
fn duplicate_sequence_poisons_the_readiness_session() {
    let nonce = Uuid::new_v4().as_simple().to_string();
    let pipe_name = readiness_pipe_name();
    let broker =
        EditorReadinessBrokerV1::start(&pipe_name, nonce.clone()).expect("start readiness broker");
    let mut client = connect(&pipe_name);
    for _ in 0..2 {
        writeln!(
            client,
            "1;{nonce};1;powershell;0;filesystem;psreadline-replace-v1"
        )
        .expect("write duplicate readiness");
    }
    client.flush().expect("flush duplicate readiness");

    wait_for_poison(&broker);
    drop(client);
    broker.stop().expect("stop readiness broker");
}

#[test]
fn queue_overflow_poisons_instead_of_blocking_the_writer() {
    let nonce = Uuid::new_v4().as_simple().to_string();
    let pipe_name = readiness_pipe_name();
    let broker =
        EditorReadinessBrokerV1::start(&pipe_name, nonce.clone()).expect("start readiness broker");
    let mut client = connect(&pipe_name);
    for sequence in 1..=9 {
        writeln!(
            client,
            "1;{nonce};{sequence};powershell;0;filesystem;psreadline-replace-v1"
        )
        .expect("write readiness burst");
    }
    client.flush().expect("flush readiness burst");

    wait_for_poison(&broker);
    drop(client);
    broker.stop().expect("stop readiness broker");
}

fn readiness_pipe_name() -> String {
    format!(
        r"\\.\pipe\wingman-readiness-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    )
}

fn connect(pipe_name: &str) -> std::fs::File {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match OpenOptions::new().read(true).write(true).open(pipe_name) {
            Ok(client) => return client,
            Err(error) if Instant::now() < deadline => {
                let _ = error;
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("connect readiness pipe: {error}"),
        }
    }
}

fn drain_until(
    broker: &EditorReadinessBrokerV1,
    expected: usize,
) -> Vec<wingman_lib::transport::EditorReadinessFrameV1> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut frames = Vec::new();
    while Instant::now() < deadline {
        frames.extend(broker.drain().expect("drain readiness frames"));
        if frames.len() >= expected {
            return frames;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {expected} readiness frames");
}

fn wait_for_poison(broker: &EditorReadinessBrokerV1) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if broker.drain().is_err() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("readiness session was not poisoned");
}
