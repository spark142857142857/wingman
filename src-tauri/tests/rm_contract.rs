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
fn files_are_removed_left_to_right_and_missing_is_force_only_success() {
    let sandbox = sandbox();
    let first = sandbox.join("first.txt");
    let missing = sandbox.join("missing.txt");
    let last = sandbox.join("last.txt");
    fs::write(&first, b"first").unwrap();
    fs::write(&last, b"last").unwrap();

    let outcome = execute_prepared(request(&[&first, &missing, &last], false, false))
        .expect("remove independent files");

    assert_eq!(outcome.exit_code, 1);
    assert!(!first.exists());
    assert!(!last.exists());
    assert!(String::from_utf8(outcome.stderr)
        .unwrap()
        .contains("missing.txt: path does not exist"));

    let forced = execute_prepared(request(&[&missing], false, true)).expect("force missing");
    assert_eq!(forced.exit_code, 0);
    assert!(forced.stderr.is_empty());
    cleanup(&sandbox);
}

#[test]
fn recursive_removal_is_child_first_and_nonrecursive_directory_is_preserved() {
    let sandbox = sandbox();
    let tree = sandbox.join("tree");
    fs::create_dir_all(tree.join("nested").join("empty")).unwrap();
    fs::write(tree.join("nested").join("content.txt"), b"content").unwrap();

    let refused = execute_prepared(request(&[&tree], false, false)).expect("refuse directory");
    assert_eq!(refused.exit_code, 1);
    assert!(tree.join("nested").join("content.txt").is_file());

    let removed = execute_prepared(request(&[&tree], true, false)).expect("remove tree");
    assert_eq!(
        removed.exit_code,
        0,
        "{}",
        String::from_utf8_lossy(&removed.stderr)
    );
    assert!(removed.stderr.is_empty());
    assert!(!tree.exists());
    cleanup(&sandbox);
}

#[test]
fn force_removes_a_readonly_file_without_bypassing_the_typed_plan() {
    let sandbox = sandbox();
    let target = sandbox.join("readonly.txt");
    fs::write(&target, b"content").unwrap();
    let mut permissions = fs::metadata(&target).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&target, permissions).unwrap();

    let outcome = execute_prepared(request(&[&target], false, true)).expect("force readonly");

    assert_eq!(outcome.exit_code, 0);
    assert!(!target.exists());
    cleanup(&sandbox);
}

#[test]
fn readonly_file_requires_force_and_remains_after_the_failed_attempt() {
    let sandbox = sandbox();
    let target = sandbox.join("readonly.txt");
    fs::write(&target, b"content").unwrap();
    let mut permissions = fs::metadata(&target).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&target, permissions).unwrap();

    let refused = execute_prepared(request(&[&target], false, false)).expect("refuse readonly");
    assert_eq!(refused.exit_code, 1);
    assert_eq!(fs::read(&target).unwrap(), b"content");

    let forced = execute_prepared(request(&[&target], false, true)).expect("force readonly");
    assert_eq!(forced.exit_code, 0);
    assert!(!target.exists());
    cleanup(&sandbox);
}

#[test]
fn cancellation_before_rm_preflight_mutates_and_reports_nothing() {
    let sandbox = sandbox();
    let target = sandbox.join("keep.txt");
    fs::write(&target, b"keep").unwrap();
    let cancellation = RunnerCancellationV1::new();
    cancellation.cancel();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let exit_code = execute_prepared_to_with_cancellation(
        request(&[&target], false, false),
        &mut stdout,
        &mut stderr,
        &cancellation,
    )
    .expect("cancel removal");

    assert_eq!(exit_code, 130);
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
    assert_eq!(fs::read(&target).unwrap(), b"keep");
    cleanup(&sandbox);
}

#[test]
fn a_recursive_cwd_target_rejects_the_whole_request_before_deletion() {
    let sandbox = sandbox();
    let safe = sandbox.join("safe.txt");
    fs::write(&safe, b"keep").unwrap();
    let cwd = std::env::current_dir().unwrap();

    let outcome = execute_prepared(request(&[&safe, &cwd], true, false))
        .expect("reject current directory target");

    assert_eq!(outcome.exit_code, 2);
    assert!(safe.is_file());
    assert!(String::from_utf8(outcome.stderr)
        .unwrap()
        .contains("current directory or its ancestor is not allowed"));
    cleanup(&sandbox);
}

#[test]
fn recursive_removal_deletes_reparse_links_without_following_their_targets() {
    let sandbox = sandbox();
    let outside = sandbox.with_extension("outside");
    let tree = sandbox.join("tree");
    let outside_file = outside.join("sentinel.txt");
    fs::create_dir(&outside).unwrap();
    fs::write(&outside_file, b"outside").unwrap();
    fs::create_dir(&tree).unwrap();
    create_directory_reparse(&outside, &tree.join("link"));

    let outcome = execute_prepared(request(&[&tree], true, false)).expect("remove reparse leaf");

    assert_eq!(
        outcome.exit_code,
        0,
        "{}",
        String::from_utf8_lossy(&outcome.stderr)
    );
    assert!(!tree.exists());
    assert_eq!(fs::read(&outside_file).unwrap(), b"outside");
    cleanup(&sandbox);
    cleanup(&outside);
}

#[test]
fn removing_one_hard_link_name_preserves_the_other_name() {
    let sandbox = sandbox();
    let original = sandbox.join("original.txt");
    let link = sandbox.join("link.txt");
    fs::write(&original, b"shared").unwrap();
    fs::hard_link(&original, &link).unwrap();

    let outcome = execute_prepared(request(&[&link], false, false)).expect("remove hard link");

    assert_eq!(outcome.exit_code, 0);
    assert!(!link.exists());
    assert_eq!(fs::read(&original).unwrap(), b"shared");
    cleanup(&sandbox);
}

fn request(paths: &[&Path], recursive: bool, force: bool) -> PreparedRequestV1 {
    PreparedRequestV1 {
        protocol: "wingman.run".to_string(),
        version: 1,
        kind: PreparedRequestKindV1::Execute {
            plan: ExecutionPlanV1 {
                stages: vec![StagePlanV1::RemovePaths {
                    paths: paths
                        .iter()
                        .map(|path| validate_path_value(&path.display().to_string()).unwrap())
                        .collect(),
                    recursive,
                    force,
                }],
                redirect: None,
            },
        },
    }
}

fn sandbox() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "wingman-rm-test-{}-{}",
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
