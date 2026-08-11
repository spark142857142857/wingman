use crate::interpreter::{ExecutionPlanV1, StagePlanV1};
use crate::runner_cancel::RunnerCancellationV1;
use crate::runner_mutation::MutationExecutionErrorV1;
use crate::runner_transfer::execute_move;
use std::io::Write;

pub(crate) fn execute_mv_to<E: Write>(
    plan: &ExecutionPlanV1,
    stderr: &mut E,
    cancellation: &RunnerCancellationV1,
) -> Option<Result<u8, MutationExecutionErrorV1>> {
    let [StagePlanV1::MovePath {
        source,
        destination,
        existing_destination,
    }] = plan.stages.as_slice()
    else {
        return None;
    };
    if plan.redirect.is_some() {
        return None;
    }
    Some(execute_move(
        source,
        destination,
        *existing_destination,
        stderr,
        cancellation,
    ))
}
