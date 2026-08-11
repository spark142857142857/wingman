use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use uuid::Uuid;
use wingman_lib::interpreter::{
    ExecutionPlanV1, ExistingDestinationPolicyV1, PreparedRequestKindV1, PreparedRequestV1,
    StagePlanV1,
};
use wingman_lib::runner::execute_prepared;
use wingman_lib::windows_path::validate_path_value;

#[test]
fn same_volume_file_move_renames_the_source_and_replaces_at_commit() {
    let sandbox = sandbox();
    let source = sandbox.join("한글 source.txt");
    let destination = sandbox.join("destination.txt");
    fs::write(&source, b"new content").unwrap();
    fs::write(&destination, b"old content").unwrap();

    let outcome = execute_prepared(request(
        &source,
        &destination,
        ExistingDestinationPolicyV1::Replace,
    ))
    .expect("move file");

    assert_eq!(outcome.exit_code, 0, "{:?}", outcome.stderr);
    assert!(!source.exists());
    assert_eq!(fs::read(&destination).unwrap(), b"new content");
    cleanup(&sandbox);
}

#[test]
fn same_volume_directory_move_preserves_the_complete_tree() {
    let sandbox = sandbox();
    let source = sandbox.join("source tree");
    let destination = sandbox.join("moved tree");
    fs::create_dir_all(source.join("nested").join("empty")).unwrap();
    fs::write(source.join("nested").join("file.txt"), b"content").unwrap();

    let outcome = execute_prepared(request(
        &source,
        &destination,
        ExistingDestinationPolicyV1::Replace,
    ))
    .expect("move directory");

    assert_eq!(outcome.exit_code, 0, "{:?}", outcome.stderr);
    assert!(!source.exists());
    assert_eq!(
        fs::read(destination.join("nested").join("file.txt")).unwrap(),
        b"content"
    );
    assert!(destination.join("nested").join("empty").is_dir());
    cleanup(&sandbox);
}

#[test]
fn move_into_existing_directory_uses_source_basename_and_no_clobber_skips() {
    let sandbox = sandbox();
    let source = sandbox.join("source.txt");
    let destination_directory = sandbox.join("destination");
    fs::create_dir(&destination_directory).unwrap();
    fs::write(&source, b"source").unwrap();
    fs::write(destination_directory.join("source.txt"), b"existing").unwrap();

    let outcome = execute_prepared(request(
        &source,
        &destination_directory,
        ExistingDestinationPolicyV1::NoClobber,
    ))
    .expect("skip move");

    assert_eq!(outcome.exit_code, 0);
    assert_eq!(fs::read(&source).unwrap(), b"source");
    assert_eq!(
        fs::read(destination_directory.join("source.txt")).unwrap(),
        b"existing"
    );
    cleanup(&sandbox);
}

#[test]
fn force_move_replaces_a_readonly_destination() {
    let sandbox = sandbox();
    let source = sandbox.join("source.txt");
    let destination = sandbox.join("destination.txt");
    fs::write(&source, b"new").unwrap();
    fs::write(&destination, b"old").unwrap();
    let mut permissions = fs::metadata(&destination).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&destination, permissions).unwrap();

    let outcome = execute_prepared(request(
        &source,
        &destination,
        ExistingDestinationPolicyV1::Force,
    ))
    .expect("force move");

    assert_eq!(outcome.exit_code, 0, "{:?}", outcome.stderr);
    assert!(!source.exists());
    assert_eq!(fs::read(&destination).unwrap(), b"new");
    cleanup(&sandbox);
}

#[test]
fn same_file_contained_destination_and_reparse_tree_are_no_mutation_rejections() {
    let sandbox = sandbox();
    let outside = sandbox.with_extension("outside");
    let file = sandbox.join("file.txt");
    let hard_link = sandbox.join("hard-link.txt");
    fs::write(&file, b"preserve").unwrap();
    fs::hard_link(&file, &hard_link).unwrap();

    let same = execute_prepared(request(
        &file,
        &hard_link,
        ExistingDestinationPolicyV1::Replace,
    ))
    .expect("reject same file");
    assert_eq!(same.exit_code, 2);
    assert_eq!(fs::read(&file).unwrap(), b"preserve");

    let directory = sandbox.join("directory");
    fs::create_dir(&directory).unwrap();
    let contained = directory.join("inside");
    let inside = execute_prepared(request(
        &directory,
        &contained,
        ExistingDestinationPolicyV1::Replace,
    ))
    .expect("reject contained destination");
    assert_eq!(inside.exit_code, 2);
    assert!(directory.is_dir());
    assert!(!contained.exists());

    fs::create_dir(&outside).unwrap();
    create_directory_reparse(&outside, &directory.join("link"));
    let reparse_destination = sandbox.join("must-not-exist");
    let reparse = execute_prepared(request(
        &directory,
        &reparse_destination,
        ExistingDestinationPolicyV1::Replace,
    ))
    .expect("reject reparse tree");
    assert_eq!(reparse.exit_code, 2);
    assert!(directory.is_dir());
    assert!(!reparse_destination.exists());

    fs::remove_dir(directory.join("link")).unwrap();
    cleanup(&sandbox);
    cleanup(&outside);
}

#[test]
fn missing_source_does_not_mask_an_unsafe_destination() {
    let sandbox = sandbox();
    let outside = sandbox.with_extension("outside");
    let missing_source = sandbox.join("missing.txt");
    let link = sandbox.join("destination-link");
    fs::create_dir(&outside).unwrap();
    create_directory_reparse(&outside, &link);
    let unsafe_destination = link.join("destination.txt");

    let outcome = execute_prepared(request(
        &missing_source,
        &unsafe_destination,
        ExistingDestinationPolicyV1::Replace,
    ))
    .expect("inspect destination before missing-source result");

    assert_eq!(outcome.exit_code, 2);
    assert!(String::from_utf8(outcome.stderr)
        .unwrap()
        .contains("reparse ancestors are not allowed"));
    assert!(!outside.join("destination.txt").exists());
    fs::remove_dir(&link).unwrap();
    cleanup(&sandbox);
    cleanup(&outside);
}

#[test]
#[ignore = "requires WINGMAN_TEST_SECOND_VOLUME to name a writable distinct volume"]
fn actual_cross_volume_move_commits_then_removes_the_source() {
    let second_root = PathBuf::from(
        std::env::var_os("WINGMAN_TEST_SECOND_VOLUME")
            .expect("set WINGMAN_TEST_SECOND_VOLUME to a writable distinct volume"),
    );
    let destination_sandbox = second_root.join(format!(
        "wingman-mv-cross-volume-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    ));
    fs::create_dir(&destination_sandbox).unwrap();
    let source_sandbox = sandbox();
    let source = source_sandbox.join("source.txt");
    let destination = destination_sandbox.join("destination.txt");
    fs::write(&source, b"cross-volume").unwrap();

    let outcome = execute_prepared(request(
        &source,
        &destination,
        ExistingDestinationPolicyV1::Replace,
    ))
    .expect("move across actual volumes");

    assert_eq!(
        outcome.exit_code,
        0,
        "{}",
        String::from_utf8_lossy(&outcome.stderr)
    );
    assert!(!source.exists());
    assert_eq!(fs::read(&destination).unwrap(), b"cross-volume");
    cleanup(&source_sandbox);
    assert!(destination_sandbox.starts_with(&second_root));
    fs::remove_dir_all(&destination_sandbox).unwrap();
}

fn request(
    source: &Path,
    destination: &Path,
    policy: ExistingDestinationPolicyV1,
) -> PreparedRequestV1 {
    PreparedRequestV1 {
        protocol: "wingman.run".to_string(),
        version: 1,
        kind: PreparedRequestKindV1::Execute {
            plan: ExecutionPlanV1 {
                stages: vec![StagePlanV1::MovePath {
                    source: validate_path_value(&source.display().to_string()).unwrap(),
                    destination: validate_path_value(&destination.display().to_string()).unwrap(),
                    existing_destination: policy,
                }],
                redirect: None,
            },
        },
    }
}

fn sandbox() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "wingman-mv-test-{}-{}",
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
