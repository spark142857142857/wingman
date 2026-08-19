use crate::interpreter::{
    parse_familiar_control, ActiveShell, FamiliarControlEffectV1, FrontendDecisionKindV1,
};
use crate::shell_adapter::{build_prepared_editor_write, EditorReplacementErrorV1};
use crate::terminal_session::{TerminalInputActionV1, TerminalSessionV1};
use crate::transport::SessionBrokerV1;
use std::io::{self, Write};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionWriteOutcomeV1 {
    Written,
    Stale,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalExecutionOutcomeV1 {
    Native,
    NativeFallback,
    Prepared {
        request_id: String,
        familiar_effect: Option<FamiliarControlEffectV1>,
    },
}

pub fn write_session_input<W: Write + ?Sized>(
    active_session_id: u64,
    client_session_id: u64,
    writer: &mut W,
    data: &str,
) -> io::Result<SessionWriteOutcomeV1> {
    if active_session_id != client_session_id {
        return Ok(SessionWriteOutcomeV1::Stale);
    }

    writer.write_all(data.as_bytes())?;
    writer.flush()?;
    Ok(SessionWriteOutcomeV1::Written)
}

pub fn apply_familiar_effect<F>(
    effect: FamiliarControlEffectV1,
    familiar_enabled: &mut bool,
    mut persist: F,
) -> io::Result<bool>
where
    F: FnMut(bool) -> io::Result<()>,
{
    if let Some(enabled) = effect.enabled() {
        persist(enabled)?;
        *familiar_enabled = enabled;
    }
    Ok(*familiar_enabled)
}

pub struct PreparedTerminalDispatchV1 {
    pub request_id: String,
    pub editor_write: String,
    pub familiar_effect: Option<FamiliarControlEffectV1>,
}

pub struct TerminalDispatchV1 {
    pub native_writes: Vec<String>,
    pub prepared: Option<PreparedTerminalDispatchV1>,
}

#[derive(Debug)]
pub enum TerminalDispatchErrorV1 {
    MultiplePreparedSubmissions,
    MissingPreparedRequest,
    EditorReplacement(EditorReplacementErrorV1),
    Broker(io::Error),
}

pub fn dispatch_terminal_input(
    session: &mut TerminalSessionV1,
    shell: ActiveShell,
    broker: &SessionBrokerV1,
    data: &str,
    familiar_enabled: bool,
) -> Result<TerminalDispatchV1, TerminalDispatchErrorV1> {
    let mut dispatch = TerminalDispatchV1 {
        native_writes: Vec::new(),
        prepared: None,
    };

    for action in session.handle_terminal_input(data, familiar_enabled) {
        match action {
            TerminalInputActionV1::Forward { data } => dispatch.native_writes.push(data),
            TerminalInputActionV1::Prepared { decision, editor } => match &decision.decision {
                FrontendDecisionKindV1::PassThrough { .. } => {
                    dispatch.native_writes.push("\r".to_string());
                }
                FrontendDecisionKindV1::InvokePrepared { request_id, .. } => {
                    if dispatch.prepared.is_some() {
                        return Err(TerminalDispatchErrorV1::MultiplePreparedSubmissions);
                    }
                    let editor_write = build_prepared_editor_write(shell, &decision, editor)
                        .map_err(TerminalDispatchErrorV1::EditorReplacement)?;
                    let request = session
                        .consume_prepared(request_id)
                        .ok_or(TerminalDispatchErrorV1::MissingPreparedRequest)?;
                    broker
                        .register(request_id.clone(), request)
                        .map_err(TerminalDispatchErrorV1::Broker)?;
                    let familiar_effect = match &decision.decision {
                        FrontendDecisionKindV1::InvokePrepared { display_line, .. } => {
                            parse_familiar_control(display_line)
                        }
                        FrontendDecisionKindV1::PassThrough { .. } => None,
                    };
                    dispatch.prepared = Some(PreparedTerminalDispatchV1 {
                        request_id: request_id.clone(),
                        editor_write,
                        familiar_effect,
                    });
                }
            },
        }
    }

    Ok(dispatch)
}

pub fn execute_terminal_input<W: Write + ?Sized>(
    session: &mut TerminalSessionV1,
    shell: ActiveShell,
    broker: &SessionBrokerV1,
    writer: &mut W,
    data: &str,
    familiar_enabled: bool,
) -> io::Result<TerminalExecutionOutcomeV1> {
    let cancellation_error = data
        .contains('\u{3}')
        .then(|| broker.cancel_current_requests())
        .transpose()
        .err();
    let dispatch = match dispatch_terminal_input(session, shell, broker, data, familiar_enabled) {
        Ok(dispatch) => dispatch,
        Err(_) => {
            session.suspend_after_transport_failure();
            writer.write_all(data.as_bytes())?;
            writer.flush()?;
            if let Some(error) = cancellation_error {
                return Err(error);
            }
            return Ok(TerminalExecutionOutcomeV1::NativeFallback);
        }
    };

    let mut wire = dispatch.native_writes.concat();
    let registered_request_id = dispatch
        .prepared
        .as_ref()
        .map(|prepared| prepared.request_id.clone());
    let outcome = if let Some(prepared) = dispatch.prepared {
        wire.push_str(&prepared.editor_write);
        TerminalExecutionOutcomeV1::Prepared {
            request_id: prepared.request_id,
            familiar_effect: prepared.familiar_effect,
        }
    } else {
        TerminalExecutionOutcomeV1::Native
    };

    if let Err(error) = writer
        .write_all(wire.as_bytes())
        .and_then(|()| writer.flush())
    {
        session.suspend_after_transport_failure();
        if let Some(request_id) = registered_request_id {
            let _ = broker.unregister(&request_id);
        }
        return Err(error);
    }
    if let Some(error) = cancellation_error {
        session.suspend_after_transport_failure();
        return Err(error);
    }
    Ok(outcome)
}
