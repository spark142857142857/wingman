use wingman_lib::interpreter::{
    ActiveShell, ExecutionPlanV1, FrontendDecisionKindV1, FrontendDecisionV1, InterpreterSession,
    LineEvidence, PrepareSubmissionErrorV1, PrepareSubmissionV1, PreparedRequestKindV1,
    PreparedRequestV1, RedirectModeV1, StagePlanV1, ValidatedRedirectPlanV1,
};
use wingman_lib::windows_path::validate_path_value;

#[test]
fn reliable_familiar_mkdir_is_stored_as_a_typed_mutation_plan() {
    let mut session = InterpreterSession::new(41, 6, ActiveShell::WindowsPowerShell);
    let decision = session
        .prepare_submission(PrepareSubmissionV1 {
            session_id: 41,
            command_sequence: 6,
            shell: ActiveShell::WindowsPowerShell,
            familiar_enabled: true,
            evidence: LineEvidence::Reliable,
            raw_line: "mkdir -p one two\\three".to_string(),
        })
        .expect("classify mkdir");
    let request_id = match decision.decision {
        FrontendDecisionKindV1::InvokePrepared { request_id, .. } => request_id,
        other => panic!("expected a prepared mkdir plan, got {other:?}"),
    };
    assert_eq!(
        session.consume_prepared(&request_id),
        Some(PreparedRequestV1 {
            protocol: "wingman.run".to_string(),
            version: 1,
            kind: PreparedRequestKindV1::Execute {
                plan: ExecutionPlanV1 {
                    stages: vec![StagePlanV1::CreateDirectories {
                        paths: vec![
                            validate_path_value("one").unwrap(),
                            validate_path_value("two\\three").unwrap(),
                        ],
                        parents: true,
                    }],
                    redirect: None,
                },
            },
        })
    );
}

#[test]
fn claimed_invalid_mkdir_is_rejected_without_native_fallback() {
    let mut session = InterpreterSession::new(41, 5, ActiveShell::WindowsPowerShell);
    let decision = session
        .prepare_submission(PrepareSubmissionV1 {
            session_id: 41,
            command_sequence: 5,
            shell: ActiveShell::WindowsPowerShell,
            familiar_enabled: true,
            evidence: LineEvidence::Reliable,
            raw_line: "mkdir -m 755 output".to_string(),
        })
        .expect("classify invalid mkdir");
    let request_id = match decision.decision {
        FrontendDecisionKindV1::InvokePrepared { request_id, .. } => request_id,
        other => panic!("expected a prepared mkdir rejection, got {other:?}"),
    };
    assert_eq!(
        session.consume_prepared(&request_id).unwrap().kind,
        PreparedRequestKindV1::Reject {
            diagnostic: "wingman mkdir: unsupported option".to_string(),
            exit_code: 2,
        }
    );
}

#[test]
fn native_command_is_returned_as_authoritative_pass_through_line() {
    let request = PrepareSubmissionV1 {
        session_id: 41,
        command_sequence: 7,
        shell: ActiveShell::WindowsPowerShell,
        familiar_enabled: true,
        evidence: LineEvidence::Reliable,
        raw_line: "git status".to_string(),
    };

    let mut session = InterpreterSession::new(41, 7, ActiveShell::WindowsPowerShell);
    assert_eq!(
        session
            .prepare_submission(request)
            .expect("current session"),
        FrontendDecisionV1 {
            session_id: 41,
            command_sequence: 7,
            decision: FrontendDecisionKindV1::PassThrough {
                raw_line: "git status".to_string(),
            },
        }
    );
}

#[test]
fn uncertain_p0_looking_line_is_passed_through_unchanged() {
    let request = PrepareSubmissionV1 {
        session_id: 41,
        command_sequence: 8,
        shell: ActiveShell::Cmd,
        familiar_enabled: true,
        evidence: LineEvidence::Uncertain,
        raw_line: "grep TODO app.log".to_string(),
    };

    let mut session = InterpreterSession::new(41, 8, ActiveShell::Cmd);
    assert_eq!(
        session
            .prepare_submission(request)
            .expect("current session"),
        FrontendDecisionV1 {
            session_id: 41,
            command_sequence: 8,
            decision: FrontendDecisionKindV1::PassThrough {
                raw_line: "grep TODO app.log".to_string(),
            },
        }
    );
}

#[test]
fn unsupported_owned_command_uses_one_shot_prepared_rejection() {
    let mut session = InterpreterSession::new(41, 9, ActiveShell::WindowsPowerShell);
    let raw_line = "grep -z TODO app.log";

    let decision = session
        .prepare_submission(PrepareSubmissionV1 {
            session_id: 41,
            command_sequence: 9,
            shell: ActiveShell::WindowsPowerShell,
            familiar_enabled: true,
            evidence: LineEvidence::Reliable,
            raw_line: raw_line.to_string(),
        })
        .expect("current session");

    let request_id = match decision {
        FrontendDecisionV1 {
            session_id: 41,
            command_sequence: 9,
            decision:
                FrontendDecisionKindV1::InvokePrepared {
                    request_id,
                    display_line,
                },
        } => {
            assert_eq!(display_line, raw_line);
            assert!(request_id.len() >= 32);
            assert!(!request_id.contains("grep"));
            request_id
        }
        other => panic!("expected prepared rejection, got {other:?}"),
    };

    assert_eq!(
        session.consume_prepared(&request_id),
        Some(PreparedRequestV1 {
            protocol: "wingman.run".to_string(),
            version: 1,
            kind: PreparedRequestKindV1::Reject {
                diagnostic: "wingman grep: unsupported option".to_string(),
                exit_code: 2,
            },
        })
    );
    assert_eq!(session.consume_prepared(&request_id), None);
}

#[test]
fn mismatched_session_is_rejected_before_classification() {
    let mut session = InterpreterSession::new(41, 10, ActiveShell::Cmd);

    assert_eq!(
        session.prepare_submission(PrepareSubmissionV1 {
            session_id: 40,
            command_sequence: 10,
            shell: ActiveShell::Cmd,
            familiar_enabled: true,
            evidence: LineEvidence::Reliable,
            raw_line: "grep -z TODO app.log".to_string(),
        }),
        Err(PrepareSubmissionErrorV1::SessionMismatch {
            expected: 41,
            received: 40,
        })
    );
}

#[test]
fn stale_command_sequence_is_rejected_before_classification() {
    let mut session = InterpreterSession::new(41, 11, ActiveShell::WindowsPowerShell);

    assert_eq!(
        session.prepare_submission(PrepareSubmissionV1 {
            session_id: 41,
            command_sequence: 10,
            shell: ActiveShell::WindowsPowerShell,
            familiar_enabled: true,
            evidence: LineEvidence::Reliable,
            raw_line: "grep -z TODO app.log".to_string(),
        }),
        Err(PrepareSubmissionErrorV1::CommandSequenceMismatch {
            expected: 11,
            received: 10,
        })
    );
}

#[test]
fn mismatched_active_shell_is_rejected_before_classification() {
    let mut session = InterpreterSession::new(41, 12, ActiveShell::Cmd);

    assert_eq!(
        session.prepare_submission(PrepareSubmissionV1 {
            session_id: 41,
            command_sequence: 12,
            shell: ActiveShell::WindowsPowerShell,
            familiar_enabled: true,
            evidence: LineEvidence::Reliable,
            raw_line: "grep -z TODO app.log".to_string(),
        }),
        Err(PrepareSubmissionErrorV1::ShellMismatch {
            expected: ActiveShell::Cmd,
            received: ActiveShell::WindowsPowerShell,
        })
    );
}

#[test]
fn valid_pwd_is_stored_as_a_typed_execution_plan() {
    let mut session = InterpreterSession::new(41, 13, ActiveShell::WindowsPowerShell);

    let decision = session
        .prepare_submission(PrepareSubmissionV1 {
            session_id: 41,
            command_sequence: 13,
            shell: ActiveShell::WindowsPowerShell,
            familiar_enabled: true,
            evidence: LineEvidence::Reliable,
            raw_line: "pwd".to_string(),
        })
        .expect("current reliable prompt");

    let request_id = match decision.decision {
        FrontendDecisionKindV1::InvokePrepared {
            request_id,
            display_line,
        } => {
            assert_eq!(display_line, "pwd");
            request_id
        }
        other => panic!("expected prepared execution, got {other:?}"),
    };

    assert_eq!(
        session.consume_prepared(&request_id),
        Some(PreparedRequestV1 {
            protocol: "wingman.run".to_string(),
            version: 1,
            kind: PreparedRequestKindV1::Execute {
                plan: ExecutionPlanV1 {
                    stages: vec![StagePlanV1::PrintWorkingDirectory],
                    redirect: None,
                },
            },
        })
    );
}

#[test]
fn one_prompt_sequence_cannot_be_prepared_twice() {
    let mut session = InterpreterSession::new(41, 14, ActiveShell::Cmd);
    let request = PrepareSubmissionV1 {
        session_id: 41,
        command_sequence: 14,
        shell: ActiveShell::Cmd,
        familiar_enabled: true,
        evidence: LineEvidence::Reliable,
        raw_line: "pwd".to_string(),
    };

    session
        .prepare_submission(request.clone())
        .expect("first preparation");
    assert_eq!(
        session.prepare_submission(request),
        Err(PrepareSubmissionErrorV1::AlreadyPrepared {
            command_sequence: 14,
        })
    );
}

#[test]
fn next_prompt_invalidates_old_requests_and_reopens_preparation() {
    let mut session = InterpreterSession::new(41, 15, ActiveShell::Cmd);
    let first = session
        .prepare_submission(PrepareSubmissionV1 {
            session_id: 41,
            command_sequence: 15,
            shell: ActiveShell::Cmd,
            familiar_enabled: true,
            evidence: LineEvidence::Reliable,
            raw_line: "pwd".to_string(),
        })
        .expect("first prompt");
    let old_request_id = match first.decision {
        FrontendDecisionKindV1::InvokePrepared { request_id, .. } => request_id,
        other => panic!("expected prepared execution, got {other:?}"),
    };

    assert!(session.synchronize_prompt(16, ActiveShell::Cmd));
    assert_eq!(session.consume_prepared(&old_request_id), None);
    assert!(session
        .prepare_submission(PrepareSubmissionV1 {
            session_id: 41,
            command_sequence: 16,
            shell: ActiveShell::Cmd,
            familiar_enabled: true,
            evidence: LineEvidence::Reliable,
            raw_line: "git status".to_string(),
        })
        .is_ok());
}

#[test]
fn familiar_control_is_prepared_even_when_familiar_mode_is_off() {
    let mut session = InterpreterSession::new(41, 17, ActiveShell::WindowsPowerShell);
    let decision = session
        .prepare_submission(PrepareSubmissionV1 {
            session_id: 41,
            command_sequence: 17,
            shell: ActiveShell::WindowsPowerShell,
            familiar_enabled: false,
            evidence: LineEvidence::Reliable,
            raw_line: "  fam ON  ".to_string(),
        })
        .expect("current reliable prompt");
    let request_id = match decision.decision {
        FrontendDecisionKindV1::InvokePrepared {
            request_id,
            display_line,
        } => {
            assert_eq!(display_line, "  fam ON  ");
            request_id
        }
        other => panic!("expected prepared control, got {other:?}"),
    };

    assert_eq!(
        session.consume_prepared(&request_id),
        Some(PreparedRequestV1 {
            protocol: "wingman.run".to_string(),
            version: 1,
            kind: PreparedRequestKindV1::Control {
                response: "Familiar: ON".to_string(),
                exit_code: 0,
            },
        })
    );
}

#[test]
fn reliable_familiar_cat_head_redirection_is_stored_as_one_typed_plan() {
    let mut session = InterpreterSession::new(41, 18, ActiveShell::WindowsPowerShell);
    let raw_line = "cat -n input.txt | head -n 3 > output.txt";
    let decision = session
        .prepare_submission(PrepareSubmissionV1 {
            session_id: 41,
            command_sequence: 18,
            shell: ActiveShell::WindowsPowerShell,
            familiar_enabled: true,
            evidence: LineEvidence::Reliable,
            raw_line: raw_line.to_string(),
        })
        .expect("classify a reliable read-only pipeline");
    let request_id = match decision.decision {
        FrontendDecisionKindV1::InvokePrepared {
            request_id,
            display_line,
        } => {
            assert_eq!(display_line, raw_line);
            request_id
        }
        other => panic!("expected a prepared read-only plan, got {other:?}"),
    };

    assert_eq!(
        session.consume_prepared(&request_id),
        Some(PreparedRequestV1 {
            protocol: "wingman.run".to_string(),
            version: 1,
            kind: PreparedRequestKindV1::Execute {
                plan: ExecutionPlanV1 {
                    stages: vec![
                        StagePlanV1::ReadTextFiles {
                            paths: vec![validate_path_value("input.txt").unwrap()],
                            number_lines: true,
                        },
                        StagePlanV1::HeadLines {
                            count: 3,
                            path: None,
                        },
                    ],
                    redirect: Some(ValidatedRedirectPlanV1 {
                        mode: RedirectModeV1::Overwrite,
                        path: validate_path_value("output.txt").unwrap(),
                    }),
                },
            },
        })
    );
}

#[test]
fn reliable_familiar_uniq_is_published_as_a_typed_plan() {
    let mut session = InterpreterSession::new(41, 34, ActiveShell::WindowsPowerShell);
    let raw_line = "cat input.txt | uniq -c | head -n 2";
    let decision = session
        .prepare_submission(PrepareSubmissionV1 {
            session_id: 41,
            command_sequence: 34,
            shell: ActiveShell::WindowsPowerShell,
            familiar_enabled: true,
            evidence: LineEvidence::Reliable,
            raw_line: raw_line.to_string(),
        })
        .expect("classify a reliable uniq pipeline");
    let request_id = match decision.decision {
        FrontendDecisionKindV1::InvokePrepared { request_id, .. } => request_id,
        other => panic!("expected a prepared uniq plan, got {other:?}"),
    };

    let request = session.consume_prepared(&request_id).unwrap();
    let PreparedRequestKindV1::Execute { plan } = request.kind else {
        panic!("expected executable uniq plan");
    };
    assert!(matches!(
        plan.stages[1],
        StagePlanV1::UniqueLines {
            path: None,
            count: true,
            repeated_only: false,
            unique_only: false,
        }
    ));
}

#[test]
fn reliable_familiar_sort_is_published_as_a_typed_plan() {
    let mut session = InterpreterSession::new(41, 37, ActiveShell::WindowsPowerShell);
    let raw_line = "cat input.txt | sort -nu | uniq -c";
    let decision = session
        .prepare_submission(PrepareSubmissionV1 {
            session_id: 41,
            command_sequence: 37,
            shell: ActiveShell::WindowsPowerShell,
            familiar_enabled: true,
            evidence: LineEvidence::Reliable,
            raw_line: raw_line.to_string(),
        })
        .expect("classify a reliable sort pipeline");
    let request_id = match decision.decision {
        FrontendDecisionKindV1::InvokePrepared { request_id, .. } => request_id,
        other => panic!("expected a prepared sort plan, got {other:?}"),
    };

    let request = session.consume_prepared(&request_id).unwrap();
    let PreparedRequestKindV1::Execute { plan } = request.kind else {
        panic!("expected executable sort plan");
    };
    assert!(matches!(
        plan.stages[1],
        StagePlanV1::SortLines {
            path: None,
            reverse: false,
            numeric: true,
            unique: true,
        }
    ));
}

#[test]
fn read_only_names_remain_native_when_ownership_evidence_is_missing() {
    for (sequence, familiar_enabled, evidence, raw_line) in [
        (19, false, LineEvidence::Reliable, "cat input.txt"),
        (20, true, LineEvidence::Uncertain, "head -n 1 input.txt"),
        (21, true, LineEvidence::Reliable, "cat.exe input.txt"),
        (22, true, LineEvidence::Reliable, "git log | head -n 1"),
        (26, true, LineEvidence::Reliable, "wc.exe -l input.txt"),
        (30, true, LineEvidence::Reliable, "tail.exe -n 2 input.txt"),
        (33, true, LineEvidence::Reliable, "grep.exe TODO input.txt"),
        (35, true, LineEvidence::Reliable, "uniq.exe input.txt"),
        (38, true, LineEvidence::Reliable, "sort.exe input.txt"),
    ] {
        let mut session = InterpreterSession::new(41, sequence, ActiveShell::WindowsPowerShell);
        let decision = session
            .prepare_submission(PrepareSubmissionV1 {
                session_id: 41,
                command_sequence: sequence,
                shell: ActiveShell::WindowsPowerShell,
                familiar_enabled,
                evidence,
                raw_line: raw_line.to_string(),
            })
            .expect("preserve native ownership");
        assert_eq!(
            decision.decision,
            FrontendDecisionKindV1::PassThrough {
                raw_line: raw_line.to_string(),
            }
        );
    }
}

#[test]
fn claimed_read_only_syntax_and_catalog_failures_become_prepared_rejections() {
    for (sequence, raw_line, diagnostic) in [
        (
            23,
            "cat input.txt && dir",
            "wingman cat: unsupported shell operator",
        ),
        (
            24,
            "cat input.txt | powershell Get-Date",
            "wingman cat: pipeline contains an unsupported command",
        ),
        (
            32,
            "grep -E TODO input.txt",
            "wingman grep: unsupported option",
        ),
        (25, "cat -z input.txt", "wingman cat: unsupported option"),
        (27, "wc input.txt", "wingman wc: unsupported option"),
        (36, "uniq -du input.txt", "wingman uniq: unsupported option"),
        (39, "sort -f input.txt", "wingman sort: unsupported option"),
    ] {
        let mut session = InterpreterSession::new(41, sequence, ActiveShell::WindowsPowerShell);
        let decision = session
            .prepare_submission(PrepareSubmissionV1 {
                session_id: 41,
                command_sequence: sequence,
                shell: ActiveShell::WindowsPowerShell,
                familiar_enabled: true,
                evidence: LineEvidence::Reliable,
                raw_line: raw_line.to_string(),
            })
            .expect("classify a claimed invalid line");
        let request_id = match decision.decision {
            FrontendDecisionKindV1::InvokePrepared {
                request_id,
                display_line,
            } => {
                assert_eq!(display_line, raw_line);
                request_id
            }
            other => panic!("expected a prepared rejection, got {other:?}"),
        };
        assert_eq!(
            session.consume_prepared(&request_id),
            Some(PreparedRequestV1 {
                protocol: "wingman.run".to_string(),
                version: 1,
                kind: PreparedRequestKindV1::Reject {
                    diagnostic: diagnostic.to_string(),
                    exit_code: 2,
                },
            })
        );
    }
}

#[test]
fn reliable_familiar_wc_lines_is_stored_as_a_typed_count_stage() {
    let mut session = InterpreterSession::new(41, 28, ActiveShell::WindowsPowerShell);
    let decision = session
        .prepare_submission(PrepareSubmissionV1 {
            session_id: 41,
            command_sequence: 28,
            shell: ActiveShell::WindowsPowerShell,
            familiar_enabled: true,
            evidence: LineEvidence::Reliable,
            raw_line: "wc -l input.txt".to_string(),
        })
        .expect("classify wc lines");
    let request_id = match decision.decision {
        FrontendDecisionKindV1::InvokePrepared { request_id, .. } => request_id,
        other => panic!("expected a prepared wc plan, got {other:?}"),
    };
    assert_eq!(
        session.consume_prepared(&request_id),
        Some(PreparedRequestV1 {
            protocol: "wingman.run".to_string(),
            version: 1,
            kind: PreparedRequestKindV1::Execute {
                plan: ExecutionPlanV1 {
                    stages: vec![StagePlanV1::CountLines {
                        path: Some(validate_path_value("input.txt").unwrap()),
                    }],
                    redirect: None,
                },
            },
        })
    );
}

#[test]
fn reliable_familiar_tail_is_stored_as_a_typed_finite_stage() {
    let mut session = InterpreterSession::new(42, 29, ActiveShell::WindowsPowerShell);
    let decision = session
        .prepare_submission(PrepareSubmissionV1 {
            session_id: 42,
            command_sequence: 29,
            shell: ActiveShell::WindowsPowerShell,
            familiar_enabled: true,
            evidence: LineEvidence::Reliable,
            raw_line: "tail -n 2 input.txt".to_string(),
        })
        .expect("classify finite tail");
    let request_id = match decision.decision {
        FrontendDecisionKindV1::InvokePrepared { request_id, .. } => request_id,
        other => panic!("expected a prepared tail plan, got {other:?}"),
    };
    assert_eq!(
        session.consume_prepared(&request_id),
        Some(PreparedRequestV1 {
            protocol: "wingman.run".to_string(),
            version: 1,
            kind: PreparedRequestKindV1::Execute {
                plan: ExecutionPlanV1 {
                    stages: vec![StagePlanV1::TailLines {
                        count: 2,
                        path: Some(validate_path_value("input.txt").unwrap()),
                    }],
                    redirect: None,
                },
            },
        })
    );
}

#[test]
fn reliable_familiar_follow_is_stored_as_a_typed_file_source() {
    let mut session = InterpreterSession::new(42, 31, ActiveShell::WindowsPowerShell);
    let decision = session
        .prepare_submission(PrepareSubmissionV1 {
            session_id: 42,
            command_sequence: 31,
            shell: ActiveShell::WindowsPowerShell,
            familiar_enabled: true,
            evidence: LineEvidence::Reliable,
            raw_line: "tail --follow -n 2 input.txt".to_string(),
        })
        .expect("classify follow tail");
    let request_id = match decision.decision {
        FrontendDecisionKindV1::InvokePrepared { request_id, .. } => request_id,
        other => panic!("expected a prepared follow plan, got {other:?}"),
    };
    assert_eq!(
        session.consume_prepared(&request_id),
        Some(PreparedRequestV1 {
            protocol: "wingman.run".to_string(),
            version: 1,
            kind: PreparedRequestKindV1::Execute {
                plan: ExecutionPlanV1 {
                    stages: vec![StagePlanV1::FollowFile {
                        count: 2,
                        path: validate_path_value("input.txt").unwrap(),
                    }],
                    redirect: None,
                },
            },
        })
    );
}

#[test]
fn reliable_familiar_grep_is_stored_as_a_typed_search_stage() {
    let mut session = InterpreterSession::new(43, 34, ActiveShell::WindowsPowerShell);
    let decision = session
        .prepare_submission(PrepareSubmissionV1 {
            session_id: 43,
            command_sequence: 34,
            shell: ActiveShell::WindowsPowerShell,
            familiar_enabled: true,
            evidence: LineEvidence::Reliable,
            raw_line: "grep -in TODO input.txt".to_string(),
        })
        .expect("classify grep");
    let request_id = match decision.decision {
        FrontendDecisionKindV1::InvokePrepared { request_id, .. } => request_id,
        other => panic!("expected a prepared grep plan, got {other:?}"),
    };
    assert_eq!(
        session.consume_prepared(&request_id),
        Some(PreparedRequestV1 {
            protocol: "wingman.run".to_string(),
            version: 1,
            kind: PreparedRequestKindV1::Execute {
                plan: ExecutionPlanV1 {
                    stages: vec![StagePlanV1::SearchText {
                        pattern: "TODO".to_string(),
                        paths: vec![validate_path_value("input.txt").unwrap()],
                        ignore_case: true,
                        line_numbers: true,
                        invert_match: false,
                        fixed_strings: false,
                        recursive: false,
                    }],
                    redirect: None,
                },
            },
        })
    );
}
