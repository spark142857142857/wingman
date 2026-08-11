use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use std::path::Path;
use uuid::Uuid;
use windows_sys::Win32::Storage::FileSystem::{SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN};
use wingman_lib::catalog::{build_execution_plan, CatalogErrorV1};
use wingman_lib::interpreter::{
    ActiveShell, ExecutionPlanV1, FrontendDecisionKindV1, InterpreterSession, LineEvidence,
    PrepareSubmissionV1, PreparedRequestKindV1, StagePlanV1,
};
use wingman_lib::lexer::lex_p0_line;
use wingman_lib::parser::parse_p0_tokens;
use wingman_lib::runner_cancel::RunnerCancellationV1;
use wingman_lib::runner_ls::execute_ls_with_cwd_to;

fn parse(line: &str) -> Result<ExecutionPlanV1, CatalogErrorV1> {
    build_execution_plan(&parse_p0_tokens(&lex_p0_line(line).unwrap()).unwrap())
}

fn fixture() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("wingman-ls-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    root
}

fn execute(line: &str, cwd: &Path) -> (u8, Vec<u8>, Vec<u8>) {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit = execute_ls_with_cwd_to(
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
fn ls_and_ll_build_bounded_typed_source_plans() {
    assert_eq!(
        parse("ls -lah folder").unwrap().stages,
        vec![StagePlanV1::ListEntries {
            path: Some(wingman_lib::windows_path::validate_path_value("folder").unwrap()),
            include_hidden: true,
            long: true,
            human_readable: true,
        }]
    );
    assert!(matches!(
        parse("ll file.txt").unwrap().stages.as_slice(),
        [StagePlanV1::ListEntries { long: true, .. }]
    ));
    for line in [
        "ls -h",
        "ls -z",
        "ls one two",
        "ll -a",
        "cat file.txt | ls",
        "ls *.txt",
    ] {
        assert!(parse(line).is_err(), "line: {line}");
    }
}

#[test]
fn reliable_familiar_ls_is_prepared_as_one_typed_pipeline() {
    let line = "ls -a | grep .txt | head -n 2";
    let mut session = InterpreterSession::new(79, 5, ActiveShell::WindowsPowerShell);
    let decision = session
        .prepare_submission(PrepareSubmissionV1 {
            session_id: 79,
            command_sequence: 5,
            shell: ActiveShell::WindowsPowerShell,
            familiar_enabled: true,
            evidence: LineEvidence::Reliable,
            raw_line: line.to_string(),
        })
        .unwrap();
    let request_id = match decision.decision {
        FrontendDecisionKindV1::InvokePrepared { request_id, .. } => request_id,
        other => panic!("expected prepared ls request, got {other:?}"),
    };
    assert_eq!(
        session.consume_prepared(&request_id).unwrap().kind,
        PreparedRequestKindV1::Execute {
            plan: parse(line).unwrap()
        }
    );
}

#[test]
fn ls_sorts_raw_basenames_and_can_feed_ordered_text_stages() {
    let root = fixture();
    fs::write(root.join("B.txt"), b"").unwrap();
    fs::write(root.join("a.txt"), b"").unwrap();
    fs::write(root.join("c.txt"), b"").unwrap();
    fs::create_dir(root.join("folder")).unwrap();

    let (exit, stdout, stderr) = execute("ls", &root);
    assert_eq!(exit, 0);
    assert_eq!(stdout, b"a.txt\r\nB.txt\r\nc.txt\r\nfolder\r\n");
    assert!(stderr.is_empty());

    let (exit, stdout, stderr) = execute("ls | grep .txt | head -n 2", &root);
    assert_eq!(exit, 0);
    assert_eq!(stdout, b"a.txt\r\nB.txt\r\n");
    assert!(stderr.is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn ls_hides_windows_hidden_entries_unless_all_is_requested() {
    let root = fixture();
    let hidden = root.join("hidden.txt");
    fs::write(&hidden, b"").unwrap();
    let attributes = fs::metadata(&hidden).unwrap().file_attributes();
    let wide = hidden
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    assert_ne!(
        unsafe { SetFileAttributesW(wide.as_ptr(), attributes | FILE_ATTRIBUTE_HIDDEN) },
        0
    );

    assert_eq!(execute("ls", &root).1, b"");
    assert_eq!(execute("ls -a", &root).1, b"hidden.txt\r\n");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn long_human_listing_uses_the_pinned_shape_and_safe_redirection() {
    let root = fixture();
    fs::write(root.join("sample.bin"), vec![0u8; 1536]).unwrap();
    let (exit, stdout, stderr) = execute("ls -lh sample.bin", &root);
    assert_eq!(exit, 0);
    let output = String::from_utf8(stdout).unwrap();
    assert!(output.starts_with("- "));
    assert!(output.contains(" 1.5KiB "));
    assert!(output.ends_with(" sample.bin\r\n"));
    assert!(stderr.is_empty());

    let (exit, stdout, stderr) = execute("ls > listing.txt", &root);
    assert_eq!(exit, 0);
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
    assert_eq!(
        fs::read(root.join("listing.txt")).unwrap(),
        b"sample.bin\r\n"
    );
    fs::remove_dir_all(root).unwrap();
}
