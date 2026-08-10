use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;
use wingman_lib::catalog::{build_readonly_plan, CatalogErrorV1};
use wingman_lib::interpreter::{
    ActiveShell, FrontendDecisionKindV1, InterpreterSession, LineEvidence, PrepareSubmissionV1,
    PreparedRequestKindV1, StagePlanV1,
};
use wingman_lib::lexer::lex_p0_line;
use wingman_lib::parser::parse_p0_tokens;
use wingman_lib::runner_cancel::RunnerCancellationV1;
use wingman_lib::runner_which::execute_which_with_snapshot_to;

fn parse(line: &str) -> Result<wingman_lib::interpreter::ExecutionPlanV1, CatalogErrorV1> {
    build_readonly_plan(&parse_p0_tokens(&lex_p0_line(line).unwrap()).unwrap())
}

#[test]
fn which_builds_one_non_pipeline_non_redirected_stage() {
    assert_eq!(
        parse("which cargo").unwrap().stages,
        vec![StagePlanV1::FindExecutable {
            name: "cargo".to_string()
        }]
    );
    assert_eq!(
        parse("which -- -tool").unwrap().stages,
        vec![StagePlanV1::FindExecutable {
            name: "-tool".to_string()
        }]
    );
    for line in [
        "which",
        "which one two",
        "which folder\\tool",
        "which C:tool",
        "which *.exe",
        "which .",
        "which tool | head",
        "which tool > result.txt",
    ] {
        assert!(parse(line).is_err(), "line: {line}");
    }
}

#[test]
fn reliable_familiar_which_is_prepared_for_the_runner() {
    let mut session = InterpreterSession::new(77, 3, ActiveShell::WindowsPowerShell);
    let decision = session
        .prepare_submission(PrepareSubmissionV1 {
            session_id: 77,
            command_sequence: 3,
            shell: ActiveShell::WindowsPowerShell,
            familiar_enabled: true,
            evidence: LineEvidence::Reliable,
            raw_line: "which cargo".to_string(),
        })
        .unwrap();
    let request_id = match decision.decision {
        FrontendDecisionKindV1::InvokePrepared { request_id, .. } => request_id,
        other => panic!("expected prepared which request, got {other:?}"),
    };
    let request = session.consume_prepared(&request_id).unwrap();
    assert_eq!(
        request.kind,
        PreparedRequestKindV1::Execute {
            plan: parse("which cargo").unwrap()
        }
    );
}

#[test]
fn which_uses_current_directory_then_path_and_pathext_order() {
    let root = std::env::temp_dir().join(format!("wingman-which-{}", Uuid::new_v4()));
    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    fs::write(root.join("demo.CMD"), b"").unwrap();
    fs::write(bin.join("demo.EXE"), b"").unwrap();
    fs::write(bin.join("explicit.BAT"), b"").unwrap();

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit = execute_which_with_snapshot_to(
        "demo",
        &root,
        Some(bin.as_os_str()),
        Some(OsStr::new(".EXE;.CMD;.exe;.BAT")),
        &mut stdout,
        &mut stderr,
        &RunnerCancellationV1::new(),
    )
    .unwrap();
    assert_eq!(exit, 0);
    assert_eq!(
        stdout,
        format!("{}\r\n", root.join("demo.CMD").display()).as_bytes()
    );
    assert!(stderr.is_empty());

    stdout.clear();
    let exit = execute_which_with_snapshot_to(
        "explicit.BAT",
        &root,
        Some(bin.as_os_str()),
        Some(OsStr::new("EXE;BAT")),
        &mut stdout,
        &mut stderr,
        &RunnerCancellationV1::new(),
    )
    .unwrap();
    assert_eq!(exit, 0);
    assert_eq!(
        stdout,
        format!("{}\r\n", bin.join("explicit.BAT").display()).as_bytes()
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn which_returns_result_one_without_diagnostic_when_no_match_exists() {
    let root = std::env::temp_dir().join(format!("wingman-which-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let missing_path = PathBuf::from(&root).join("missing");
    let exit = execute_which_with_snapshot_to(
        "absent",
        &root,
        Some(missing_path.as_os_str()),
        None,
        &mut stdout,
        &mut stderr,
        &RunnerCancellationV1::new(),
    )
    .unwrap();
    assert_eq!(exit, 1);
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
    fs::remove_dir_all(root).unwrap();
}
