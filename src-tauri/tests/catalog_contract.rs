use wingman_lib::catalog::{build_execution_plan, CatalogErrorV1};
use wingman_lib::interpreter::{
    validate_execution_plan, ExecutionPlanV1, RedirectModeV1, StagePlanV1, ValidatedRedirectPlanV1,
    MAX_PATH_OPERANDS, MAX_PIPELINE_STAGES,
};
use wingman_lib::lexer::lex_p0_line;
use wingman_lib::parser::parse_p0_tokens;
use wingman_lib::windows_path::validate_path_value;

#[test]
fn mkdir_builds_one_bounded_non_composable_mutation_plan() {
    let parsed = parse_p0_tokens(&lex_p0_line("mkdir -p one two\\three").unwrap()).unwrap();
    assert_eq!(
        build_execution_plan(&parsed).unwrap(),
        ExecutionPlanV1 {
            stages: vec![StagePlanV1::CreateDirectories {
                paths: vec![
                    validate_path_value("one").unwrap(),
                    validate_path_value("two\\three").unwrap(),
                ],
                parents: true,
            }],
            redirect: None,
        }
    );

    let dash_path = parse_p0_tokens(&lex_p0_line("mkdir -- -cache").unwrap()).unwrap();
    assert_eq!(
        build_execution_plan(&dash_path).unwrap().stages,
        vec![StagePlanV1::CreateDirectories {
            paths: vec![validate_path_value("-cache").unwrap()],
            parents: false,
        }]
    );
}

#[test]
fn mkdir_rejects_unsupported_or_composed_shapes_before_execution() {
    for line in [
        "mkdir",
        "mkdir -m 755 output",
        "mkdir *.cache",
        "mkdir output | wc -l",
        "mkdir output > result.txt",
    ] {
        let parsed = parse_p0_tokens(&lex_p0_line(line).unwrap()).unwrap();
        assert!(build_execution_plan(&parsed).is_err(), "line: {line}");
    }
}

#[test]
fn runner_revalidates_the_bounded_mkdir_wire_shape() {
    let valid = ExecutionPlanV1 {
        stages: vec![StagePlanV1::CreateDirectories {
            paths: vec![validate_path_value("one").unwrap()],
            parents: false,
        }],
        redirect: None,
    };
    assert_eq!(validate_execution_plan(&valid), Ok(()));

    let empty = ExecutionPlanV1 {
        stages: vec![StagePlanV1::CreateDirectories {
            paths: Vec::new(),
            parents: false,
        }],
        redirect: None,
    };
    assert!(validate_execution_plan(&empty).is_err());

    let redirected = ExecutionPlanV1 {
        stages: valid.stages,
        redirect: Some(ValidatedRedirectPlanV1 {
            mode: RedirectModeV1::Overwrite,
            path: validate_path_value("out.txt").unwrap(),
        }),
    };
    assert!(validate_execution_plan(&redirected).is_err());
}

#[test]
fn touch_builds_one_bounded_non_composable_mutation_plan() {
    let parsed = parse_p0_tokens(&lex_p0_line("touch one.txt two\\three.txt").unwrap()).unwrap();
    assert_eq!(
        build_execution_plan(&parsed).unwrap(),
        ExecutionPlanV1 {
            stages: vec![StagePlanV1::TouchFiles {
                paths: vec![
                    validate_path_value("one.txt").unwrap(),
                    validate_path_value("two\\three.txt").unwrap(),
                ],
            }],
            redirect: None,
        }
    );

    let dash_path = parse_p0_tokens(&lex_p0_line("touch -- -stamp").unwrap()).unwrap();
    assert_eq!(
        build_execution_plan(&dash_path).unwrap().stages,
        vec![StagePlanV1::TouchFiles {
            paths: vec![validate_path_value("-stamp").unwrap()],
        }]
    );
}

#[test]
fn touch_rejects_options_or_composed_shapes_before_execution() {
    for line in [
        "touch",
        "touch -m output.txt",
        "touch *.txt",
        "touch output.txt | wc -l",
        "touch output.txt > result.txt",
    ] {
        let parsed = parse_p0_tokens(&lex_p0_line(line).unwrap()).unwrap();
        assert!(build_execution_plan(&parsed).is_err(), "line: {line}");
    }
}

#[test]
fn runner_revalidates_the_bounded_touch_wire_shape() {
    let valid = ExecutionPlanV1 {
        stages: vec![StagePlanV1::TouchFiles {
            paths: vec![validate_path_value("one.txt").unwrap()],
        }],
        redirect: None,
    };
    assert_eq!(validate_execution_plan(&valid), Ok(()));

    let empty = ExecutionPlanV1 {
        stages: vec![StagePlanV1::TouchFiles { paths: Vec::new() }],
        redirect: None,
    };
    assert!(validate_execution_plan(&empty).is_err());

    let redirected = ExecutionPlanV1 {
        stages: valid.stages,
        redirect: Some(ValidatedRedirectPlanV1 {
            mode: RedirectModeV1::Overwrite,
            path: validate_path_value("out.txt").unwrap(),
        }),
    };
    assert!(validate_execution_plan(&redirected).is_err());
}

#[test]
fn cat_head_and_redirect_build_one_typed_shell_independent_plan() {
    let parsed =
        parse_p0_tokens(&lex_p0_line(r#"cat -n "app log.txt" | head -n 5 > out.txt"#).unwrap())
            .unwrap();
    let plan = build_execution_plan(&parsed).expect("build cat/head plan");

    assert_eq!(
        plan.stages,
        vec![
            StagePlanV1::ReadTextFiles {
                paths: vec![validate_path_value("app log.txt").unwrap()],
                number_lines: true,
            },
            StagePlanV1::HeadLines {
                count: 5,
                path: None,
            },
        ]
    );
    assert_eq!(
        plan.redirect,
        Some(ValidatedRedirectPlanV1 {
            mode: RedirectModeV1::Overwrite,
            path: validate_path_value("out.txt").unwrap(),
        })
    );
}

#[test]
fn head_file_and_option_terminator_are_validated_by_the_catalog() {
    let parsed = parse_p0_tokens(&lex_p0_line(r#"head -n 0 -- "-file.txt""#).unwrap()).unwrap();
    let plan = build_execution_plan(&parsed).expect("build file head plan");
    assert_eq!(
        plan.stages,
        vec![StagePlanV1::HeadLines {
            count: 0,
            path: Some(validate_path_value("-file.txt").unwrap()),
        }]
    );
}

#[test]
fn invalid_source_shapes_and_options_are_rejected_before_runner_execution() {
    for line in [
        "cat",
        "cat -A file.txt",
        "cat *.txt",
        "head -n nope file.txt",
        "head one.txt two.txt",
        "head file.txt | cat other.txt",
        "cat file.txt | head other.txt",
    ] {
        let parsed = parse_p0_tokens(&lex_p0_line(line).unwrap()).unwrap();
        assert!(build_execution_plan(&parsed).is_err(), "line: {line}");
    }
}

#[test]
fn host_catalog_applies_the_same_stage_and_path_limits_as_the_runner() {
    let pipeline = std::iter::once("head file.txt")
        .chain(std::iter::repeat_n("head", MAX_PIPELINE_STAGES))
        .collect::<Vec<_>>()
        .join(" | ");
    let parsed = parse_p0_tokens(&lex_p0_line(&pipeline).unwrap()).unwrap();
    assert_eq!(
        build_execution_plan(&parsed),
        Err(CatalogErrorV1::ResourceLimit)
    );

    let cat = format!(
        "cat {}",
        std::iter::repeat_n("file.txt", MAX_PATH_OPERANDS + 1)
            .collect::<Vec<_>>()
            .join(" ")
    );
    let parsed = parse_p0_tokens(&lex_p0_line(&cat).unwrap()).unwrap();
    assert_eq!(
        build_execution_plan(&parsed),
        Err(CatalogErrorV1::ResourceLimit)
    );
}

#[test]
fn wc_lines_accepts_exactly_one_file_or_one_pipeline_input() {
    let parsed = parse_p0_tokens(&lex_p0_line("wc -l input.txt").unwrap()).unwrap();
    let plan = build_execution_plan(&parsed).expect("build file wc plan");
    assert_eq!(
        plan.stages,
        vec![StagePlanV1::CountLines {
            path: Some(validate_path_value("input.txt").unwrap()),
        }]
    );

    let parsed =
        parse_p0_tokens(&lex_p0_line("cat input.txt | head -n 2 | wc --lines").unwrap()).unwrap();
    let plan = build_execution_plan(&parsed).expect("build pipeline wc plan");
    assert_eq!(
        plan.stages,
        vec![
            StagePlanV1::ReadTextFiles {
                paths: vec![validate_path_value("input.txt").unwrap()],
                number_lines: false,
            },
            StagePlanV1::HeadLines {
                count: 2,
                path: None,
            },
            StagePlanV1::CountLines { path: None },
        ]
    );

    for line in [
        "wc",
        "wc -w input.txt",
        "wc -l",
        "cat input.txt | wc -l other.txt",
    ] {
        let parsed = parse_p0_tokens(&lex_p0_line(line).unwrap()).unwrap();
        assert!(build_execution_plan(&parsed).is_err(), "line: {line}");
    }
}

#[test]
fn tail_accepts_finite_and_single_file_follow_sources() {
    let parsed = parse_p0_tokens(&lex_p0_line("tail input.txt").unwrap()).unwrap();
    let plan = build_execution_plan(&parsed).expect("build default tail plan");
    assert_eq!(
        plan.stages,
        vec![StagePlanV1::TailLines {
            count: 10,
            path: Some(validate_path_value("input.txt").unwrap()),
        }]
    );

    let parsed =
        parse_p0_tokens(&lex_p0_line("cat input.txt | head -n 5 | tail -n 2 | wc -l").unwrap())
            .unwrap();
    let plan = build_execution_plan(&parsed).expect("build pipeline tail plan");
    assert_eq!(
        plan.stages,
        vec![
            StagePlanV1::ReadTextFiles {
                paths: vec![validate_path_value("input.txt").unwrap()],
                number_lines: false,
            },
            StagePlanV1::HeadLines {
                count: 5,
                path: None,
            },
            StagePlanV1::TailLines {
                count: 2,
                path: None,
            },
            StagePlanV1::CountLines { path: None },
        ]
    );

    let parsed = parse_p0_tokens(
        &lex_p0_line("tail --follow -n 3 input.txt | grep ERROR | head -n 1").unwrap(),
    )
    .unwrap();
    let plan = build_execution_plan(&parsed).expect("build follow pipeline");
    assert!(matches!(
        &plan.stages[0],
        StagePlanV1::FollowFile { count: 3, path }
            if path == &validate_path_value("input.txt").unwrap()
    ));

    for line in [
        "tail -f",
        "tail -f one.txt two.txt",
        "cat input.txt | tail -f",
        "tail -n nope input.txt",
        "tail one.txt two.txt",
        "cat input.txt | tail other.txt",
    ] {
        let parsed = parse_p0_tokens(&lex_p0_line(line).unwrap()).unwrap();
        assert!(build_execution_plan(&parsed).is_err(), "line: {line}");
    }
}

#[test]
fn tail_output_can_feed_a_later_head() {
    let parsed = parse_p0_tokens(&lex_p0_line("tail -n 3 input.txt | head -n 2").unwrap()).unwrap();
    let plan = build_execution_plan(&parsed).expect("connect tail output to head input");

    assert!(matches!(plan.stages[0], StagePlanV1::TailLines { .. }));
    assert!(matches!(
        plan.stages[1],
        StagePlanV1::HeadLines {
            count: 2,
            path: None
        }
    ));
}

#[test]
fn tail_output_can_feed_a_second_tail() {
    let parsed = parse_p0_tokens(&lex_p0_line("tail -n 4 input.txt | tail -n 2").unwrap()).unwrap();
    let plan = build_execution_plan(&parsed).expect("connect tail output to a second tail input");

    assert_eq!(plan.stages.len(), 2);
    assert!(plan
        .stages
        .iter()
        .all(|stage| matches!(stage, StagePlanV1::TailLines { .. })));
}

#[test]
fn grep_builds_typed_file_and_pipeline_search_stages() {
    let parsed = parse_p0_tokens(&lex_p0_line("grep -inv TODO one.txt two.txt").unwrap()).unwrap();
    let plan = build_execution_plan(&parsed).expect("build multi-file grep plan");
    assert_eq!(
        plan.stages,
        vec![StagePlanV1::SearchText {
            pattern: "TODO".to_string(),
            paths: vec![
                validate_path_value("one.txt").unwrap(),
                validate_path_value("two.txt").unwrap(),
            ],
            ignore_case: true,
            line_numbers: true,
            invert_match: true,
            fixed_strings: false,
            recursive: false,
        }]
    );

    let parsed = parse_p0_tokens(
        &lex_p0_line(r#"cat app.log | grep --fixed-strings "[TODO]" | head -n 1"#).unwrap(),
    )
    .unwrap();
    let plan = build_execution_plan(&parsed).expect("build pipeline grep plan");
    assert_eq!(
        plan.stages,
        vec![
            StagePlanV1::ReadTextFiles {
                paths: vec![validate_path_value("app.log").unwrap()],
                number_lines: false,
            },
            StagePlanV1::SearchText {
                pattern: "[TODO]".to_string(),
                paths: vec![],
                ignore_case: false,
                line_numbers: false,
                invert_match: false,
                fixed_strings: true,
                recursive: false,
            },
            StagePlanV1::HeadLines {
                count: 1,
                path: None,
            },
        ]
    );
}

#[test]
fn grep_rejects_unsupported_options_patterns_and_source_shapes() {
    for line in [
        "grep TODO",
        "grep -E TODO app.log",
        "grep 'a|b' app.log",
        "grep TODO *.txt",
        "cat app.log | grep -r TODO",
        "cat app.log | grep TODO other.log",
    ] {
        let parsed = parse_p0_tokens(&lex_p0_line(line).unwrap()).unwrap();
        assert!(build_execution_plan(&parsed).is_err(), "line: {line}");
    }
}

#[test]
fn uniq_builds_typed_file_and_pipeline_stages() {
    let parsed = parse_p0_tokens(&lex_p0_line("uniq -cd input.txt").unwrap()).unwrap();
    let plan = build_execution_plan(&parsed).expect("build file uniq plan");
    assert_eq!(
        plan.stages,
        vec![StagePlanV1::UniqueLines {
            path: Some(validate_path_value("input.txt").unwrap()),
            count: true,
            repeated_only: true,
            unique_only: false,
        }]
    );

    let parsed = parse_p0_tokens(
        &lex_p0_line("cat input.txt | grep ERROR | uniq --unique | head -n 2 | wc -l").unwrap(),
    )
    .unwrap();
    let plan = build_execution_plan(&parsed).expect("build pipeline uniq plan");
    assert!(matches!(
        plan.stages[2],
        StagePlanV1::UniqueLines {
            path: None,
            count: false,
            repeated_only: false,
            unique_only: true,
        }
    ));
}

#[test]
fn uniq_output_can_feed_a_second_uniq() {
    let parsed = parse_p0_tokens(&lex_p0_line("uniq input.txt | uniq -c").unwrap()).unwrap();
    let plan = build_execution_plan(&parsed).expect("connect uniq output to a second uniq input");

    assert_eq!(plan.stages.len(), 2);
    assert!(plan
        .stages
        .iter()
        .all(|stage| matches!(stage, StagePlanV1::UniqueLines { .. })));
}

#[test]
fn head_output_can_feed_a_later_uniq() {
    let parsed = parse_p0_tokens(&lex_p0_line("head -n 3 input.txt | uniq -c").unwrap()).unwrap();
    let plan = build_execution_plan(&parsed).expect("connect head output to uniq input");

    assert!(matches!(plan.stages[0], StagePlanV1::HeadLines { .. }));
    assert!(matches!(
        plan.stages[1],
        StagePlanV1::UniqueLines {
            path: None,
            count: true,
            ..
        }
    ));
}

#[test]
fn tail_output_can_feed_a_later_uniq() {
    let parsed = parse_p0_tokens(&lex_p0_line("tail -n 4 input.txt | uniq -c").unwrap()).unwrap();
    let plan = build_execution_plan(&parsed).expect("connect tail output to uniq input");

    assert!(matches!(plan.stages[0], StagePlanV1::TailLines { .. }));
    assert!(matches!(
        plan.stages[1],
        StagePlanV1::UniqueLines {
            path: None,
            count: true,
            ..
        }
    ));
}

#[test]
fn uniq_rejects_conflicts_and_invalid_source_shapes() {
    for line in [
        "uniq",
        "uniq -du input.txt",
        "uniq one.txt two.txt",
        "uniq -z input.txt",
        "cat input.txt | uniq other.txt",
    ] {
        let parsed = parse_p0_tokens(&lex_p0_line(line).unwrap()).unwrap();
        assert!(build_execution_plan(&parsed).is_err(), "line: {line}");
    }
}

#[test]
fn sort_builds_typed_file_and_pipeline_stages() {
    let parsed = parse_p0_tokens(&lex_p0_line("sort -rnu numbers.txt").unwrap()).unwrap();
    let plan = build_execution_plan(&parsed).expect("build file sort plan");
    assert_eq!(
        plan.stages,
        vec![StagePlanV1::SortLines {
            path: Some(validate_path_value("numbers.txt").unwrap()),
            reverse: true,
            numeric: true,
            unique: true,
        }]
    );

    let parsed = parse_p0_tokens(
        &lex_p0_line("cat input.txt | grep value | sort --reverse | uniq | head -n 2").unwrap(),
    )
    .unwrap();
    let plan = build_execution_plan(&parsed).expect("build pipeline sort plan");
    assert!(matches!(
        plan.stages[2],
        StagePlanV1::SortLines {
            path: None,
            reverse: true,
            numeric: false,
            unique: false,
        }
    ));
}

#[test]
fn sort_output_can_feed_a_later_text_filter() {
    let parsed = parse_p0_tokens(&lex_p0_line("sort input.txt | grep value").unwrap()).unwrap();
    let plan = build_execution_plan(&parsed).expect("connect sort output to grep input");

    assert!(matches!(plan.stages[0], StagePlanV1::SortLines { .. }));
    assert!(matches!(
        plan.stages[1],
        StagePlanV1::SearchText {
            ref paths,
            recursive: false,
            ..
        } if paths.is_empty()
    ));
}

#[test]
fn sort_output_can_feed_a_second_sort() {
    let parsed = parse_p0_tokens(&lex_p0_line("sort -r input.txt | sort -n").unwrap()).unwrap();
    let plan = build_execution_plan(&parsed).expect("connect sort output to a second sort input");

    assert_eq!(plan.stages.len(), 2);
    assert!(plan
        .stages
        .iter()
        .all(|stage| matches!(stage, StagePlanV1::SortLines { .. })));
}

#[test]
fn grep_output_can_feed_a_second_text_filter() {
    let parsed =
        parse_p0_tokens(&lex_p0_line("grep alpha input.txt | grep beta").unwrap()).unwrap();
    let plan = build_execution_plan(&parsed).expect("connect grep output to a second grep input");

    assert_eq!(plan.stages.len(), 2);
    assert!(plan.stages.iter().all(|stage| matches!(
        stage,
        StagePlanV1::SearchText {
            recursive: false,
            ..
        }
    )));
}

#[test]
fn uniq_output_can_feed_a_later_text_filter() {
    let parsed = parse_p0_tokens(&lex_p0_line("uniq input.txt | grep beta").unwrap()).unwrap();
    let plan = build_execution_plan(&parsed).expect("connect uniq output to grep input");

    assert!(matches!(plan.stages[0], StagePlanV1::UniqueLines { .. }));
    assert!(matches!(
        plan.stages[1],
        StagePlanV1::SearchText {
            ref paths,
            recursive: false,
            ..
        } if paths.is_empty()
    ));
}

#[test]
fn head_output_can_feed_a_later_text_filter() {
    let parsed = parse_p0_tokens(&lex_p0_line("head -n 2 input.txt | grep keep").unwrap()).unwrap();
    let plan = build_execution_plan(&parsed).expect("connect head output to grep input");

    assert!(matches!(plan.stages[0], StagePlanV1::HeadLines { .. }));
    assert!(matches!(
        plan.stages[1],
        StagePlanV1::SearchText {
            ref paths,
            recursive: false,
            ..
        } if paths.is_empty()
    ));
}

#[test]
fn uniq_output_can_feed_a_later_sort() {
    let parsed = parse_p0_tokens(&lex_p0_line("uniq input.txt | sort").unwrap()).unwrap();
    let plan = build_execution_plan(&parsed).expect("connect uniq output to sort input");

    assert!(matches!(plan.stages[0], StagePlanV1::UniqueLines { .. }));
    assert!(matches!(
        plan.stages[1],
        StagePlanV1::SortLines { path: None, .. }
    ));
}

#[test]
fn head_output_can_feed_a_later_sort() {
    let parsed = parse_p0_tokens(&lex_p0_line("head -n 2 input.txt | sort").unwrap()).unwrap();
    let plan = build_execution_plan(&parsed).expect("connect head output to sort input");

    assert!(matches!(plan.stages[0], StagePlanV1::HeadLines { .. }));
    assert!(matches!(
        plan.stages[1],
        StagePlanV1::SortLines { path: None, .. }
    ));
}

#[test]
fn tail_output_can_feed_a_later_sort() {
    let parsed = parse_p0_tokens(&lex_p0_line("tail -n 3 input.txt | sort").unwrap()).unwrap();
    let plan = build_execution_plan(&parsed).expect("connect tail output to sort input");

    assert!(matches!(plan.stages[0], StagePlanV1::TailLines { .. }));
    assert!(matches!(
        plan.stages[1],
        StagePlanV1::SortLines { path: None, .. }
    ));
}

#[test]
fn sort_rejects_unsupported_options_and_invalid_source_shapes() {
    for line in [
        "sort",
        "sort one.txt two.txt",
        "sort -f input.txt",
        "cat input.txt | sort other.txt",
    ] {
        let parsed = parse_p0_tokens(&lex_p0_line(line).unwrap()).unwrap();
        assert!(build_execution_plan(&parsed).is_err(), "line: {line}");
    }
}

#[test]
fn recursive_grep_can_feed_the_common_ordered_stages() {
    let parsed = parse_p0_tokens(
        &lex_p0_line("grep -r value src | grep keep | sort -r | uniq -c | head -n 2").unwrap(),
    )
    .unwrap();
    let plan = build_execution_plan(&parsed).expect("build recursive ordered pipeline");
    assert_eq!(plan.stages.len(), 5);
    assert!(matches!(
        plan.stages[0],
        StagePlanV1::SearchText {
            recursive: true,
            ..
        }
    ));
    assert!(matches!(
        plan.stages[1],
        StagePlanV1::SearchText {
            recursive: false,
            ..
        }
    ));
    assert!(matches!(plan.stages[2], StagePlanV1::SortLines { .. }));
    assert!(matches!(plan.stages[3], StagePlanV1::UniqueLines { .. }));
    assert!(matches!(plan.stages[4], StagePlanV1::HeadLines { .. }));
}
