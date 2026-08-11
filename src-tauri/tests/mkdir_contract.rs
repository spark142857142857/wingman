use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use uuid::Uuid;
use wingman_lib::interpreter::{
    ExecutionPlanV1, PreparedRequestKindV1, PreparedRequestV1, StagePlanV1,
};
use wingman_lib::runner::{execute_prepared, execute_prepared_to_with_cancellation};
use wingman_lib::runner_cancel::RunnerCancellationV1;
use wingman_lib::windows_path::validate_path_value;

#[test]
fn mkdir_creates_operands_left_to_right_and_continues_after_operational_failures() {
    let sandbox = sandbox();
    let existing = sandbox.join("existing");
    let first = sandbox.join("first");
    let blocking_file = sandbox.join("blocking-file");
    let blocked = blocking_file.join("child");
    let last = sandbox.join("last");
    fs::create_dir(&existing).unwrap();
    fs::write(&blocking_file, b"preserve").unwrap();

    let outcome = execute_prepared(request([&existing, &first, &blocked, &last], false))
        .expect("execute mkdir operands");

    assert_eq!(outcome.exit_code, 1);
    assert!(outcome.stdout.is_empty());
    let stderr = String::from_utf8(outcome.stderr).unwrap();
    let existing_position = stderr.find("existing: directory already exists").unwrap();
    let blocked_position = stderr
        .find("child: a path component is not a directory")
        .unwrap();
    assert!(existing_position < blocked_position);
    assert!(first.is_dir());
    assert!(last.is_dir());
    assert_eq!(fs::read(&blocking_file).unwrap(), b"preserve");
    cleanup(&sandbox);
}

#[test]
fn mkdir_parents_creates_missing_components_and_accepts_existing_directories() {
    let sandbox = sandbox();
    let existing = sandbox.join("existing");
    let nested = sandbox.join("한글").join("nested").join("leaf");
    fs::create_dir(&existing).unwrap();

    let outcome = execute_prepared(request([&existing, &nested], true)).expect("mkdir parents");

    assert_eq!(outcome.exit_code, 0);
    assert!(outcome.stdout.is_empty());
    assert!(outcome.stderr.is_empty());
    assert!(nested.is_dir());
    cleanup(&sandbox);
}

#[test]
fn overlapping_mkdir_operands_reuse_directories_created_earlier_in_the_request() {
    let sandbox = sandbox();
    let parent = sandbox.join("parent");
    let child = parent.join("child");

    let plain = execute_prepared(request([&parent, &child], false))
        .expect("plain overlapping mkdir operands");
    assert_eq!(plain.exit_code, 0);
    assert!(plain.stderr.is_empty());
    assert!(child.is_dir());

    let parents_root = sandbox.join("parents-root");
    let parents_child = parents_root.join("child");
    let recursive = execute_prepared(request([&parents_root, &parents_child], true))
        .expect("parents overlapping mkdir operands");
    assert_eq!(recursive.exit_code, 0);
    assert!(recursive.stderr.is_empty());
    assert!(parents_child.is_dir());
    cleanup(&sandbox);
}

#[test]
fn duplicate_plain_mkdir_operand_reports_the_directory_created_by_the_first() {
    let sandbox = sandbox();
    let target = sandbox.join("duplicate");

    let outcome = execute_prepared(request([&target, &target], false)).expect("duplicate mkdir");

    assert_eq!(outcome.exit_code, 1);
    assert!(target.is_dir());
    assert!(String::from_utf8(outcome.stderr)
        .unwrap()
        .contains("directory already exists"));
    cleanup(&sandbox);
}

#[test]
fn mkdir_without_parents_does_not_create_a_partial_ancestor_chain() {
    let sandbox = sandbox();
    let nested = sandbox.join("missing").join("leaf");
    let independent = sandbox.join("independent");

    let outcome =
        execute_prepared(request([&nested, &independent], false)).expect("mkdir without parents");

    assert_eq!(outcome.exit_code, 1);
    assert!(!sandbox.join("missing").exists());
    assert!(independent.is_dir());
    assert!(String::from_utf8(outcome.stderr)
        .unwrap()
        .contains("parent directory does not exist"));
    cleanup(&sandbox);
}

#[test]
fn a_later_reparse_operand_prevents_every_mkdir_mutation() {
    let sandbox = sandbox();
    let outside = sandbox.with_extension("outside");
    let link = sandbox.join("link");
    let safe = sandbox.join("must-not-exist");
    fs::create_dir(&outside).unwrap();
    create_directory_reparse(&outside, &link);

    let outcome = execute_prepared(request([&safe, &link.join("child")], true))
        .expect("reject reparse before mutation");

    assert_eq!(outcome.exit_code, 2);
    assert!(!safe.exists());
    assert!(!outside.join("child").exists());
    assert!(String::from_utf8(outcome.stderr)
        .unwrap()
        .contains("reparse paths are not allowed"));
    fs::remove_dir(&link).unwrap();
    cleanup(&sandbox);
    cleanup(&outside);
}

#[test]
fn cancellation_before_mkdir_preflight_writes_and_mutates_nothing() {
    let sandbox = sandbox();
    let target = sandbox.join("cancelled");
    let cancellation = RunnerCancellationV1::new();
    cancellation.cancel();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let exit_code = execute_prepared_to_with_cancellation(
        request([&target], false),
        &mut stdout,
        &mut stderr,
        &cancellation,
    )
    .expect("cancel mkdir");

    assert_eq!(exit_code, 130);
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
    assert!(!target.exists());
    cleanup(&sandbox);
}

fn request<'a>(paths: impl IntoIterator<Item = &'a PathBuf>, parents: bool) -> PreparedRequestV1 {
    PreparedRequestV1 {
        protocol: "wingman.run".to_string(),
        version: 1,
        kind: PreparedRequestKindV1::Execute {
            plan: ExecutionPlanV1 {
                stages: vec![StagePlanV1::CreateDirectories {
                    paths: paths
                        .into_iter()
                        .map(|path| validate_path_value(&path.display().to_string()).unwrap())
                        .collect(),
                    parents,
                }],
                redirect: None,
            },
        },
    }
}

fn sandbox() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "wingman-mkdir-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    ));
    fs::create_dir(&path).unwrap();
    path
}

fn cleanup(path: &Path) {
    assert!(path.starts_with(std::env::temp_dir()));
    if path.exists() {
        fs::remove_dir_all(path).unwrap();
    }
}

fn create_directory_reparse(target: &Path, link: &Path) {
    use std::os::windows::fs::symlink_dir;

    if symlink_dir(target, link).is_ok() {
        return;
    }
    let output = Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .expect("start junction helper");
    assert!(output.status.success(), "create directory reparse fixture");
}
