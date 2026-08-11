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
fn file_copy_commits_complete_content_and_replaces_only_at_commit() {
    let sandbox = sandbox();
    let source = sandbox.join("한글 source.txt");
    let destination = sandbox.join("destination.txt");
    let content = vec![b'x'; 160_000];
    fs::write(&source, &content).unwrap();
    fs::write(&destination, b"old").unwrap();

    let outcome = execute_prepared(request(
        &source,
        &destination,
        ExistingDestinationPolicyV1::Replace,
    ))
    .expect("copy file");

    assert_eq!(outcome.exit_code, 0);
    assert!(outcome.stdout.is_empty());
    assert!(outcome.stderr.is_empty());
    assert_eq!(fs::read(&source).unwrap(), content);
    assert_eq!(fs::read(&destination).unwrap(), content);
    assert_no_staging_artifacts(&sandbox);
    cleanup(&sandbox);
}

#[test]
fn copy_into_an_existing_directory_uses_the_source_basename() {
    let sandbox = sandbox();
    let source = sandbox.join("source.txt");
    let destination_directory = sandbox.join("destination");
    fs::write(&source, b"content").unwrap();
    fs::create_dir(&destination_directory).unwrap();

    let outcome = execute_prepared(request(
        &source,
        &destination_directory,
        ExistingDestinationPolicyV1::Replace,
    ))
    .expect("copy into directory");

    assert_eq!(outcome.exit_code, 0);
    assert_eq!(
        fs::read(destination_directory.join("source.txt")).unwrap(),
        b"content"
    );
    assert_no_staging_artifacts(&destination_directory);
    cleanup(&sandbox);
}

#[test]
fn no_clobber_skips_an_existing_file_without_staging() {
    let sandbox = sandbox();
    let source = sandbox.join("source.txt");
    let destination = sandbox.join("destination.txt");
    fs::write(&source, b"new").unwrap();
    fs::write(&destination, b"old").unwrap();

    let outcome = execute_prepared(request(
        &source,
        &destination,
        ExistingDestinationPolicyV1::NoClobber,
    ))
    .expect("skip existing file");

    assert_eq!(outcome.exit_code, 0);
    assert_eq!(fs::read(&destination).unwrap(), b"old");
    assert_no_staging_artifacts(&sandbox);
    cleanup(&sandbox);
}

#[test]
fn force_replaces_a_readonly_destination_at_the_commit_boundary() {
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
    .expect("force copy");

    assert_eq!(outcome.exit_code, 0);
    assert_eq!(fs::read(&destination).unwrap(), b"new");
    assert_no_staging_artifacts(&sandbox);
    cleanup(&sandbox);
}

#[test]
fn same_file_hard_link_and_reparse_destination_are_no_mutation_rejections() {
    let sandbox = sandbox();
    let outside = sandbox.with_extension("outside");
    let source = sandbox.join("source.txt");
    let hard_link = sandbox.join("hard-link.txt");
    let reparse = sandbox.join("reparse");
    fs::write(&source, b"preserve").unwrap();
    fs::hard_link(&source, &hard_link).unwrap();
    fs::create_dir(&outside).unwrap();
    create_directory_reparse(&outside, &reparse);

    let hard_link_outcome = execute_prepared(request(
        &source,
        &hard_link,
        ExistingDestinationPolicyV1::Replace,
    ))
    .expect("reject same identity");
    assert_eq!(hard_link_outcome.exit_code, 2);
    assert_eq!(fs::read(&source).unwrap(), b"preserve");

    let reparse_outcome = execute_prepared(request(
        &source,
        &reparse.join("copied.txt"),
        ExistingDestinationPolicyV1::Replace,
    ))
    .expect("reject reparse destination");
    assert_eq!(reparse_outcome.exit_code, 2);
    assert!(!outside.join("copied.txt").exists());
    assert_no_staging_artifacts(&sandbox);

    fs::remove_dir(&reparse).unwrap();
    cleanup(&sandbox);
    cleanup(&outside);
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
                stages: vec![StagePlanV1::CopyPath {
                    source: validate_path_value(&source.display().to_string()).unwrap(),
                    destination: validate_path_value(&destination.display().to_string()).unwrap(),
                    recursive: false,
                    existing_destination: policy,
                }],
                redirect: None,
            },
        },
    }
}

fn assert_no_staging_artifacts(path: &Path) {
    assert!(fs::read_dir(path).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".wingman-stage-")
    }));
}

fn sandbox() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "wingman-cp-test-{}-{}",
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
