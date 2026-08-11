use std::fs::{self, File, FileTimes};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};
use uuid::Uuid;
use wingman_lib::interpreter::{
    ExecutionPlanV1, PreparedRequestKindV1, PreparedRequestV1, StagePlanV1,
};
use wingman_lib::runner::{execute_prepared, execute_prepared_to_with_cancellation};
use wingman_lib::runner_cancel::RunnerCancellationV1;
use wingman_lib::windows_path::validate_path_value;

#[test]
fn touch_preserves_existing_contents_and_applies_one_timestamp_to_every_operand() {
    let sandbox = sandbox();
    let existing = sandbox.join("existing.txt");
    let first_new = sandbox.join("한글 새 파일.txt");
    let second_new = sandbox.join("second.txt");
    fs::write(&existing, b"preserve exactly").unwrap();
    let old_timestamp = SystemTime::now() - Duration::from_secs(86_400);
    File::options()
        .write(true)
        .open(&existing)
        .unwrap()
        .set_times(FileTimes::new().set_modified(old_timestamp))
        .unwrap();

    let outcome = execute_prepared(request([&existing, &first_new, &second_new]))
        .expect("execute touch operands");

    assert_eq!(outcome.exit_code, 0);
    assert!(outcome.stdout.is_empty());
    assert!(outcome.stderr.is_empty());
    assert_eq!(fs::read(&existing).unwrap(), b"preserve exactly");
    assert_eq!(fs::read(&first_new).unwrap(), b"");
    assert_eq!(fs::read(&second_new).unwrap(), b"");
    let existing_modified = fs::metadata(&existing).unwrap().modified().unwrap();
    let first_modified = fs::metadata(&first_new).unwrap().modified().unwrap();
    let second_modified = fs::metadata(&second_new).unwrap().modified().unwrap();
    assert!(existing_modified > old_timestamp);
    assert_eq!(existing_modified, first_modified);
    assert_eq!(first_modified, second_modified);
    cleanup(&sandbox);
}

#[test]
fn touch_continues_after_directory_and_missing_parent_operational_failures() {
    let sandbox = sandbox();
    let directory = sandbox.join("directory");
    let missing_parent = sandbox.join("missing").join("child.txt");
    let independent = sandbox.join("independent.txt");
    fs::create_dir(&directory).unwrap();

    let outcome = execute_prepared(request([&directory, &missing_parent, &independent]))
        .expect("continue touch operands");

    assert_eq!(outcome.exit_code, 1);
    assert!(independent.is_file());
    assert!(!sandbox.join("missing").exists());
    let stderr = String::from_utf8(outcome.stderr).unwrap();
    let directory_position = stderr.find("target is not a regular file").unwrap();
    let missing_position = stderr.find("parent directory does not exist").unwrap();
    assert!(directory_position < missing_position);
    cleanup(&sandbox);
}

#[test]
fn duplicate_missing_touch_operands_reuse_the_file_created_by_the_first() {
    let sandbox = sandbox();
    let target = sandbox.join("duplicate.txt");

    let outcome = execute_prepared(request([&target, &target])).expect("duplicate touch");

    assert_eq!(outcome.exit_code, 0);
    assert!(outcome.stderr.is_empty());
    assert!(target.is_file());
    assert_eq!(fs::read(&target).unwrap(), b"");
    cleanup(&sandbox);
}

#[test]
fn a_later_reparse_operand_prevents_every_touch_mutation() {
    let sandbox = sandbox();
    let outside = sandbox.with_extension("outside");
    let link = sandbox.join("link");
    let existing = sandbox.join("existing.txt");
    let safe_new = sandbox.join("must-not-exist.txt");
    fs::create_dir(&outside).unwrap();
    create_directory_reparse(&outside, &link);
    fs::write(&existing, b"preserve").unwrap();
    let old_timestamp = SystemTime::now() - Duration::from_secs(86_400);
    File::options()
        .write(true)
        .open(&existing)
        .unwrap()
        .set_times(FileTimes::new().set_modified(old_timestamp))
        .unwrap();
    let before = fs::metadata(&existing).unwrap().modified().unwrap();

    let outcome = execute_prepared(request([&existing, &safe_new, &link.join("child.txt")]))
        .expect("reject reparse before touch mutation");

    assert_eq!(outcome.exit_code, 2);
    assert_eq!(fs::metadata(&existing).unwrap().modified().unwrap(), before);
    assert_eq!(fs::read(&existing).unwrap(), b"preserve");
    assert!(!safe_new.exists());
    assert!(!outside.join("child.txt").exists());
    assert!(String::from_utf8(outcome.stderr)
        .unwrap()
        .contains("reparse paths are not allowed"));
    fs::remove_dir(&link).unwrap();
    cleanup(&sandbox);
    cleanup(&outside);
}

#[test]
fn cancellation_before_touch_preflight_writes_and_mutates_nothing() {
    let sandbox = sandbox();
    let target = sandbox.join("cancelled.txt");
    let cancellation = RunnerCancellationV1::new();
    cancellation.cancel();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let exit_code = execute_prepared_to_with_cancellation(
        request([&target]),
        &mut stdout,
        &mut stderr,
        &cancellation,
    )
    .expect("cancel touch");

    assert_eq!(exit_code, 130);
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
    assert!(!target.exists());
    cleanup(&sandbox);
}

fn request<'a>(paths: impl IntoIterator<Item = &'a PathBuf>) -> PreparedRequestV1 {
    PreparedRequestV1 {
        protocol: "wingman.run".to_string(),
        version: 1,
        kind: PreparedRequestKindV1::Execute {
            plan: ExecutionPlanV1 {
                stages: vec![StagePlanV1::TouchFiles {
                    paths: paths
                        .into_iter()
                        .map(|path| validate_path_value(&path.display().to_string()).unwrap())
                        .collect(),
                }],
                redirect: None,
            },
        },
    }
}

fn sandbox() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "wingman-touch-test-{}-{}",
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
