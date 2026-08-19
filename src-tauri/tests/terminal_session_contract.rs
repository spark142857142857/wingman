use wingman_lib::interpreter::{ActiveShell, FrontendDecisionKindV1};
use wingman_lib::terminal_session::{
    EditorSnapshotV1, TerminalInputActionV1, TerminalPrepareErrorV1, TerminalSessionV1,
};
use wingman_lib::transport::{
    EditorAdapterCapabilityV1, EditorLocationKindV1, EditorReadinessFrameV1,
};

#[test]
fn a_new_session_cannot_prepare_before_a_valid_prompt_marker() {
    let mut session = TerminalSessionV1::new(501, ActiveShell::WindowsPowerShell);

    assert!(!session.editor_ready());
    assert_eq!(
        session.prepare_submission("pwd", true),
        Err(TerminalPrepareErrorV1::PromptNotValidated)
    );
}

#[test]
fn editor_readiness_is_observable_only_while_the_verified_editor_cycle_is_clean() {
    let mut session = TerminalSessionV1::new(514, ActiveShell::WindowsPowerShell);
    let nonce = session.integration_nonce().to_string();

    assert!(!session.editor_ready());
    assert!(!session.verified_prompt_observed());
    assert!(session.apply_editor_readiness(&readiness(&nonce, 1)));
    assert!(session.editor_ready());
    assert!(session.verified_prompt_observed());

    let actions = session.handle_terminal_input("pwd\r", false);
    assert!(matches!(
        actions.last(),
        Some(TerminalInputActionV1::Prepared {
            decision: wingman_lib::interpreter::FrontendDecisionV1 {
                decision: FrontendDecisionKindV1::PassThrough { raw_line },
                ..
            },
            editor: EditorSnapshotV1 {
                character_count: 3,
                cursor: 3,
            },
        }) if raw_line == "pwd"
    ));
    assert!(!session.editor_ready());
    assert!(session.verified_prompt_observed());
}

#[test]
fn production_oob_mode_never_treats_pty_output_as_readiness() {
    let mut session = TerminalSessionV1::new(513, ActiveShell::WindowsPowerShell);
    let marker = format!(
        "\u{1b}]777;wingman-prompt;1;{};1;powershell;0;filesystem;psreadline-replace-v1\u{7}",
        session.integration_nonce()
    );
    session.disable_pty_readiness();

    assert_eq!(session.ingest_pty_output(&marker), marker);
    assert_eq!(
        session.prepare_submission("pwd", true),
        Err(TerminalPrepareErrorV1::PromptNotValidated)
    );
}

#[test]
fn a_split_private_prompt_marker_is_removed_and_enables_preparation() {
    let mut session = TerminalSessionV1::new(502, ActiveShell::WindowsPowerShell);
    let marker = format!(
        "\u{1b}]777;wingman-prompt;1;{};1;powershell;0;filesystem;psreadline-replace-v1\u{7}",
        session.integration_nonce()
    );
    let split_at = marker.len() / 2;

    assert_eq!(
        session.ingest_pty_output(&format!("booted\r\n{}", &marker[..split_at])),
        "booted\r\n"
    );
    assert_eq!(
        session.ingest_pty_output(&format!("{}PS C:\\work> ", &marker[split_at..])),
        "PS C:\\work> "
    );

    let decision = session
        .prepare_submission("pwd", true)
        .expect("validated prompt permits preparation");
    assert!(matches!(
        decision.decision,
        FrontendDecisionKindV1::InvokePrepared { .. }
    ));
}

#[test]
fn rust_mirrors_plain_input_and_holds_enter_for_the_prepared_decision() {
    let mut session = TerminalSessionV1::new(503, ActiveShell::WindowsPowerShell);
    let marker = format!(
        "\u{1b}]777;wingman-prompt;1;{};1;powershell;0;filesystem;psreadline-replace-v1\u{7}",
        session.integration_nonce()
    );
    assert_eq!(session.ingest_pty_output(&marker), "");

    let actions = session.handle_terminal_input("pwd\r", true);
    assert_eq!(
        actions.first(),
        Some(&TerminalInputActionV1::Forward {
            data: "pwd".to_string(),
        })
    );
    assert!(matches!(
        actions.get(1),
        Some(TerminalInputActionV1::Prepared {
            decision: wingman_lib::interpreter::FrontendDecisionV1 {
                decision: FrontendDecisionKindV1::InvokePrepared { display_line, .. },
                ..
            },
            editor: EditorSnapshotV1 {
                character_count: 3,
                cursor: 3,
            },
        }) if display_line == "pwd"
    ));
    assert_eq!(actions.len(), 2);
}

#[test]
fn an_unknown_edit_keeps_the_whole_submission_native() {
    let mut session = TerminalSessionV1::new(504, ActiveShell::WindowsPowerShell);
    let marker = format!(
        "\u{1b}]777;wingman-prompt;1;{};1;powershell;0;filesystem;psreadline-replace-v1\u{7}",
        session.integration_nonce()
    );
    assert_eq!(session.ingest_pty_output(&marker), "");

    assert_eq!(
        session.handle_terminal_input("\u{1b}[Zpwd\r", true),
        vec![TerminalInputActionV1::Forward {
            data: "\u{1b}[Zpwd\r".to_string(),
        }]
    );
}

#[test]
fn cmd_marker_shaped_output_never_enables_familiar_interception() {
    let mut session = TerminalSessionV1::new(508, ActiveShell::Cmd);
    let marker = format!(
        "\u{1b}]777;wingman-prompt;1;{};1;cmd;0;filesystem\u{7}",
        session.integration_nonce()
    );

    assert_eq!(session.ingest_pty_output(&marker), marker);
    assert_eq!(
        session.prepare_submission("pwd", true),
        Err(TerminalPrepareErrorV1::PromptNotValidated)
    );
}

#[test]
fn powershell_marker_without_replacement_capability_is_not_trusted() {
    let mut session = TerminalSessionV1::new(509, ActiveShell::WindowsPowerShell);
    let marker = format!(
        "\u{1b}]777;wingman-prompt;1;{};1;powershell;0;filesystem\u{7}",
        session.integration_nonce()
    );

    assert_eq!(session.ingest_pty_output(&marker), marker);
    assert_eq!(
        session.prepare_submission("pwd", true),
        Err(TerminalPrepareErrorV1::PromptNotValidated)
    );
}

#[test]
fn an_oversized_line_is_forwarded_without_partial_interpretation() {
    let mut session = TerminalSessionV1::new(505, ActiveShell::WindowsPowerShell);
    let marker = format!(
        "\u{1b}]777;wingman-prompt;1;{};1;powershell;0;filesystem;psreadline-replace-v1\u{7}",
        session.integration_nonce()
    );
    assert_eq!(session.ingest_pty_output(&marker), "");
    let line = "a".repeat(16 * 1024 + 1);

    assert_eq!(
        session.handle_terminal_input(&format!("{line}\r"), true),
        vec![TerminalInputActionV1::Forward {
            data: format!("{line}\r"),
        }]
    );
}

#[test]
fn ctrl_c_discards_the_line_and_only_the_next_prompt_reopens_editing() {
    let mut session = TerminalSessionV1::new(506, ActiveShell::WindowsPowerShell);
    let first_marker = format!(
        "\u{1b}]777;wingman-prompt;1;{};1;powershell;0;filesystem;psreadline-replace-v1\u{7}",
        session.integration_nonce()
    );
    assert_eq!(session.ingest_pty_output(&first_marker), "");
    assert_eq!(
        session.handle_terminal_input("pwd\u{3}", true),
        vec![TerminalInputActionV1::Forward {
            data: "pwd\u{3}".to_string(),
        }]
    );

    let second_marker = format!(
        "\u{1b}]777;wingman-prompt;1;{};2;powershell;0;filesystem;psreadline-replace-v1\u{7}",
        session.integration_nonce()
    );
    assert_eq!(session.ingest_pty_output(&second_marker), "");
    let actions = session.handle_terminal_input("pwd\r", true);
    assert!(matches!(
        actions.get(1),
        Some(TerminalInputActionV1::Prepared {
            decision: wingman_lib::interpreter::FrontendDecisionV1 {
                decision: FrontendDecisionKindV1::InvokePrepared { .. },
                ..
            },
            ..
        })
    ));
}

#[test]
fn prepared_action_captures_a_middle_cursor_editor_snapshot() {
    let mut session = TerminalSessionV1::new(507, ActiveShell::WindowsPowerShell);
    let marker = format!(
        "\u{1b}]777;wingman-prompt;1;{};1;powershell;0;filesystem;psreadline-replace-v1\u{7}",
        session.integration_nonce()
    );
    assert_eq!(session.ingest_pty_output(&marker), "");

    let actions = session.handle_terminal_input("pwd\u{1b}[D\r", true);
    assert!(matches!(
        actions.get(1),
        Some(TerminalInputActionV1::Prepared {
            editor: EditorSnapshotV1 {
                character_count: 3,
                cursor: 2,
            },
            ..
        })
    ));
}

#[test]
fn unicode_scalar_input_is_mirrored_without_utf16_or_display_width_drift() {
    let mut session = ready_session(520);
    let line = "echo 한글 한 e\u{301} 🚀 漢字";
    let actions = session.handle_terminal_input(&format!("{line}\r"), false);

    assert!(matches!(
        actions.last(),
        Some(TerminalInputActionV1::Prepared {
            decision: wingman_lib::interpreter::FrontendDecisionV1 {
                decision: FrontendDecisionKindV1::PassThrough { raw_line },
                ..
            },
            editor: EditorSnapshotV1 {
                character_count,
                cursor,
            },
        }) if raw_line == line
            && *character_count == line.chars().count()
            && *cursor == line.chars().count()
    ));
}

#[test]
fn known_cursor_delete_and_backspace_edits_reconstruct_the_exact_line() {
    let mut session = ready_session(521);
    let actions =
        session.handle_terminal_input("pxwd\u{1b}[H\u{1b}[C\u{1b}[3~\u{1b}[Fz\u{7f}\r", false);

    assert!(matches!(
        actions.last(),
        Some(TerminalInputActionV1::Prepared {
            decision: wingman_lib::interpreter::FrontendDecisionV1 {
                decision: FrontendDecisionKindV1::PassThrough { raw_line },
                ..
            },
            editor: EditorSnapshotV1 {
                character_count: 3,
                cursor: 3,
            },
        }) if raw_line == "pwd"
    ));
}

#[test]
fn unicode_backspace_and_middle_insertion_use_scalar_boundaries() {
    let mut session = ready_session(522);
    let actions = session.handle_terminal_input("echo 한글🚀\u{7f}\u{1b}[D!\r", false);

    assert!(matches!(
        actions.last(),
        Some(TerminalInputActionV1::Prepared {
            decision: wingman_lib::interpreter::FrontendDecisionV1 {
                decision: FrontendDecisionKindV1::PassThrough { raw_line },
                ..
            },
            editor: EditorSnapshotV1 {
                character_count: 8,
                cursor: 7,
            },
        }) if raw_line == "echo 한!글"
    ));
}

#[test]
fn prediction_accepting_keys_at_the_line_end_force_native_fallback() {
    for (session_id, sequence) in [(523, "\u{1b}[C"), (524, "\u{1b}[F"), (525, "\u{1b}[4~")] {
        let mut session = ready_session(session_id);
        let data = format!("pwd{sequence}\r");
        assert_eq!(
            session.handle_terminal_input(&data, true),
            vec![TerminalInputActionV1::Forward { data }]
        );
    }
}

#[test]
fn completion_history_search_and_unknown_keys_force_native_fallback() {
    let uncertain_inputs = [
        "\t",
        "\u{12}",
        "\u{1b}[A",
        "\u{1b}[B",
        "\u{1b}[18~",
        "\u{1b}[19~",
        "\u{1b}[20~",
        "\u{1b}[Z",
    ];
    for (offset, uncertain) in uncertain_inputs.into_iter().enumerate() {
        let mut session = ready_session(526 + offset as u64);
        let data = format!("p{uncertain}wd\r");
        assert_eq!(
            session.handle_terminal_input(&data, true),
            vec![TerminalInputActionV1::Forward { data }],
            "uncertain input {uncertain:?} was prepared"
        );
    }
}

#[test]
fn foreground_children_keep_all_following_input_native_until_parent_readiness() {
    for (offset, child_line) in ["python", "vim", "cmd", "powershell"]
        .into_iter()
        .enumerate()
    {
        let mut session = ready_session(534 + offset as u64);
        assert!(matches!(
            session.handle_terminal_input(&format!("{child_line}\r"), true).last(),
            Some(TerminalInputActionV1::Prepared {
                decision: wingman_lib::interpreter::FrontendDecisionV1 {
                    decision: FrontendDecisionKindV1::PassThrough { raw_line },
                    ..
                },
                ..
            }) if raw_line == child_line
        ));
        assert_eq!(
            session.handle_terminal_input("pwd\r", true),
            vec![TerminalInputActionV1::Forward {
                data: "pwd\r".to_string(),
            }],
        );
    }
}

#[test]
fn a_multi_submission_chunk_is_never_partially_prepared() {
    let mut session = TerminalSessionV1::new(510, ActiveShell::WindowsPowerShell);
    let marker = format!(
        "\u{1b}]777;wingman-prompt;1;{};1;powershell;0;filesystem;psreadline-replace-v1\u{7}",
        session.integration_nonce()
    );
    assert_eq!(session.ingest_pty_output(&marker), "");

    assert_eq!(
        session.handle_terminal_input("pwd\rwhoami\r", true),
        vec![TerminalInputActionV1::Forward {
            data: "pwd\rwhoami\r".to_string(),
        }]
    );
}

#[test]
fn confirmed_line_breaking_paste_suspends_interception_until_a_fresh_marker() {
    let mut session = TerminalSessionV1::new(72, ActiveShell::WindowsPowerShell);
    let marker = format!(
        "\u{1b}]777;wingman-prompt;1;{};1;powershell;0;filesystem;psreadline-replace-v1\u{7}",
        session.integration_nonce()
    );
    assert_eq!(session.ingest_pty_output(&marker), "");

    session.suspend_for_native_paste("pwd\r\nwhoami\n");

    assert_eq!(
        session.handle_terminal_input("pwd\r", true),
        vec![TerminalInputActionV1::Forward {
            data: "pwd\r".to_string(),
        }]
    );
}

#[test]
fn readiness_arriving_after_any_forwarded_input_cannot_upgrade_that_editor_cycle() {
    let mut session = TerminalSessionV1::new(511, ActiveShell::WindowsPowerShell);
    let nonce = session.integration_nonce().to_string();

    assert_eq!(
        session.handle_terminal_input("c", true),
        vec![TerminalInputActionV1::Forward {
            data: "c".to_string(),
        }]
    );
    assert!(!session.apply_editor_readiness(&readiness(&nonce, 1)));
    assert_eq!(
        session.handle_terminal_input("at\r", true),
        vec![TerminalInputActionV1::Forward {
            data: "at\r".to_string(),
        }]
    );

    assert!(session.apply_editor_readiness(&readiness(&nonce, 2)));
    assert!(matches!(
        session.handle_terminal_input("pwd\r", true).last(),
        Some(TerminalInputActionV1::Prepared { .. })
    ));
}

#[test]
fn focus_reporting_before_readiness_does_not_dirty_the_editor_cycle() {
    let mut session = TerminalSessionV1::new(515, ActiveShell::WindowsPowerShell);
    let nonce = session.integration_nonce().to_string();

    assert_eq!(
        session.handle_terminal_input("\u{1b}[I", true),
        vec![TerminalInputActionV1::Forward {
            data: "\u{1b}[I".to_string(),
        }]
    );
    assert!(session.apply_editor_readiness(&readiness(&nonce, 1)));
    assert!(session.editor_ready());
}

#[test]
fn focus_reporting_preserves_a_verified_editor_buffer() {
    let mut session = TerminalSessionV1::new(516, ActiveShell::WindowsPowerShell);
    let nonce = session.integration_nonce().to_string();
    assert!(session.apply_editor_readiness(&readiness(&nonce, 1)));

    assert_eq!(
        session.handle_terminal_input("\u{1b}[I\u{1b}[O", true),
        vec![TerminalInputActionV1::Forward {
            data: "\u{1b}[I\u{1b}[O".to_string(),
        }]
    );
    assert!(session.editor_ready());
    assert!(matches!(
        session.handle_terminal_input("pwd\r", true).last(),
        Some(TerminalInputActionV1::Prepared { .. })
    ));
}

#[test]
fn split_crlf_after_native_fallback_advances_only_one_readiness_cycle() {
    let mut session = TerminalSessionV1::new(512, ActiveShell::WindowsPowerShell);
    let nonce = session.integration_nonce().to_string();

    assert_eq!(
        session.handle_terminal_input("pwd\r", true),
        vec![TerminalInputActionV1::Forward {
            data: "pwd\r".to_string(),
        }]
    );
    assert_eq!(
        session.handle_terminal_input("\n", true),
        vec![TerminalInputActionV1::Forward {
            data: "\n".to_string(),
        }]
    );
    assert!(session.apply_editor_readiness(&readiness(&nonce, 2)));
}

#[test]
fn authenticated_nested_powershell_depth_pushes_and_pops_one_level() {
    let mut session = TerminalSessionV1::new(517, ActiveShell::WindowsPowerShell);
    let nonce = session.integration_nonce().to_string();
    assert!(session.apply_editor_readiness(&readiness_at_depth(&nonce, 1, 0)));
    assert_eq!(session.powershell_depth(), 0);

    let native_enter = session.handle_terminal_input("$host.EnterNestedPrompt()\r", true);
    assert!(matches!(
        native_enter.last(),
        Some(TerminalInputActionV1::Prepared {
            decision: wingman_lib::interpreter::FrontendDecisionV1 {
                decision: FrontendDecisionKindV1::PassThrough { raw_line },
                ..
            },
            ..
        }) if raw_line == "$host.EnterNestedPrompt()"
    ));
    assert!(session.apply_editor_readiness(&readiness_at_depth(&nonce, 2, 1)));
    assert_eq!(session.powershell_depth(), 1);
    assert!(matches!(
        session.handle_terminal_input("pwd\r", true).last(),
        Some(TerminalInputActionV1::Prepared {
            decision: wingman_lib::interpreter::FrontendDecisionV1 {
                decision: FrontendDecisionKindV1::InvokePrepared { .. },
                ..
            },
            ..
        })
    ));

    assert!(session.apply_editor_readiness(&readiness_at_depth(&nonce, 3, 1)));
    let native_exit = session.handle_terminal_input("exit\r", true);
    assert!(matches!(
        native_exit.last(),
        Some(TerminalInputActionV1::Prepared {
            decision: wingman_lib::interpreter::FrontendDecisionV1 {
                decision: FrontendDecisionKindV1::PassThrough { raw_line },
                ..
            },
            ..
        }) if raw_line == "exit"
    ));
    assert!(session.apply_editor_readiness(&readiness_at_depth(&nonce, 4, 0)));
    assert_eq!(session.powershell_depth(), 0);
}

#[test]
fn nested_powershell_depth_cannot_jump_or_start_above_root() {
    let mut new_session = TerminalSessionV1::new(518, ActiveShell::WindowsPowerShell);
    let new_nonce = new_session.integration_nonce().to_string();
    assert!(!new_session.apply_editor_readiness(&readiness_at_depth(&new_nonce, 1, 1)));
    assert!(!new_session.editor_ready());

    let mut session = TerminalSessionV1::new(519, ActiveShell::WindowsPowerShell);
    let nonce = session.integration_nonce().to_string();
    assert!(session.apply_editor_readiness(&readiness_at_depth(&nonce, 1, 0)));
    assert_eq!(
        session
            .handle_terminal_input("$host.EnterNestedPrompt()\r", true)
            .len(),
        2
    );
    assert!(!session.apply_editor_readiness(&readiness_at_depth(&nonce, 2, 2)));
    assert!(!session.editor_ready());
    assert_eq!(session.powershell_depth(), 0);
}

fn readiness(nonce: &str, sequence: u64) -> EditorReadinessFrameV1 {
    readiness_at_depth(nonce, sequence, 0)
}

fn ready_session(session_id: u64) -> TerminalSessionV1 {
    let mut session = TerminalSessionV1::new(session_id, ActiveShell::WindowsPowerShell);
    let nonce = session.integration_nonce().to_string();
    assert!(session.apply_editor_readiness(&readiness(&nonce, 1)));
    session
}

fn readiness_at_depth(nonce: &str, sequence: u64, shell_depth: u32) -> EditorReadinessFrameV1 {
    EditorReadinessFrameV1 {
        nonce: nonce.to_string(),
        sequence,
        shell: ActiveShell::WindowsPowerShell,
        shell_depth,
        location_kind: EditorLocationKindV1::FileSystem,
        adapter_capability: EditorAdapterCapabilityV1::PsReadLineReplaceV1,
    }
}
