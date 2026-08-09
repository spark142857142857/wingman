use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;
use wingman_lib::interpreter::{
    ExecutionPlanV1, PreparedRequestKindV1, PreparedRequestV1, RedirectModeV1, StagePlanV1,
    ValidatedRedirectPlanV1,
};
use wingman_lib::runner::{
    execute_prepared, execute_prepared_to, execute_prepared_to_with_cancellation,
    RunnerDispatchErrorV1,
};
use wingman_lib::runner_cancel::RunnerCancellationV1;
use wingman_lib::windows_path::validate_path_value;

#[test]
fn cat_joins_file_boundaries_and_numbers_the_resulting_records() {
    let sandbox = sandbox();
    let first = sandbox.join("first.txt");
    let second = sandbox.join("second.txt");
    fs::write(&first, b"hel").unwrap();
    fs::write(&second, b"\xef\xbb\xbflo\n\nlast").unwrap();

    let outcome = execute_prepared(request(vec![StagePlanV1::ReadTextFiles {
        paths: vec![path_spec(&first), path_spec(&second)],
        number_lines: true,
    }]))
    .expect("execute cat plan");

    assert_eq!(outcome.exit_code, 0);
    assert!(outcome.stderr.is_empty());
    assert_eq!(outcome.stdout, b"     1\thello\r\n     2\t\r\n     3\tlast");
    cleanup(&sandbox);
}

#[test]
fn head_normal_stop_does_not_decode_an_invalid_buffered_suffix() {
    let sandbox = sandbox();
    let input = sandbox.join("input.txt");
    fs::write(&input, [b"first\n".as_slice(), &[0xff, 0xfe]].concat()).unwrap();

    let outcome = execute_prepared(request(vec![
        StagePlanV1::ReadTextFiles {
            paths: vec![path_spec(&input)],
            number_lines: false,
        },
        StagePlanV1::HeadLines {
            count: 1,
            path: None,
        },
    ]))
    .expect("execute early-stop plan");

    assert_eq!(outcome.exit_code, 0);
    assert!(outcome.stderr.is_empty());
    assert_eq!(outcome.stdout, b"first\r\n");
    cleanup(&sandbox);
}

#[test]
fn invalid_text_before_head_completion_is_an_operational_failure() {
    let sandbox = sandbox();
    let input = sandbox.join("bad.txt");
    fs::write(&input, [b"good".as_slice(), &[0xff], b"\n"].concat()).unwrap();

    let outcome = execute_prepared(request(vec![StagePlanV1::HeadLines {
        count: 1,
        path: Some(path_spec(&input)),
    }]))
    .expect("return operational outcome");

    assert_eq!(outcome.exit_code, 1);
    assert!(outcome.stdout.is_empty());
    assert!(String::from_utf8(outcome.stderr)
        .unwrap()
        .contains("input is not valid bounded UTF-8 text"));
    cleanup(&sandbox);
}

#[test]
fn output_failure_stops_the_writer_based_execution_path() {
    let sandbox = sandbox();
    let input = sandbox.join("input.txt");
    fs::write(&input, b"first\nsecond\n").unwrap();
    let mut stdout = FailingWriter { remaining: 3 };
    let mut stderr = Vec::new();

    let error = execute_prepared_to(
        request(vec![StagePlanV1::ReadTextFiles {
            paths: vec![path_spec(&input)],
            number_lines: false,
        }]),
        &mut stdout,
        &mut stderr,
    )
    .expect_err("stdout failure must stop execution");

    assert_eq!(
        error,
        RunnerDispatchErrorV1::OutputFailure {
            kind: io::ErrorKind::BrokenPipe
        }
    );
    assert!(stderr.is_empty());
    cleanup(&sandbox);
}

#[test]
fn head_zero_still_opens_its_explicit_input_before_succeeding() {
    let sandbox = sandbox();
    let missing = sandbox.join("missing.txt");

    let outcome = execute_prepared(request(vec![StagePlanV1::HeadLines {
        count: 0,
        path: Some(path_spec(&missing)),
    }]))
    .expect("return startup-open outcome");

    assert_eq!(outcome.exit_code, 1);
    assert!(outcome.stdout.is_empty());
    assert!(String::from_utf8(outcome.stderr)
        .unwrap()
        .contains("input cannot be opened"));
    cleanup(&sandbox);
}

#[test]
fn cat_keeps_completed_records_and_continues_after_one_source_decode_failure() {
    let sandbox = sandbox();
    let bad = sandbox.join("bad.txt");
    let later = sandbox.join("later.txt");
    fs::write(&bad, [b"good\npartial".as_slice(), &[0xff]].concat()).unwrap();
    fs::write(&later, b"later\n").unwrap();

    let outcome = execute_prepared(request(vec![StagePlanV1::ReadTextFiles {
        paths: vec![path_spec(&bad), path_spec(&later)],
        number_lines: false,
    }]))
    .expect("return partial operational outcome");

    assert_eq!(outcome.exit_code, 1);
    assert_eq!(outcome.stdout, b"good\r\nlater\r\n");
    assert!(String::from_utf8(outcome.stderr)
        .unwrap()
        .contains("input is not valid bounded UTF-8 text"));
    cleanup(&sandbox);
}

#[test]
fn cancellation_before_execution_wins_without_opening_inputs_or_emitting_diagnostics() {
    let sandbox = sandbox();
    let missing = sandbox.join("missing.txt");
    let cancellation = RunnerCancellationV1::new();
    cancellation.cancel();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let exit_code = execute_prepared_to_with_cancellation(
        request(vec![StagePlanV1::ReadTextFiles {
            paths: vec![path_spec(&missing)],
            number_lines: false,
        }]),
        &mut stdout,
        &mut stderr,
        &cancellation,
    )
    .expect("cancel request cleanly");

    assert_eq!(exit_code, 130);
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
    cleanup(&sandbox);
}

#[test]
fn cancellation_during_streaming_keeps_only_completed_output() {
    let sandbox = sandbox();
    let input = sandbox.join("input.txt");
    fs::write(&input, b"first\nsecond\nthird\n").unwrap();
    let cancellation = RunnerCancellationV1::new();
    let mut stdout = CancellingWriter {
        bytes: Vec::new(),
        cancellation: cancellation.clone(),
    };
    let mut stderr = Vec::new();

    let exit_code = execute_prepared_to_with_cancellation(
        request(vec![StagePlanV1::ReadTextFiles {
            paths: vec![path_spec(&input)],
            number_lines: false,
        }]),
        &mut stdout,
        &mut stderr,
        &cancellation,
    )
    .expect("cancel streaming request cleanly");

    assert_eq!(exit_code, 130);
    assert_eq!(stdout.bytes, b"first\r\n");
    assert!(stderr.is_empty());
    cleanup(&sandbox);
}

#[test]
fn cancellation_wins_when_the_output_operation_fails_at_the_same_boundary() {
    let sandbox = sandbox();
    let input = sandbox.join("input.txt");
    fs::write(&input, b"first\nsecond\n").unwrap();
    let cancellation = RunnerCancellationV1::new();
    let mut stdout = CancellingFailureWriter {
        cancellation: cancellation.clone(),
    };
    let mut stderr = Vec::new();

    let exit_code = execute_prepared_to_with_cancellation(
        request(vec![StagePlanV1::ReadTextFiles {
            paths: vec![path_spec(&input)],
            number_lines: false,
        }]),
        &mut stdout,
        &mut stderr,
        &cancellation,
    )
    .expect("cancellation must dominate the simultaneous output failure");

    assert_eq!(exit_code, 130);
    assert!(stderr.is_empty());
    cleanup(&sandbox);
}

#[test]
fn overwrite_redirection_streams_normalized_records_without_using_stdout() {
    let sandbox = sandbox();
    let input = sandbox.join("input.txt");
    let output = sandbox.join("output.txt");
    fs::write(&input, b"first\nsecond").unwrap();
    fs::write(&output, b"old output").unwrap();

    let outcome = execute_prepared(request_with_redirect(
        vec![StagePlanV1::ReadTextFiles {
            paths: vec![path_spec(&input)],
            number_lines: false,
        }],
        &output,
        RedirectModeV1::Overwrite,
    ))
    .expect("execute redirected cat plan");

    assert_eq!(outcome.exit_code, 0);
    assert!(outcome.stdout.is_empty());
    assert!(outcome.stderr.is_empty());
    assert_eq!(fs::read(&output).unwrap(), b"first\r\nsecond");
    cleanup(&sandbox);
}

#[test]
fn append_redirection_adds_no_separator_or_bom() {
    let sandbox = sandbox();
    let input = sandbox.join("input.txt");
    let output = sandbox.join("output.txt");
    fs::write(&input, "한글\n끝").unwrap();
    fs::write(&output, b"existing").unwrap();

    let outcome = execute_prepared(request_with_redirect(
        vec![StagePlanV1::ReadTextFiles {
            paths: vec![path_spec(&input)],
            number_lines: false,
        }],
        &output,
        RedirectModeV1::Append,
    ))
    .expect("execute redirected append plan");

    assert_eq!(outcome.exit_code, 0);
    assert!(outcome.stdout.is_empty());
    assert!(outcome.stderr.is_empty());
    assert_eq!(
        fs::read(&output).unwrap(),
        [b"existing".as_slice(), "한글\r\n끝".as_bytes()].concat()
    );
    cleanup(&sandbox);
}

#[test]
fn redirected_missing_input_leaves_the_existing_target_untouched() {
    let sandbox = sandbox();
    let missing = sandbox.join("missing.txt");
    let output = sandbox.join("output.txt");
    fs::write(&output, b"keep me").unwrap();

    let outcome = execute_prepared(request_with_redirect(
        vec![StagePlanV1::ReadTextFiles {
            paths: vec![path_spec(&missing)],
            number_lines: false,
        }],
        &output,
        RedirectModeV1::Overwrite,
    ))
    .expect("report redirected startup failure");

    assert_eq!(outcome.exit_code, 1);
    assert!(outcome.stdout.is_empty());
    assert!(String::from_utf8(outcome.stderr)
        .unwrap()
        .contains("input cannot be opened"));
    assert_eq!(fs::read(&output).unwrap(), b"keep me");
    cleanup(&sandbox);
}

#[test]
fn redirected_hard_link_alias_is_a_safety_rejection_without_truncation() {
    let sandbox = sandbox();
    let input = sandbox.join("input.txt");
    let alias = sandbox.join("alias.txt");
    fs::write(&input, b"keep me").unwrap();
    fs::hard_link(&input, &alias).unwrap();

    let outcome = execute_prepared(request_with_redirect(
        vec![StagePlanV1::ReadTextFiles {
            paths: vec![path_spec(&input)],
            number_lines: false,
        }],
        &alias,
        RedirectModeV1::Overwrite,
    ))
    .expect("reject the same output file safely");

    assert_eq!(outcome.exit_code, 2);
    assert!(outcome.stdout.is_empty());
    assert!(String::from_utf8(outcome.stderr)
        .unwrap()
        .contains("same file as input #1"));
    assert_eq!(fs::read(&input).unwrap(), b"keep me");
    assert_eq!(fs::read(&alias).unwrap(), b"keep me");
    cleanup(&sandbox);
}

#[test]
fn redirected_head_zero_opens_input_then_truncates_without_reading_payload() {
    let sandbox = sandbox();
    let input = sandbox.join("input.txt");
    let output = sandbox.join("output.txt");
    fs::write(&input, [0xff, 0xfe]).unwrap();
    fs::write(&output, b"old output").unwrap();

    let outcome = execute_prepared(request_with_redirect(
        vec![StagePlanV1::HeadLines {
            count: 0,
            path: Some(path_spec(&input)),
        }],
        &output,
        RedirectModeV1::Overwrite,
    ))
    .expect("execute redirected head zero plan");

    assert_eq!(outcome.exit_code, 0);
    assert!(outcome.stdout.is_empty());
    assert!(outcome.stderr.is_empty());
    assert!(fs::read(&output).unwrap().is_empty());
    cleanup(&sandbox);
}

#[test]
fn redirected_runtime_decode_failure_keeps_completed_file_output_and_uses_stderr() {
    let sandbox = sandbox();
    let input = sandbox.join("input.txt");
    let output = sandbox.join("output.txt");
    fs::write(&input, [b"good\npartial".as_slice(), &[0xff]].concat()).unwrap();

    let outcome = execute_prepared(request_with_redirect(
        vec![StagePlanV1::ReadTextFiles {
            paths: vec![path_spec(&input)],
            number_lines: false,
        }],
        &output,
        RedirectModeV1::Overwrite,
    ))
    .expect("return redirected partial operational outcome");

    assert_eq!(outcome.exit_code, 1);
    assert!(outcome.stdout.is_empty());
    assert!(String::from_utf8(outcome.stderr)
        .unwrap()
        .contains("input is not valid bounded UTF-8 text"));
    assert_eq!(fs::read(&output).unwrap(), b"good\r\n");
    cleanup(&sandbox);
}

#[test]
fn redirected_directory_target_is_an_operational_failure_before_stage_output() {
    let sandbox = sandbox();
    let input = sandbox.join("input.txt");
    let output = sandbox.join("directory");
    fs::write(&input, b"must not be emitted\n").unwrap();
    fs::create_dir(&output).unwrap();

    let outcome = execute_prepared(request_with_redirect(
        vec![StagePlanV1::ReadTextFiles {
            paths: vec![path_spec(&input)],
            number_lines: false,
        }],
        &output,
        RedirectModeV1::Overwrite,
    ))
    .expect("return redirected output-open failure");

    assert_eq!(outcome.exit_code, 1);
    assert!(outcome.stdout.is_empty());
    assert!(String::from_utf8(outcome.stderr)
        .unwrap()
        .contains("redirection target cannot be opened"));
    assert!(output.is_dir());
    cleanup(&sandbox);
}

#[test]
fn cancellation_before_redirect_preparation_leaves_existing_target_untouched() {
    let sandbox = sandbox();
    let input = sandbox.join("input.txt");
    let output = sandbox.join("output.txt");
    fs::write(&input, b"input\n").unwrap();
    fs::write(&output, b"keep me").unwrap();
    let cancellation = RunnerCancellationV1::new();
    cancellation.cancel();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let exit_code = execute_prepared_to_with_cancellation(
        request_with_redirect(
            vec![StagePlanV1::ReadTextFiles {
                paths: vec![path_spec(&input)],
                number_lines: false,
            }],
            &output,
            RedirectModeV1::Overwrite,
        ),
        &mut stdout,
        &mut stderr,
        &cancellation,
    )
    .expect("cancel before redirected preparation");

    assert_eq!(exit_code, 130);
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
    assert_eq!(fs::read(&output).unwrap(), b"keep me");
    cleanup(&sandbox);
}

#[test]
fn wc_lines_counts_only_terminated_records() {
    let sandbox = sandbox();
    let input = sandbox.join("input.txt");
    fs::write(&input, b"first\nsecond\r\nunterminated").unwrap();

    let outcome = execute_prepared(request(vec![StagePlanV1::CountLines {
        path: Some(path_spec(&input)),
    }]))
    .expect("execute wc line count");

    assert_eq!(outcome.exit_code, 0);
    assert_eq!(outcome.stdout, b"2\r\n");
    assert!(outcome.stderr.is_empty());
    cleanup(&sandbox);
}

#[test]
fn cat_head_wc_stops_before_invalid_suffix_and_emits_one_generated_record() {
    let sandbox = sandbox();
    let input = sandbox.join("input.txt");
    fs::write(&input, [b"first\n".as_slice(), &[0xff, 0xfe]].concat()).unwrap();

    let outcome = execute_prepared(request(vec![
        StagePlanV1::ReadTextFiles {
            paths: vec![path_spec(&input)],
            number_lines: false,
        },
        StagePlanV1::HeadLines {
            count: 1,
            path: None,
        },
        StagePlanV1::CountLines { path: None },
    ]))
    .expect("execute cat/head/wc pipeline");

    assert_eq!(outcome.exit_code, 0);
    assert_eq!(outcome.stdout, b"1\r\n");
    assert!(outcome.stderr.is_empty());
    cleanup(&sandbox);
}

#[test]
fn wc_lines_uses_the_existing_safe_redirection_sink() {
    let sandbox = sandbox();
    let input = sandbox.join("input.txt");
    let output = sandbox.join("count.txt");
    fs::write(&input, b"first\nlast").unwrap();

    let outcome = execute_prepared(request_with_redirect(
        vec![StagePlanV1::CountLines {
            path: Some(path_spec(&input)),
        }],
        &output,
        RedirectModeV1::Overwrite,
    ))
    .expect("execute redirected wc line count");

    assert_eq!(outcome.exit_code, 0);
    assert!(outcome.stdout.is_empty());
    assert!(outcome.stderr.is_empty());
    assert_eq!(fs::read(&output).unwrap(), b"1\r\n");
    cleanup(&sandbox);
}

#[test]
fn head_zero_then_wc_emits_zero_without_decoding_input() {
    let sandbox = sandbox();
    let input = sandbox.join("input.txt");
    fs::write(&input, [0xff, 0xfe]).unwrap();

    let outcome = execute_prepared(request(vec![
        StagePlanV1::HeadLines {
            count: 0,
            path: Some(path_spec(&input)),
        },
        StagePlanV1::CountLines { path: None },
    ]))
    .expect("execute head-zero/wc pipeline");

    assert_eq!(outcome.exit_code, 0);
    assert_eq!(outcome.stdout, b"0\r\n");
    assert!(outcome.stderr.is_empty());
    cleanup(&sandbox);
}

#[test]
fn wc_runtime_decode_failure_reports_partial_terminated_count() {
    let sandbox = sandbox();
    let input = sandbox.join("input.txt");
    fs::write(&input, [b"good\npartial".as_slice(), &[0xff]].concat()).unwrap();

    let outcome = execute_prepared(request(vec![StagePlanV1::CountLines {
        path: Some(path_spec(&input)),
    }]))
    .expect("return partial wc operational outcome");

    assert_eq!(outcome.exit_code, 1);
    assert_eq!(outcome.stdout, b"1\r\n");
    assert!(String::from_utf8(outcome.stderr)
        .unwrap()
        .contains("input is not valid bounded UTF-8 text"));
    cleanup(&sandbox);
}

#[test]
fn finite_tail_preserves_the_final_unterminated_record() {
    let sandbox = sandbox();
    let input = sandbox.join("input.txt");
    fs::write(&input, b"one\nsecond\nlast").unwrap();

    let outcome = execute_prepared(request(vec![StagePlanV1::TailLines {
        count: 2,
        path: Some(path_spec(&input)),
    }]))
    .expect("execute finite tail");

    assert_eq!(outcome.exit_code, 0);
    assert_eq!(outcome.stdout, b"second\r\nlast");
    assert!(outcome.stderr.is_empty());
    cleanup(&sandbox);
}

#[test]
fn tail_zero_opens_but_does_not_decode_the_input() {
    let sandbox = sandbox();
    let input = sandbox.join("input.txt");
    fs::write(&input, [0xff, 0xfe]).unwrap();

    let outcome = execute_prepared(request(vec![StagePlanV1::TailLines {
        count: 0,
        path: Some(path_spec(&input)),
    }]))
    .expect("execute tail zero");

    assert_eq!(outcome.exit_code, 0);
    assert!(outcome.stdout.is_empty());
    assert!(outcome.stderr.is_empty());
    cleanup(&sandbox);
}

#[test]
fn cat_numbering_happens_before_tail_selection() {
    let sandbox = sandbox();
    let input = sandbox.join("input.txt");
    fs::write(&input, b"one\ntwo\nthree\n").unwrap();

    let outcome = execute_prepared(request(vec![
        StagePlanV1::ReadTextFiles {
            paths: vec![path_spec(&input)],
            number_lines: true,
        },
        StagePlanV1::TailLines {
            count: 2,
            path: None,
        },
    ]))
    .expect("execute numbered tail pipeline");

    assert_eq!(outcome.exit_code, 0);
    assert_eq!(outcome.stdout, b"     2\ttwo\r\n     3\tthree\r\n");
    assert!(outcome.stderr.is_empty());
    cleanup(&sandbox);
}

#[test]
fn tail_then_wc_counts_only_retained_terminated_records() {
    let sandbox = sandbox();
    let input = sandbox.join("input.txt");
    fs::write(&input, b"one\nsecond\nlast").unwrap();

    let outcome = execute_prepared(request(vec![
        StagePlanV1::TailLines {
            count: 2,
            path: Some(path_spec(&input)),
        },
        StagePlanV1::CountLines { path: None },
    ]))
    .expect("execute tail/wc pipeline");

    assert_eq!(outcome.exit_code, 0);
    assert_eq!(outcome.stdout, b"1\r\n");
    assert!(outcome.stderr.is_empty());
    cleanup(&sandbox);
}

#[test]
fn finite_tail_fails_closed_when_its_record_buffer_limit_is_exceeded() {
    let sandbox = sandbox();
    let input = sandbox.join("input.txt");
    fs::write(&input, vec![b'\n'; 65_537]).unwrap();

    let outcome = execute_prepared(request(vec![StagePlanV1::TailLines {
        count: 65_537,
        path: Some(path_spec(&input)),
    }]))
    .expect("return bounded tail outcome");

    assert_eq!(outcome.exit_code, 1);
    assert!(outcome.stdout.is_empty());
    assert_eq!(
        String::from_utf8(outcome.stderr).unwrap(),
        "wingman tail: buffer resource limit exceeded\r\n"
    );
    cleanup(&sandbox);
}

#[test]
fn grep_file_selects_records_and_preserves_final_termination() {
    let sandbox = sandbox();
    let input = sandbox.join("input.txt");
    fs::write(&input, "skip\nTODO 한글\nlast TODO").unwrap();

    let outcome = execute_prepared(request(vec![grep_stage(
        "TODO",
        vec![path_spec(&input)],
        false,
        false,
        false,
        false,
    )]))
    .expect("execute file grep");

    assert_eq!(outcome.exit_code, 0);
    assert_eq!(outcome.stdout, "TODO 한글\r\nlast TODO".as_bytes());
    assert!(outcome.stderr.is_empty());
    cleanup(&sandbox);
}

#[test]
fn grep_no_match_is_a_result_status_only_when_grep_is_final() {
    let sandbox = sandbox();
    let input = sandbox.join("input.txt");
    fs::write(&input, b"one\ntwo\n").unwrap();
    let stage = grep_stage(
        "NOTHING",
        vec![path_spec(&input)],
        false,
        false,
        false,
        false,
    );

    let final_grep = execute_prepared(request(vec![stage.clone()])).unwrap();
    assert_eq!(final_grep.exit_code, 1);
    assert!(final_grep.stdout.is_empty());
    assert!(final_grep.stderr.is_empty());

    let downstream_head = execute_prepared(request(vec![
        stage,
        StagePlanV1::HeadLines {
            count: 5,
            path: None,
        },
    ]))
    .unwrap();
    assert_eq!(downstream_head.exit_code, 0);
    assert!(downstream_head.stdout.is_empty());
    cleanup(&sandbox);
}

#[test]
fn grep_invert_case_folding_and_line_numbers_are_applied_before_the_sink() {
    let sandbox = sandbox();
    let input = sandbox.join("input.txt");
    fs::write(&input, "TODO\ntodo\nkeep\n").unwrap();

    let outcome = execute_prepared(request(vec![grep_stage(
        "todo",
        vec![path_spec(&input)],
        true,
        true,
        true,
        false,
    )]))
    .unwrap();

    assert_eq!(outcome.exit_code, 0);
    assert_eq!(outcome.stdout, b"3:keep\r\n");
    assert!(outcome.stderr.is_empty());
    cleanup(&sandbox);
}

#[test]
fn cat_grep_head_stops_before_an_invalid_unrequested_suffix() {
    let sandbox = sandbox();
    let input = sandbox.join("input.txt");
    fs::write(&input, [b"TODO first\n".as_slice(), &[0xff, 0xfe]].concat()).unwrap();

    let outcome = execute_prepared(request(vec![
        StagePlanV1::ReadTextFiles {
            paths: vec![path_spec(&input)],
            number_lines: false,
        },
        grep_stage("TODO", vec![], false, false, false, false),
        StagePlanV1::HeadLines {
            count: 1,
            path: None,
        },
    ]))
    .unwrap();

    assert_eq!(outcome.exit_code, 0);
    assert_eq!(outcome.stdout, b"TODO first\r\n");
    assert!(outcome.stderr.is_empty());
    cleanup(&sandbox);
}

#[test]
fn grep_output_can_feed_wc_and_safe_redirection() {
    let sandbox = sandbox();
    let input = sandbox.join("input.txt");
    let output = sandbox.join("count.txt");
    fs::write(&input, b"TODO one\nskip\nTODO two\n").unwrap();

    let outcome = execute_prepared(request_with_redirect(
        vec![
            grep_stage("TODO", vec![path_spec(&input)], false, false, false, true),
            StagePlanV1::CountLines { path: None },
        ],
        &output,
        RedirectModeV1::Overwrite,
    ))
    .unwrap();

    assert_eq!(outcome.exit_code, 0);
    assert!(outcome.stdout.is_empty());
    assert!(outcome.stderr.is_empty());
    assert_eq!(fs::read(&output).unwrap(), b"2\r\n");
    cleanup(&sandbox);
}

#[test]
fn multi_file_grep_prefixes_paths_resets_numbers_and_promotes_nonfinal_output() {
    let sandbox = sandbox();
    let first = sandbox.join("first.txt");
    let second = sandbox.join("second.txt");
    fs::write(&first, b"skip\nTODO last").unwrap();
    fs::write(&second, b"TODO first\nnope\n").unwrap();

    let outcome = execute_prepared(request(vec![grep_stage(
        "TODO",
        vec![path_spec(&first), path_spec(&second)],
        false,
        true,
        false,
        false,
    )]))
    .unwrap();

    let first_display = first.display().to_string().replace('/', "\\");
    let second_display = second.display().to_string().replace('/', "\\");
    assert_eq!(outcome.exit_code, 0);
    assert_eq!(
        String::from_utf8(outcome.stdout).unwrap(),
        format!("{first_display}:2:TODO last\r\n{second_display}:1:TODO first\r\n")
    );
    assert!(outcome.stderr.is_empty());
    cleanup(&sandbox);
}

#[test]
fn head_after_multi_file_grep_stops_after_promoting_the_first_unterminated_match() {
    let sandbox = sandbox();
    let first = sandbox.join("first.txt");
    let second = sandbox.join("second.txt");
    fs::write(&first, b"TODO first").unwrap();
    fs::write(&second, b"TODO second").unwrap();

    let outcome = execute_prepared(request(vec![
        grep_stage(
            "TODO",
            vec![path_spec(&first), path_spec(&second)],
            false,
            false,
            false,
            false,
        ),
        StagePlanV1::HeadLines {
            count: 1,
            path: None,
        },
    ]))
    .unwrap();

    let first_display = first.display().to_string().replace('/', "\\");
    assert_eq!(outcome.exit_code, 0);
    assert_eq!(
        String::from_utf8(outcome.stdout).unwrap(),
        format!("{first_display}:TODO first\r\n")
    );
    assert!(outcome.stderr.is_empty());
    cleanup(&sandbox);
}

fn grep_stage(
    pattern: &str,
    paths: Vec<wingman_lib::windows_path::ValidatedPathSpecV1>,
    ignore_case: bool,
    line_numbers: bool,
    invert_match: bool,
    fixed_strings: bool,
) -> StagePlanV1 {
    StagePlanV1::SearchText {
        pattern: pattern.to_string(),
        paths,
        ignore_case,
        line_numbers,
        invert_match,
        fixed_strings,
        recursive: false,
    }
}

fn request(stages: Vec<StagePlanV1>) -> PreparedRequestV1 {
    PreparedRequestV1 {
        protocol: "wingman.run".to_string(),
        version: 1,
        kind: PreparedRequestKindV1::Execute {
            plan: ExecutionPlanV1 {
                stages,
                redirect: None,
            },
        },
    }
}

fn request_with_redirect(
    stages: Vec<StagePlanV1>,
    path: &Path,
    mode: RedirectModeV1,
) -> PreparedRequestV1 {
    PreparedRequestV1 {
        protocol: "wingman.run".to_string(),
        version: 1,
        kind: PreparedRequestKindV1::Execute {
            plan: ExecutionPlanV1 {
                stages,
                redirect: Some(ValidatedRedirectPlanV1 {
                    mode,
                    path: path_spec(path),
                }),
            },
        },
    }
}

fn path_spec(path: &Path) -> wingman_lib::windows_path::ValidatedPathSpecV1 {
    validate_path_value(&path.to_string_lossy()).unwrap()
}

struct FailingWriter {
    remaining: usize,
}

struct CancellingWriter {
    bytes: Vec<u8>,
    cancellation: RunnerCancellationV1,
}

impl Write for CancellingWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.bytes.extend_from_slice(buffer);
        self.cancellation.cancel();
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct CancellingFailureWriter {
    cancellation: RunnerCancellationV1,
}

impl Write for CancellingFailureWriter {
    fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
        self.cancellation.cancel();
        Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "test sink cancelled and closed",
        ))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Write for FailingWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "test sink closed",
            ));
        }
        let length = self.remaining.min(buffer.len());
        self.remaining -= length;
        Ok(length)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn sandbox() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "wingman-readonly-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    ));
    fs::create_dir(&path).unwrap();
    path
}

fn cleanup(path: &Path) {
    let _ = fs::remove_dir_all(path);
}
