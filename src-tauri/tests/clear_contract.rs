use std::process::Command;
use std::thread;
use uuid::Uuid;
use wingman_lib::catalog::{build_readonly_plan, CatalogErrorV1};
use wingman_lib::interpreter::{
    ActiveShell, ExecutionPlanV1, FrontendDecisionKindV1, InterpreterSession, LineEvidence,
    PrepareSubmissionV1, PreparedRequestKindV1, PreparedRequestV1, StagePlanV1,
};
use wingman_lib::lexer::lex_p0_line;
use wingman_lib::parser::parse_p0_tokens;
use wingman_lib::runner::execute_prepared;
use wingman_lib::transport::OneShotBrokerV1;

fn parse(line: &str) -> Result<ExecutionPlanV1, CatalogErrorV1> {
    build_readonly_plan(&parse_p0_tokens(&lex_p0_line(line).unwrap()).unwrap())
}

#[test]
fn clear_builds_only_the_bounded_standalone_plan() {
    assert_eq!(
        parse("clear").unwrap(),
        ExecutionPlanV1 {
            stages: vec![StagePlanV1::ClearTerminal],
            redirect: None,
        }
    );
    for line in ["clear now", "clear | head", "clear > out.txt"] {
        assert!(parse(line).is_err(), "line: {line}");
    }
}

#[test]
fn reliable_familiar_clear_is_prepared_for_the_runner() {
    let mut session = InterpreterSession::new(78, 4, ActiveShell::WindowsPowerShell);
    let decision = session
        .prepare_submission(PrepareSubmissionV1 {
            session_id: 78,
            command_sequence: 4,
            shell: ActiveShell::WindowsPowerShell,
            familiar_enabled: true,
            evidence: LineEvidence::Reliable,
            raw_line: "clear".to_string(),
        })
        .unwrap();
    let request_id = match decision.decision {
        FrontendDecisionKindV1::InvokePrepared { request_id, .. } => request_id,
        other => panic!("expected prepared clear request, got {other:?}"),
    };
    assert_eq!(
        session.consume_prepared(&request_id).unwrap().kind,
        PreparedRequestKindV1::Execute {
            plan: parse("clear").unwrap()
        }
    );
}

#[test]
fn runner_emits_only_the_fixed_clear_and_home_sequence() {
    let outcome = execute_prepared(PreparedRequestV1 {
        protocol: "wingman.run".to_string(),
        version: 1,
        kind: PreparedRequestKindV1::Execute {
            plan: parse("clear").unwrap(),
        },
    })
    .unwrap();
    assert_eq!(outcome.exit_code, 0);
    assert_eq!(outcome.stdout, b"\x1b[2J\x1b[H");
    assert!(outcome.stderr.is_empty());
}

#[test]
fn packaged_runner_preserves_the_fixed_terminal_sequence() {
    let request_id = Uuid::new_v4().as_simple().to_string();
    let pipe_name = format!(
        r"\\.\pipe\wingman-clear-test-{}-{}",
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
                plan: parse("clear").unwrap(),
            },
        },
    )
    .unwrap();
    let server = thread::spawn(move || broker.serve());

    let output = Command::new(env!("CARGO_BIN_EXE_wingman-runner"))
        .arg(&request_id)
        .env("WINGMAN_BROKER_PIPE", &pipe_name)
        .output()
        .unwrap();
    server.join().unwrap().unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"\x1b[2J\x1b[H");
    assert!(output.stderr.is_empty());
}
