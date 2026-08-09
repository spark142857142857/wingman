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

    assert_eq!(
        session.prepare_submission("pwd", true),
        Err(TerminalPrepareErrorV1::PromptNotValidated)
    );
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

fn readiness(nonce: &str, sequence: u64) -> EditorReadinessFrameV1 {
    EditorReadinessFrameV1 {
        nonce: nonce.to_string(),
        sequence,
        shell: ActiveShell::WindowsPowerShell,
        shell_depth: 0,
        location_kind: EditorLocationKindV1::FileSystem,
        adapter_capability: EditorAdapterCapabilityV1::PsReadLineReplaceV1,
    }
}
