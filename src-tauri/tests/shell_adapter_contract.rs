use wingman_lib::interpreter::{ActiveShell, FrontendDecisionKindV1, FrontendDecisionV1};
use wingman_lib::shell_adapter::build_prepared_editor_write;
use wingman_lib::terminal_session::EditorSnapshotV1;

#[test]
fn powershell_replacement_uses_only_editor_shape_and_the_opaque_request_id() {
    let request_id = "0123456789abcdef0123456789abcdef";
    let decision = FrontendDecisionV1 {
        session_id: 81,
        command_sequence: 4,
        decision: FrontendDecisionKindV1::InvokePrepared {
            request_id: request_id.to_string(),
            display_line: "pwd".to_string(),
        },
    };

    assert_eq!(
        build_prepared_editor_write(
            ActiveShell::WindowsPowerShell,
            &decision,
            EditorSnapshotV1 {
                character_count: 3,
                cursor: 2,
            },
        )
        .expect("valid prepared replacement"),
        format!("\u{18}\u{17}Invoke-WingmanPrepared -RequestId '{request_id}'\r")
    );
}
