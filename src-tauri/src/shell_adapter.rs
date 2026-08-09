use crate::interpreter::{ActiveShell, FrontendDecisionKindV1, FrontendDecisionV1};
use crate::terminal_session::EditorSnapshotV1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EditorReplacementErrorV1 {
    NotPrepared,
    InvalidRequestId,
    EditorSnapshotMismatch,
    UnsupportedShell,
}

pub fn build_prepared_editor_write(
    shell: ActiveShell,
    decision: &FrontendDecisionV1,
    editor: EditorSnapshotV1,
) -> Result<String, EditorReplacementErrorV1> {
    let FrontendDecisionKindV1::InvokePrepared {
        request_id,
        display_line,
    } = &decision.decision
    else {
        return Err(EditorReplacementErrorV1::NotPrepared);
    };
    if request_id.len() != 32 || !request_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(EditorReplacementErrorV1::InvalidRequestId);
    }
    if editor.cursor > editor.character_count
        || display_line.chars().count() != editor.character_count
    {
        return Err(EditorReplacementErrorV1::EditorSnapshotMismatch);
    }
    if shell != ActiveShell::WindowsPowerShell {
        return Err(EditorReplacementErrorV1::UnsupportedShell);
    }

    let mut write = String::from("\u{18}\u{17}Invoke-WingmanPrepared -RequestId '");
    write.push_str(request_id);
    write.push_str("'\r");
    Ok(write)
}
