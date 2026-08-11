use crate::interpreter::{
    validate_prepared_request, InterpreterSession, PreparedRequestKindV1,
    RunnerRequestValidationErrorV1, StagePlanV1,
};
use crate::runner_cancel::RunnerCancellationV1;
use crate::runner_cp::execute_cp_to;
use crate::runner_find::execute_find_to;
use crate::runner_grep::execute_recursive_grep_to;
use crate::runner_ls::execute_ls_to;
use crate::runner_mkdir::execute_mkdir_to;
use crate::runner_mutation::MutationExecutionErrorV1;
use crate::runner_mv::execute_mv_to;
use crate::runner_readonly::{execute_readonly_plan_to, ReadonlyExecutionErrorV1};
use crate::runner_rm::execute_rm_to;
use crate::runner_touch::execute_touch_to;
use crate::runner_which::execute_which_to;
use std::io::{self, Write};

const CLEAR_TERMINAL_SEQUENCE: &[u8] = b"\x1b[2J\x1b[H";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunnerOutcomeV1 {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunnerDispatchErrorV1 {
    UnknownOrConsumedRequest,
    InvalidPreparedRequest(RunnerRequestValidationErrorV1),
    UnsupportedExecutionPlan,
    OutputFailure { kind: io::ErrorKind },
}

pub fn dispatch_prepared(
    session: &mut InterpreterSession,
    request_id: &str,
) -> Result<RunnerOutcomeV1, RunnerDispatchErrorV1> {
    let request = session
        .consume_prepared(request_id)
        .ok_or(RunnerDispatchErrorV1::UnknownOrConsumedRequest)?;

    execute_prepared(request)
}

pub fn execute_prepared(
    request: crate::interpreter::PreparedRequestV1,
) -> Result<RunnerOutcomeV1, RunnerDispatchErrorV1> {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit_code = execute_prepared_to(request, &mut stdout, &mut stderr)?;
    Ok(RunnerOutcomeV1 {
        stdout,
        stderr,
        exit_code,
    })
}

pub fn execute_prepared_to<W: Write, E: Write>(
    request: crate::interpreter::PreparedRequestV1,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<u8, RunnerDispatchErrorV1> {
    execute_prepared_to_with_cancellation(request, stdout, stderr, &RunnerCancellationV1::new())
}

pub fn execute_prepared_to_with_cancellation<W: Write, E: Write>(
    request: crate::interpreter::PreparedRequestV1,
    stdout: &mut W,
    stderr: &mut E,
    cancellation: &RunnerCancellationV1,
) -> Result<u8, RunnerDispatchErrorV1> {
    validate_prepared_request(&request).map_err(RunnerDispatchErrorV1::InvalidPreparedRequest)?;
    if cancellation.is_cancelled() {
        return Ok(130);
    }
    match request.kind {
        PreparedRequestKindV1::Reject {
            diagnostic,
            exit_code,
        } => {
            write_line(stderr, &diagnostic)?;
            Ok(exit_code)
        }
        PreparedRequestKindV1::Control {
            response,
            exit_code,
        } => {
            write_line(stdout, &response)?;
            Ok(exit_code)
        }
        PreparedRequestKindV1::Execute { plan }
            if plan.redirect.is_none()
                && plan.stages.as_slice() == [StagePlanV1::PrintWorkingDirectory] =>
        {
            match std::env::current_dir() {
                Ok(cwd) => {
                    write_line(stdout, &cwd.display().to_string())?;
                    Ok(0)
                }
                Err(_) => {
                    write_line(
                        stderr,
                        "wingman pwd: unable to read current working directory",
                    )?;
                    Ok(1)
                }
            }
        }
        PreparedRequestKindV1::Execute { plan }
            if plan.redirect.is_none()
                && plan.stages.as_slice() == [StagePlanV1::ClearTerminal] =>
        {
            if cancellation.is_cancelled() {
                return Ok(130);
            }
            stdout
                .write_all(CLEAR_TERMINAL_SEQUENCE)
                .and_then(|()| stdout.flush())
                .map_err(|error| RunnerDispatchErrorV1::OutputFailure { kind: error.kind() })?;
            Ok(if cancellation.is_cancelled() { 130 } else { 0 })
        }
        PreparedRequestKindV1::Execute { plan }
            if plan.redirect.is_none()
                && matches!(plan.stages.as_slice(), [StagePlanV1::FindExecutable { .. }]) =>
        {
            let [StagePlanV1::FindExecutable { name }] = plan.stages.as_slice() else {
                unreachable!();
            };
            execute_which_to(name, stdout, stderr, cancellation)
                .map_err(|error| RunnerDispatchErrorV1::OutputFailure { kind: error.kind() })
        }
        PreparedRequestKindV1::Execute { plan } => {
            if let Some(result) = execute_mkdir_to(&plan, stderr, cancellation) {
                return result.map_err(|error| match error {
                    MutationExecutionErrorV1::Output { kind } => {
                        RunnerDispatchErrorV1::OutputFailure { kind }
                    }
                });
            }
            if let Some(result) = execute_touch_to(&plan, stderr, cancellation) {
                return result.map_err(|error| match error {
                    MutationExecutionErrorV1::Output { kind } => {
                        RunnerDispatchErrorV1::OutputFailure { kind }
                    }
                });
            }
            if let Some(result) = execute_cp_to(&plan, stderr, cancellation) {
                return result.map_err(|error| match error {
                    MutationExecutionErrorV1::Output { kind } => {
                        RunnerDispatchErrorV1::OutputFailure { kind }
                    }
                });
            }
            if let Some(result) = execute_mv_to(&plan, stderr, cancellation) {
                return result.map_err(|error| match error {
                    MutationExecutionErrorV1::Output { kind } => {
                        RunnerDispatchErrorV1::OutputFailure { kind }
                    }
                });
            }
            if let Some(result) = execute_rm_to(&plan, stderr, cancellation) {
                return result.map_err(|error| match error {
                    MutationExecutionErrorV1::Output { kind } => {
                        RunnerDispatchErrorV1::OutputFailure { kind }
                    }
                });
            }
            if let Some(result) = execute_find_to(&plan, stdout, stderr, cancellation) {
                return result.map_err(|error| match error {
                    ReadonlyExecutionErrorV1::UnsupportedPlan => {
                        RunnerDispatchErrorV1::UnsupportedExecutionPlan
                    }
                    ReadonlyExecutionErrorV1::Output { kind } => {
                        RunnerDispatchErrorV1::OutputFailure { kind }
                    }
                });
            }
            if let Some(result) = execute_ls_to(&plan, stdout, stderr, cancellation) {
                return result.map_err(|error| match error {
                    ReadonlyExecutionErrorV1::UnsupportedPlan => {
                        RunnerDispatchErrorV1::UnsupportedExecutionPlan
                    }
                    ReadonlyExecutionErrorV1::Output { kind } => {
                        RunnerDispatchErrorV1::OutputFailure { kind }
                    }
                });
            }
            if let Some(result) = execute_recursive_grep_to(&plan, stdout, stderr, cancellation) {
                return result.map_err(|error| match error {
                    ReadonlyExecutionErrorV1::UnsupportedPlan => {
                        RunnerDispatchErrorV1::UnsupportedExecutionPlan
                    }
                    ReadonlyExecutionErrorV1::Output { kind } => {
                        RunnerDispatchErrorV1::OutputFailure { kind }
                    }
                });
            }
            let result = execute_readonly_plan_to(&plan, stdout, stderr, cancellation);
            if cancellation.is_cancelled() {
                return Ok(130);
            }
            result.map_err(|error| match error {
                ReadonlyExecutionErrorV1::UnsupportedPlan => {
                    RunnerDispatchErrorV1::UnsupportedExecutionPlan
                }
                ReadonlyExecutionErrorV1::Output { kind } => {
                    RunnerDispatchErrorV1::OutputFailure { kind }
                }
            })
        }
    }
}

fn write_line(writer: &mut impl Write, value: &str) -> Result<(), RunnerDispatchErrorV1> {
    writer
        .write_all(value.as_bytes())
        .and_then(|()| writer.write_all(b"\r\n"))
        .and_then(|()| writer.flush())
        .map_err(|error| RunnerDispatchErrorV1::OutputFailure { kind: error.kind() })
}
