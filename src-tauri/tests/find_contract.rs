use std::fs;
use std::path::Path;
use uuid::Uuid;
use wingman_lib::catalog::{build_execution_plan, CatalogErrorV1};
use wingman_lib::interpreter::{
    ActiveShell, ExecutionPlanV1, FindEntryTypeV1, FrontendDecisionKindV1, InterpreterSession,
    LineEvidence, PrepareSubmissionV1, PreparedRequestKindV1, RunnerRequestValidationErrorV1,
    StagePlanV1,
};
use wingman_lib::lexer::lex_p0_line;
use wingman_lib::parser::parse_p0_tokens;
use wingman_lib::runner_cancel::RunnerCancellationV1;
use wingman_lib::runner_find::execute_find_with_cwd_to;
use wingman_lib::windows_path::validate_path_value;

fn parse(line: &str) -> Result<ExecutionPlanV1, CatalogErrorV1> {
    build_execution_plan(&parse_p0_tokens(&lex_p0_line(line).unwrap()).unwrap())
}

fn fixture() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("wingman-find-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    root
}

fn execute(line: &str, cwd: &Path) -> (u8, Vec<u8>, Vec<u8>) {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit = execute_find_with_cwd_to(
        &parse(line).unwrap(),
        cwd,
        &mut stdout,
        &mut stderr,
        &RunnerCancellationV1::new(),
    )
    .unwrap();
    (exit, stdout, stderr)
}

#[test]
fn find_builds_one_bounded_typed_source_stage() {
    assert_eq!(
        parse(r#"find src -iname "*test*" -type f -mindepth 1 -maxdepth 3 -print"#)
            .unwrap()
            .stages,
        vec![StagePlanV1::FindPaths {
            path: validate_path_value("src").unwrap(),
            entry_type: Some(FindEntryTypeV1::File),
            name_pattern: Some("*test*".to_string()),
            ignore_case: true,
            min_depth: 1,
            max_depth: Some(3),
        }]
    );
    for line in [
        "find",
        "find . -type x",
        "find . -type f -type d",
        "find . -name one -iname two",
        "find . -mindepth -1",
        "find . -maxdepth nope",
        "find . -print -print",
        "find . -delete",
        "cat file | find .",
    ] {
        assert!(parse(line).is_err(), "line: {line}");
    }
}

#[test]
fn find_walks_depth_first_preorder_with_native_relative_display() {
    let root = fixture();
    fs::create_dir_all(root.join("a-dir").join("nested")).unwrap();
    fs::create_dir(root.join("B-dir")).unwrap();
    fs::write(root.join("a-dir").join("one.ts"), b"").unwrap();
    fs::write(root.join("a-dir").join("nested").join("two.txt"), b"").unwrap();
    fs::write(root.join("z.txt"), b"").unwrap();

    let (exit, stdout, stderr) = execute("find .", &root);
    assert_eq!(exit, 0);
    assert_eq!(
        stdout,
        b".\r\n.\\a-dir\r\n.\\a-dir\\nested\r\n.\\a-dir\\nested\\two.txt\r\n.\\a-dir\\one.ts\r\n.\\B-dir\r\n.\\z.txt\r\n"
    );
    assert!(stderr.is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn find_filters_basename_type_and_depth_then_feeds_the_ordered_pipeline() {
    let root = fixture();
    fs::create_dir_all(root.join("src").join("nested")).unwrap();
    fs::write(root.join("src").join("Alpha.TS"), b"").unwrap();
    fs::write(root.join("src").join("nested").join("beta.ts"), b"").unwrap();
    fs::write(root.join("src").join("nested").join("note.txt"), b"").unwrap();

    let (exit, stdout, stderr) = execute(
        r#"find src -type f -iname "*.ts" -mindepth 1 | wc -l"#,
        &root,
    );
    assert_eq!(exit, 0);
    assert_eq!(stdout, b"2\r\n");
    assert!(stderr.is_empty());

    let (exit, stdout, _) = execute("find src -type d -mindepth 1 -maxdepth 1", &root);
    assert_eq!(exit, 0);
    assert_eq!(stdout, b"src\\nested\r\n");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn find_no_match_is_success_and_missing_start_is_operational_failure() {
    let root = fixture();
    let (exit, stdout, stderr) = execute(r#"find . -name "never*""#, &root);
    assert_eq!(exit, 0);
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());

    let (exit, stdout, stderr) = execute("find missing", &root);
    assert_eq!(exit, 1);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"wingman find: start path cannot be inspected\r\n");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn find_reports_but_never_descends_into_a_reparse_entry() {
    let root = fixture();
    let outside = fixture();
    fs::write(outside.join("must-not-visit.txt"), b"").unwrap();
    let link = root.join("linked");
    if std::os::windows::fs::symlink_dir(&outside, &link).is_err() {
        let output = std::process::Command::new("cmd.exe")
            .args([
                "/d",
                "/c",
                "mklink",
                "/J",
                link.to_str().unwrap(),
                outside.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
    }

    let (exit, stdout, stderr) = execute("find .", &root);
    assert_eq!(exit, 0);
    assert_eq!(stdout, b".\r\n.\\linked\r\n");
    assert!(stderr.is_empty());
    assert_eq!(execute("find . -type d", &root).1, b".\r\n");

    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(outside).unwrap();
}

#[test]
fn reliable_familiar_find_is_prepared_for_the_runner() {
    let line = r#"find . -type f -name "*.rs" | head -n 1"#;
    let mut session = InterpreterSession::new(80, 6, ActiveShell::WindowsPowerShell);
    let decision = session
        .prepare_submission(PrepareSubmissionV1 {
            session_id: 80,
            command_sequence: 6,
            shell: ActiveShell::WindowsPowerShell,
            familiar_enabled: true,
            evidence: LineEvidence::Reliable,
            raw_line: line.to_string(),
        })
        .unwrap();
    let request_id = match decision.decision {
        FrontendDecisionKindV1::InvokePrepared { request_id, .. } => request_id,
        other => panic!("expected prepared find request, got {other:?}"),
    };
    assert_eq!(
        session.consume_prepared(&request_id).unwrap().kind,
        PreparedRequestKindV1::Execute {
            plan: parse(line).unwrap()
        }
    );
}

#[test]
fn runner_rejects_a_noncanonical_find_wire_plan() {
    let mut plan = parse("find .").unwrap();
    let StagePlanV1::FindPaths { ignore_case, .. } = &mut plan.stages[0] else {
        unreachable!();
    };
    *ignore_case = true;
    assert_eq!(
        wingman_lib::interpreter::validate_execution_plan(&plan),
        Err(RunnerRequestValidationErrorV1::InvalidStageShape)
    );
}
