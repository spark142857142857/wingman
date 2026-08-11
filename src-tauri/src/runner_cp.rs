use crate::interpreter::{ExecutionPlanV1, StagePlanV1};
use crate::runner_cancel::RunnerCancellationV1;
use crate::runner_mutation::MutationExecutionErrorV1;
use crate::runner_transfer::execute_copy;
use std::io::Write;

pub(crate) fn execute_cp_to<E: Write>(
    plan: &ExecutionPlanV1,
    stderr: &mut E,
    cancellation: &RunnerCancellationV1,
) -> Option<Result<u8, MutationExecutionErrorV1>> {
    let [StagePlanV1::CopyPath {
        source,
        destination,
        recursive,
        existing_destination,
    }] = plan.stages.as_slice()
    else {
        return None;
    };
    if plan.redirect.is_some() {
        return None;
    }
    Some(execute_copy(
        source,
        destination,
        *recursive,
        *existing_destination,
        stderr,
        cancellation,
    ))
}
