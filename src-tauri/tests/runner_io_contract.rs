use std::fs;
use std::io::ErrorKind;
#[cfg(windows)]
use std::os::windows::fs::symlink_dir;
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::process::Command;
use uuid::Uuid;
use wingman_lib::runner_io::{
    prepare_file_io, IoPreparationErrorV1, RedirectModeV1, RedirectSpecV1,
};
use wingman_lib::text_stream::RecordFrameV1;

#[test]
fn missing_inputs_are_reported_in_operand_order_before_redirect_is_touched() {
    let sandbox = sandbox();
    let redirect_path = sandbox.join("out.txt");
    fs::write(&redirect_path, b"keep me").unwrap();
    let inputs = vec![sandbox.join("missing-a"), sandbox.join("missing-b")];

    let error = prepare_file_io(
        &inputs,
        Some(RedirectSpecV1 {
            path: redirect_path.clone(),
            mode: RedirectModeV1::Overwrite,
        }),
    )
    .expect_err("missing inputs must fail before output open");

    let IoPreparationErrorV1::Inputs(errors) = error else {
        panic!("expected ordered input failures");
    };
    assert_eq!(
        errors.iter().map(|error| error.index).collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(fs::read(&redirect_path).unwrap(), b"keep me");
    cleanup(&sandbox);
}

#[test]
fn overwrite_truncates_only_after_every_input_is_open() {
    let sandbox = sandbox();
    let input_path = sandbox.join("input.txt");
    let redirect_path = sandbox.join("out.txt");
    fs::write(&input_path, b"input").unwrap();
    fs::write(&redirect_path, b"old output").unwrap();

    let prepared = prepare_file_io(
        &[input_path],
        Some(RedirectSpecV1 {
            path: redirect_path.clone(),
            mode: RedirectModeV1::Overwrite,
        }),
    )
    .expect("prepare input and overwrite sink");

    assert_eq!(prepared.inputs().len(), 1);
    assert_eq!(fs::read(&redirect_path).unwrap(), b"");
    drop(prepared);
    cleanup(&sandbox);
}

#[test]
fn hard_link_alias_of_an_input_is_rejected_without_truncation() {
    let sandbox = sandbox();
    let input_path = sandbox.join("input.txt");
    let alias_path = sandbox.join("alias.txt");
    fs::write(&input_path, b"original").unwrap();
    fs::hard_link(&input_path, &alias_path).unwrap();

    let error = prepare_file_io(
        std::slice::from_ref(&input_path),
        Some(RedirectSpecV1 {
            path: alias_path.clone(),
            mode: RedirectModeV1::Overwrite,
        }),
    )
    .expect_err("same file identity must be rejected");

    assert_eq!(error, IoPreparationErrorV1::SameFile { input_index: 0 });
    assert_eq!(fs::read(&input_path).unwrap(), b"original");
    assert_eq!(fs::read(&alias_path).unwrap(), b"original");
    cleanup(&sandbox);
}

#[test]
fn append_adds_no_hidden_separator_or_bom() {
    let sandbox = sandbox();
    let redirect_path = sandbox.join("append.txt");
    fs::write(&redirect_path, b"existing").unwrap();
    let mut prepared = prepare_file_io(
        &[],
        Some(RedirectSpecV1 {
            path: redirect_path.clone(),
            mode: RedirectModeV1::Append,
        }),
    )
    .expect("prepare append sink");

    prepared
        .write_stdout_records(&[RecordFrameV1 {
            text: "한글".to_string(),
            terminated: true,
        }])
        .expect("append encoded record");
    drop(prepared);

    assert_eq!(
        fs::read(&redirect_path).unwrap(),
        [b"existing".as_slice(), "한글\r\n".as_bytes()].concat()
    );
    cleanup(&sandbox);
}

#[test]
fn output_open_failure_happens_after_inputs_and_before_any_stage_output() {
    let sandbox = sandbox();
    let input_path = sandbox.join("input.txt");
    let output_directory = sandbox.join("directory");
    fs::write(&input_path, b"input").unwrap();
    fs::create_dir(&output_directory).unwrap();

    let error = prepare_file_io(
        &[input_path],
        Some(RedirectSpecV1 {
            path: output_directory,
            mode: RedirectModeV1::Overwrite,
        }),
    )
    .expect_err("directory cannot be an output file");

    assert!(
        matches!(
            error,
            IoPreparationErrorV1::Output {
                kind: ErrorKind::PermissionDenied
                    | ErrorKind::IsADirectory
                    | ErrorKind::InvalidInput,
            }
        ),
        "unexpected directory output error: {error:?}"
    );
    cleanup(&sandbox);
}

#[cfg(windows)]
#[test]
fn existing_reparse_output_is_rejected_before_mutation() {
    let sandbox = sandbox();
    let target_directory = sandbox.join("target");
    let reparse_output = sandbox.join("redirect");
    fs::create_dir(&target_directory).unwrap();
    fs::write(target_directory.join("sentinel.txt"), b"keep me").unwrap();
    create_directory_reparse(&target_directory, &reparse_output);

    let error = prepare_file_io(
        &[],
        Some(RedirectSpecV1 {
            path: reparse_output.clone(),
            mode: RedirectModeV1::Overwrite,
        }),
    )
    .expect_err("a reparse output leaf must not be opened for writing");

    assert_eq!(error, IoPreparationErrorV1::OutputReparsePoint);
    assert_eq!(
        fs::read(target_directory.join("sentinel.txt")).unwrap(),
        b"keep me"
    );
    fs::remove_dir(&reparse_output).unwrap();
    cleanup(&sandbox);
}

#[cfg(windows)]
#[test]
fn reparse_output_ancestor_is_rejected_before_target_is_touched() {
    let sandbox = sandbox();
    let target_directory = sandbox.join("target");
    let reparse_ancestor = sandbox.join("linked-parent");
    let target_output = target_directory.join("out.txt");
    fs::create_dir(&target_directory).unwrap();
    fs::write(&target_output, b"keep me").unwrap();
    create_directory_reparse(&target_directory, &reparse_ancestor);

    let error = prepare_file_io(
        &[],
        Some(RedirectSpecV1 {
            path: reparse_ancestor.join("out.txt"),
            mode: RedirectModeV1::Overwrite,
        }),
    )
    .expect_err("a reparse output ancestor must stop preparation");

    assert_eq!(error, IoPreparationErrorV1::OutputReparsePoint);
    assert_eq!(fs::read(&target_output).unwrap(), b"keep me");
    fs::remove_dir(&reparse_ancestor).unwrap();
    cleanup(&sandbox);
}

#[cfg(windows)]
#[test]
fn reparse_output_ancestor_does_not_create_a_missing_target() {
    let sandbox = sandbox();
    let target_directory = sandbox.join("target");
    let reparse_ancestor = sandbox.join("linked-parent");
    let target_output = target_directory.join("new.txt");
    fs::create_dir(&target_directory).unwrap();
    create_directory_reparse(&target_directory, &reparse_ancestor);

    let error = prepare_file_io(
        &[],
        Some(RedirectSpecV1 {
            path: reparse_ancestor.join("new.txt"),
            mode: RedirectModeV1::Overwrite,
        }),
    )
    .expect_err("a reparse ancestor must be rejected before output creation");

    assert_eq!(error, IoPreparationErrorV1::OutputReparsePoint);
    assert!(!target_output.exists());
    fs::remove_dir(&reparse_ancestor).unwrap();
    cleanup(&sandbox);
}

fn sandbox() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "wingman-runner-io-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    ));
    fs::create_dir(&path).unwrap();
    path
}

#[cfg(windows)]
fn create_directory_reparse(target: &Path, link: &Path) {
    if symlink_dir(target, link).is_ok() {
        return;
    }

    let output = Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .expect("start cmd junction fixture helper");
    assert!(output.status.success(), "create junction fixture");
}

fn cleanup(path: &Path) {
    let _ = fs::remove_dir_all(path);
}
