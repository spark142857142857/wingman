use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;
use wingman_lib::interpreter::{
    ExecutionPlanV1, PreparedRequestKindV1, PreparedRequestV1, RedirectModeV1, StagePlanV1,
    ValidatedRedirectPlanV1,
};
use wingman_lib::runner::execute_prepared;
use wingman_lib::runner_cancel::RunnerCancellationV1;
use wingman_lib::windows_path::{validate_path_value, ValidatedPathSpecV1};

#[test]
fn recursive_grep_walks_depth_first_in_deterministic_name_order() {
    let sandbox = sandbox();
    let root = sandbox.join("root");
    fs::create_dir_all(root.join("A-dir")).unwrap();
    fs::create_dir_all(root.join("C-dir")).unwrap();
    fs::write(root.join("A-dir").join("z.txt"), b"TODO a\n").unwrap();
    fs::write(root.join("b.txt"), b"TODO b\n").unwrap();
    fs::write(root.join("C-dir").join("a.txt"), b"TODO c\n").unwrap();

    let outcome = execute_prepared(request(&root, "TODO", true)).unwrap();

    let display = root.display().to_string().replace('/', "\\");
    assert_eq!(outcome.exit_code, 0);
    assert_eq!(
        String::from_utf8(outcome.stdout).unwrap(),
        format!(
            "{display}\\A-dir\\z.txt:1:TODO a\r\n{display}\\b.txt:1:TODO b\r\n{display}\\C-dir\\a.txt:1:TODO c\r\n"
        )
    );
    assert!(outcome.stderr.is_empty());
    cleanup(&sandbox);
}

#[test]
fn recursive_grep_keeps_matches_and_continues_after_a_decode_failure() {
    let sandbox = sandbox();
    let root = sandbox.join("root");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("a-bad.txt"),
        [b"TODO before\n".as_slice(), &[0xff]].concat(),
    )
    .unwrap();
    fs::write(root.join("b-good.txt"), b"TODO after\n").unwrap();

    let outcome = execute_prepared(request(&root, "TODO", false)).unwrap();

    let display = root.display().to_string().replace('/', "\\");
    assert_eq!(outcome.exit_code, 1);
    assert_eq!(
        String::from_utf8(outcome.stdout).unwrap(),
        format!("{display}\\a-bad.txt:TODO before\r\n{display}\\b-good.txt:TODO after\r\n")
    );
    assert!(String::from_utf8(outcome.stderr)
        .unwrap()
        .contains("input is not valid bounded UTF-8 text"));
    cleanup(&sandbox);
}

#[test]
fn recursive_grep_never_descends_into_a_discovered_reparse_directory() {
    let sandbox = sandbox();
    let root = sandbox.join("root");
    let outside = sandbox.join("outside");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(root.join("visible.txt"), b"TODO visible\n").unwrap();
    fs::write(outside.join("hidden.txt"), b"TODO hidden\n").unwrap();
    let link = root.join("linked");
    create_directory_reparse(&outside, &link);

    let outcome = execute_prepared(request(&root, "TODO", false)).unwrap();

    let stdout = String::from_utf8(outcome.stdout).unwrap();
    assert_eq!(outcome.exit_code, 0);
    assert!(stdout.contains("visible.txt:TODO visible"));
    assert!(!stdout.contains("hidden"));
    assert!(outcome.stderr.is_empty());
    fs::remove_dir(&link).unwrap();
    cleanup(&sandbox);
}

#[test]
fn recursive_grep_no_match_and_pre_cancel_have_distinct_results() {
    let sandbox = sandbox();
    let root = sandbox.join("root");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("input.txt"), b"nothing\n").unwrap();

    let no_match = execute_prepared(request(&root, "TODO", false)).unwrap();
    assert_eq!(no_match.exit_code, 1);
    assert!(no_match.stdout.is_empty());
    assert!(no_match.stderr.is_empty());

    let cancellation = RunnerCancellationV1::new();
    cancellation.cancel();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit_code = wingman_lib::runner::execute_prepared_to_with_cancellation(
        request(&root, "TODO", false),
        &mut stdout,
        &mut stderr,
        &cancellation,
    )
    .unwrap();
    assert_eq!(exit_code, 130);
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
    cleanup(&sandbox);
}

#[test]
fn recursive_grep_uses_the_safe_redirect_sink_and_rejects_an_input_alias() {
    let sandbox = sandbox();
    let root = sandbox.join("root");
    fs::create_dir_all(&root).unwrap();
    let input = root.join("input.txt");
    let output = sandbox.join("output.txt");
    fs::write(&input, b"TODO redirected\n").unwrap();

    let redirected = execute_prepared(request_with_redirect(&root, "TODO", &output)).unwrap();
    assert_eq!(redirected.exit_code, 0);
    assert!(redirected.stdout.is_empty());
    assert!(redirected.stderr.is_empty());
    let display = root.display().to_string().replace('/', "\\");
    assert_eq!(
        String::from_utf8(fs::read(&output).unwrap()).unwrap(),
        format!("{display}\\input.txt:TODO redirected\r\n")
    );

    let unchanged = fs::read(&input).unwrap();
    let same_file = execute_prepared(request_with_redirect(&root, "TODO", &input)).unwrap();
    assert_eq!(same_file.exit_code, 2);
    assert!(same_file.stdout.is_empty());
    assert!(String::from_utf8(same_file.stderr)
        .unwrap()
        .contains("same file as recursive input"));
    assert_eq!(fs::read(&input).unwrap(), unchanged);

    let hard_link_alias = sandbox.join("input-alias.txt");
    fs::hard_link(&input, &hard_link_alias).unwrap();
    let hard_link = execute_prepared(request_with_redirect(&root, "TODO", &hard_link_alias))
        .expect("reject a recursive input hard-link alias");
    assert_eq!(hard_link.exit_code, 2);
    assert!(String::from_utf8(hard_link.stderr)
        .unwrap()
        .contains("same file as recursive input"));
    assert_eq!(fs::read(&input).unwrap(), unchanged);

    let unrelated = sandbox.join("unrelated-output.txt");
    let unrelated_alias = sandbox.join("unrelated-output-alias.txt");
    fs::write(&unrelated, b"old\n").unwrap();
    fs::hard_link(&unrelated, &unrelated_alias).unwrap();
    let unrelated_output = execute_prepared(request_with_redirect(&root, "TODO", &unrelated))
        .expect("allow a multiply-linked output disjoint from recursive inputs");
    assert_eq!(unrelated_output.exit_code, 0);
    assert_eq!(
        String::from_utf8(fs::read(&unrelated_alias).unwrap()).unwrap(),
        format!("{display}\\input.txt:TODO redirected\r\n")
    );
    cleanup(&sandbox);
}

#[test]
fn recursive_grep_head_stops_before_opening_a_later_invalid_file() {
    let sandbox = sandbox();
    let root = sandbox.join("root");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("a-match.txt"), b"TODO first\n").unwrap();
    fs::write(root.join("z-invalid.txt"), [0xff, 0xfe]).unwrap();

    let outcome = execute_prepared(request_with_downstream(
        &root,
        "TODO",
        vec![StagePlanV1::HeadLines {
            count: 1,
            path: None,
        }],
    ))
    .unwrap();

    let display = root.display().to_string().replace('/', "\\");
    assert_eq!(outcome.exit_code, 0);
    assert_eq!(
        String::from_utf8(outcome.stdout).unwrap(),
        format!("{display}\\a-match.txt:TODO first\r\n")
    );
    assert!(outcome.stderr.is_empty());

    let output = sandbox.join("head-output.txt");
    let mut redirected_request = request_with_downstream(
        &root,
        "TODO",
        vec![StagePlanV1::HeadLines {
            count: 1,
            path: None,
        }],
    );
    let PreparedRequestKindV1::Execute { plan } = &mut redirected_request.kind else {
        unreachable!();
    };
    plan.redirect = Some(ValidatedRedirectPlanV1 {
        mode: RedirectModeV1::Overwrite,
        path: path_spec(&output),
    });
    let redirected = execute_prepared(redirected_request).unwrap();
    assert_eq!(redirected.exit_code, 0);
    assert!(redirected.stdout.is_empty());
    assert!(redirected.stderr.is_empty());
    assert_eq!(
        String::from_utf8(fs::read(output).unwrap()).unwrap(),
        format!("{display}\\a-match.txt:TODO first\r\n")
    );
    cleanup(&sandbox);
}

#[test]
fn recursive_grep_head_does_not_traverse_a_later_overdepth_tree() {
    let sandbox = sandbox();
    let root = sandbox.join("root");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("a-match.txt"), b"TODO first\n").unwrap();
    let mut deep = root.join("z-deep");
    fs::create_dir(&deep).unwrap();
    for depth in 0..=wingman_lib::runner_grep::MAX_RECURSIVE_GREP_DEPTH {
        deep = deep.join(format!("d{depth}"));
        fs::create_dir(&deep).unwrap();
    }

    let outcome = execute_prepared(request_with_downstream(
        &root,
        "TODO",
        vec![StagePlanV1::HeadLines {
            count: 1,
            path: None,
        }],
    ))
    .unwrap();

    let display = root.display().to_string().replace('/', "\\");
    assert_eq!(outcome.exit_code, 0);
    assert_eq!(
        String::from_utf8(outcome.stdout).unwrap(),
        format!("{display}\\a-match.txt:TODO first\r\n")
    );
    assert!(outcome.stderr.is_empty());
    cleanup(&sandbox);
}

#[test]
fn recursive_grep_can_feed_finite_tail_and_wc() {
    let sandbox = sandbox();
    let root = sandbox.join("root");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("a.txt"), b"TODO one\nTODO two\n").unwrap();
    fs::write(root.join("b.txt"), b"TODO three\n").unwrap();

    let tailed = execute_prepared(request_with_downstream(
        &root,
        "TODO",
        vec![StagePlanV1::TailLines {
            count: 2,
            path: None,
        }],
    ))
    .unwrap();
    let display = root.display().to_string().replace('/', "\\");
    assert_eq!(tailed.exit_code, 0);
    assert_eq!(
        String::from_utf8(tailed.stdout).unwrap(),
        format!("{display}\\a.txt:TODO two\r\n{display}\\b.txt:TODO three\r\n")
    );

    let counted = execute_prepared(request_with_downstream(
        &root,
        "TODO",
        vec![StagePlanV1::CountLines { path: None }],
    ))
    .unwrap();
    assert_eq!(counted.exit_code, 0);
    assert_eq!(counted.stdout, b"3\r\n");
    assert!(counted.stderr.is_empty());
    cleanup(&sandbox);
}

#[test]
fn recursive_grep_uses_the_common_repeated_sort_and_uniq_stages() {
    let sandbox = sandbox();
    let root = sandbox.join("root");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("input.txt"),
        b"TODO zebra\nTODO repeat\nTODO repeat\nTODO alpha\nignore\n",
    )
    .unwrap();

    let sorted = execute_prepared(request_with_downstream(
        &root,
        "TODO",
        vec![
            StagePlanV1::SearchText {
                pattern: "TODO".to_string(),
                paths: Vec::new(),
                ignore_case: false,
                line_numbers: false,
                invert_match: false,
                fixed_strings: false,
                recursive: false,
            },
            StagePlanV1::SortLines {
                path: None,
                reverse: false,
                numeric: false,
                unique: false,
            },
        ],
    ))
    .unwrap();
    let display = root.display().to_string().replace('/', "\\");
    assert_eq!(sorted.exit_code, 0);
    assert_eq!(
        String::from_utf8(sorted.stdout).unwrap(),
        format!(
            "{display}\\input.txt:TODO alpha\r\n{display}\\input.txt:TODO repeat\r\n{display}\\input.txt:TODO repeat\r\n{display}\\input.txt:TODO zebra\r\n"
        )
    );

    let unique = execute_prepared(request_with_downstream(
        &root,
        "repeat",
        vec![StagePlanV1::UniqueLines {
            path: None,
            count: true,
            repeated_only: false,
            unique_only: false,
        }],
    ))
    .unwrap();
    assert_eq!(unique.exit_code, 0);
    assert_eq!(
        String::from_utf8(unique.stdout).unwrap(),
        format!("2 {display}\\input.txt:TODO repeat\r\n")
    );
    assert!(unique.stderr.is_empty());
    cleanup(&sandbox);
}

fn request(root: &Path, pattern: &str, line_numbers: bool) -> PreparedRequestV1 {
    PreparedRequestV1 {
        protocol: "wingman.run".to_string(),
        version: 1,
        kind: PreparedRequestKindV1::Execute {
            plan: ExecutionPlanV1 {
                stages: vec![StagePlanV1::SearchText {
                    pattern: pattern.to_string(),
                    paths: vec![path_spec(root)],
                    ignore_case: false,
                    line_numbers,
                    invert_match: false,
                    fixed_strings: false,
                    recursive: true,
                }],
                redirect: None,
            },
        },
    }
}

fn request_with_redirect(root: &Path, pattern: &str, output: &Path) -> PreparedRequestV1 {
    let mut request = request(root, pattern, false);
    let PreparedRequestKindV1::Execute { plan } = &mut request.kind else {
        unreachable!();
    };
    plan.redirect = Some(ValidatedRedirectPlanV1 {
        mode: RedirectModeV1::Overwrite,
        path: path_spec(output),
    });
    request
}

fn request_with_downstream(
    root: &Path,
    pattern: &str,
    downstream: Vec<StagePlanV1>,
) -> PreparedRequestV1 {
    let mut request = request(root, pattern, false);
    let PreparedRequestKindV1::Execute { plan } = &mut request.kind else {
        unreachable!();
    };
    plan.stages.extend(downstream);
    request
}

fn path_spec(path: &Path) -> ValidatedPathSpecV1 {
    validate_path_value(path.to_str().unwrap()).unwrap()
}

fn sandbox() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "wingman-grep-contract-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    ));
    fs::create_dir(&path).unwrap();
    path
}

fn cleanup(path: &Path) {
    fs::remove_dir_all(path).unwrap();
}

fn create_directory_reparse(target: &Path, link: &Path) {
    if std::os::windows::fs::symlink_dir(target, link).is_ok() {
        return;
    }
    let output = std::process::Command::new("cmd.exe")
        .args([
            "/d",
            "/c",
            "mklink",
            "/J",
            link.to_str().unwrap(),
            target.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "failed to create test junction: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
