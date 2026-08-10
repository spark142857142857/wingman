use crate::interpreter::{
    ExecutionPlanV1, RedirectModeV1 as PlanRedirectModeV1, StagePlanV1,
    MAX_PREPARED_DIAGNOSTIC_BYTES,
};
use crate::ordered_pipeline::{
    OrderedFinishCauseV1, OrderedFlowV1, OrderedPipelineFaultV1, OrderedPipelineV1,
};
use crate::runner_cancel::RunnerCancellationV1;
use crate::runner_io::{
    prepare_file_io, IoPreparationErrorV1, PreparedInputV1, RedirectModeV1, RedirectSpecV1,
};
use crate::text_stream::{
    RecordFrameV1, RecordStreamWriterV1, TextReadErrorV1, TextStreamWriteErrorV1,
    Utf8RecordDecoderV1, Utf8RecordReaderV1, MAX_RECORD_BYTES,
};
use crate::windows_path::{resolve_path_spec, PathResolutionErrorV1, ValidatedPathSpecV1};
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::thread;
use std::time::Duration;

pub use crate::sort_support::{MAX_SORT_BYTES, MAX_SORT_RECORDS};

pub const MAX_TAIL_BUFFER_RECORDS: usize = 65_536;
pub const MAX_TAIL_BUFFER_BYTES: usize = 16 * 1024 * 1024;
const FOLLOW_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadonlyExecutionErrorV1 {
    UnsupportedPlan,
    Output { kind: io::ErrorKind },
}

struct ReadonlySourceV1<'a> {
    command_name: &'static str,
    paths: Vec<&'a ValidatedPathSpecV1>,
    number_lines: bool,
    grep_is_final: bool,
    grep_direct: bool,
    follow_count: Option<usize>,
}

pub fn execute_readonly_plan_to<W: Write, E: Write>(
    plan: &ExecutionPlanV1,
    stdout: &mut W,
    stderr: &mut E,
    cancellation: &RunnerCancellationV1,
) -> Result<u8, ReadonlyExecutionErrorV1> {
    let source = readonly_source(plan)?;
    if cancellation.is_cancelled() {
        return Ok(130);
    }
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(_) => {
            write_diagnostic(
                stderr,
                &format!(
                    "wingman {}: unable to read current working directory",
                    source.command_name
                ),
            )?;
            return Ok(1);
        }
    };
    let Some(cwd) = cwd.to_str() else {
        write_diagnostic(
            stderr,
            &format!(
                "wingman {}: current working directory is not valid Unicode",
                source.command_name
            ),
        )?;
        return Ok(1);
    };

    let mut resolved_paths = Vec::with_capacity(source.paths.len());
    let mut resolution_failed = false;
    let mut resolution_exit = 1;
    for (index, path) in source.paths.iter().enumerate() {
        if cancellation.is_cancelled() {
            return Ok(130);
        }
        match resolve_path_spec(path, cwd) {
            Ok(path) => resolved_paths.push(path),
            Err(error) => {
                resolution_failed = true;
                let exit_code = match error {
                    PathResolutionErrorV1::InvalidCurrentDirectory => 1,
                    PathResolutionErrorV1::InvalidSpec
                    | PathResolutionErrorV1::TraversalAboveRoot
                    | PathResolutionErrorV1::TooLong => 2,
                };
                resolution_exit = resolution_exit.max(exit_code);
                write_diagnostic(
                    stderr,
                    &operand_diagnostic(
                        source.command_name,
                        index,
                        path,
                        "path cannot be resolved safely",
                    ),
                )?;
            }
        }
    }
    if resolution_failed {
        return Ok(resolution_exit);
    }
    if cancellation.is_cancelled() {
        return Ok(130);
    }

    let redirect = match &plan.redirect {
        Some(redirect) => {
            let path = match resolve_path_spec(&redirect.path, cwd) {
                Ok(path) => path,
                Err(error) => {
                    let exit_code = path_resolution_exit(error);
                    write_diagnostic(
                        stderr,
                        &format!(
                            "wingman {}: redirection target cannot be resolved safely",
                            source.command_name
                        ),
                    )?;
                    return Ok(exit_code);
                }
            };
            Some(RedirectSpecV1 {
                path,
                mode: match redirect.mode {
                    PlanRedirectModeV1::Overwrite => RedirectModeV1::Overwrite,
                    PlanRedirectModeV1::Append => RedirectModeV1::Append,
                },
            })
        }
        None => None,
    };
    if cancellation.is_cancelled() {
        return Ok(130);
    }

    let preparation = prepare_file_io(&resolved_paths, redirect);
    if cancellation.is_cancelled() {
        return Ok(130);
    }
    let mut prepared = match preparation {
        Ok(prepared) => prepared,
        Err(IoPreparationErrorV1::Inputs(errors)) => {
            for error in errors {
                if cancellation.is_cancelled() {
                    return Ok(130);
                }
                let path = source.paths[error.index];
                write_diagnostic(
                    stderr,
                    &operand_diagnostic(
                        source.command_name,
                        error.index,
                        path,
                        "input cannot be opened",
                    ),
                )?;
            }
            return Ok(1);
        }
        Err(IoPreparationErrorV1::Output { .. }) => {
            write_diagnostic(
                stderr,
                &format!(
                    "wingman {}: redirection target cannot be opened",
                    source.command_name
                ),
            )?;
            return Ok(1);
        }
        Err(IoPreparationErrorV1::OutputReparsePoint) => {
            write_diagnostic(
                stderr,
                &format!(
                    "wingman {}: redirection target is or crosses a reparse point",
                    source.command_name
                ),
            )?;
            return Ok(2);
        }
        Err(IoPreparationErrorV1::SameFile { input_index }) => {
            write_diagnostic(
                stderr,
                &format!(
                    "wingman {}: redirection target is the same file as input #{}",
                    source.command_name,
                    input_index + 1
                ),
            )?;
            return Ok(2);
        }
    };
    if cancellation.is_cancelled() {
        return Ok(130);
    }

    let redirected = plan.redirect.is_some();
    let (inputs, output) = prepared.stream_parts_mut();
    let execution = match output {
        Some(output) if source.follow_count.is_some() => {
            execute_follow_to(&mut inputs[0], plan, &source, output, stderr, cancellation)
        }
        Some(output) => execute_stream_to(inputs, plan, &source, output, stderr, cancellation),
        None if source.follow_count.is_some() => {
            execute_follow_to(&mut inputs[0], plan, &source, stdout, stderr, cancellation)
        }
        None => execute_stream_to(inputs, plan, &source, stdout, stderr, cancellation),
    };
    if cancellation.is_cancelled() {
        return Ok(130);
    }
    match execution {
        Ok(exit_code) => Ok(exit_code),
        Err(ReadonlyExecutionErrorV1::Output { .. }) if redirected => {
            write_diagnostic(
                stderr,
                &format!(
                    "wingman {}: redirection output failed and may be partial",
                    source.command_name
                ),
            )?;
            Ok(1)
        }
        Err(error) => Err(error),
    }
}

fn execute_stream_to<W: Write, E: Write>(
    inputs: &mut [PreparedInputV1],
    plan: &ExecutionPlanV1,
    source: &ReadonlySourceV1<'_>,
    writer: &mut W,
    stderr: &mut E,
    cancellation: &RunnerCancellationV1,
) -> Result<u8, ReadonlyExecutionErrorV1> {
    let mut sink = RecordStreamWriterV1::new(&mut *writer);
    let stream = stream_inputs_ordered(inputs, plan, source, &mut sink, cancellation)?;
    if stream.cancelled || cancellation.is_cancelled() {
        drop(sink);
        let _ = writer.flush();
        return Ok(130);
    }
    let finish_result = sink.finish();
    if cancellation.is_cancelled() {
        return Ok(130);
    }
    finish_result.map_err(map_sink_error)?;
    for diagnostic in stream.diagnostics {
        if cancellation.is_cancelled() {
            return Ok(130);
        }
        write_diagnostic(stderr, &diagnostic)?;
    }
    Ok(
        if stream.had_operational_failure || (source.grep_is_final && !stream.grep_matched) {
            1
        } else {
            0
        },
    )
}

fn execute_follow_to<W: Write, E: Write>(
    input: &mut PreparedInputV1,
    plan: &ExecutionPlanV1,
    source: &ReadonlySourceV1<'_>,
    writer: &mut W,
    stderr: &mut E,
    cancellation: &RunnerCancellationV1,
) -> Result<u8, ReadonlyExecutionErrorV1> {
    let count = source
        .follow_count
        .ok_or(ReadonlyExecutionErrorV1::UnsupportedPlan)?;
    let mut sink = RecordStreamWriterV1::new(&mut *writer);
    let mut pipeline = OrderedPipelineV1::new(plan, &mut sink, cancellation, &source.paths)
        .map_err(map_ordered_setup_fault)?;
    let mut decoder = Utf8RecordDecoderV1::new();
    let mut initial: VecDeque<RecordFrameV1> = VecDeque::new();
    let mut initial_bytes = 0usize;
    let mut read_buffer = [0u8; 8192];
    let mut observed_bytes = 0u64;
    let mut stopped = pipeline.starts_stopped();
    let mut operational_failure = false;
    let mut diagnostics = Vec::new();
    let mut stage_fault = None;

    while !stopped && !operational_failure && !cancellation.is_cancelled() {
        let length = match input.file_mut().read(&mut read_buffer) {
            Ok(0) => break,
            Ok(length) => length,
            Err(_) => {
                operational_failure = true;
                diagnostics.push(operand_diagnostic(
                    source.command_name,
                    0,
                    source.paths[0],
                    "input read failed",
                ));
                break;
            }
        };
        observed_bytes = observed_bytes.saturating_add(length as u64);
        let records = match decoder.push(&read_buffer[..length]) {
            Ok(records) => records,
            Err(_) => {
                operational_failure = true;
                diagnostics.push(operand_diagnostic(
                    source.command_name,
                    0,
                    source.paths[0],
                    "input is not valid bounded UTF-8 text",
                ));
                break;
            }
        };
        if count == 0 {
            continue;
        }
        for record in records {
            while initial.len() >= count {
                if let Some(discarded) = initial.pop_front() {
                    initial_bytes = initial_bytes.saturating_sub(discarded.text.len());
                }
            }
            let next_bytes = initial_bytes.saturating_add(record.text.len());
            if initial.len() >= MAX_TAIL_BUFFER_RECORDS || next_bytes > MAX_TAIL_BUFFER_BYTES {
                initial.clear();
                operational_failure = true;
                diagnostics.push("wingman tail: buffer resource limit exceeded".to_string());
                break;
            }
            initial_bytes = next_bytes;
            initial.push_back(record);
        }
    }

    while !stopped && !operational_failure && !cancellation.is_cancelled() {
        let Some(record) = initial.pop_front() else {
            break;
        };
        match push_follow_record(&mut pipeline, record) {
            Ok(OrderedFlowV1::Continue) => {}
            Ok(OrderedFlowV1::StopUpstream) => stopped = true,
            Err(fault) => stage_fault = Some(fault),
        }
        if stage_fault.is_some() {
            break;
        }
    }

    while !stopped && !operational_failure && stage_fault.is_none() && !cancellation.is_cancelled()
    {
        match input.file_mut().read(&mut read_buffer) {
            Ok(0) => match input.file_mut().metadata() {
                Ok(metadata) if metadata.len() < observed_bytes => {
                    operational_failure = true;
                    diagnostics.push(operand_diagnostic(
                        source.command_name,
                        0,
                        source.paths[0],
                        "input was truncated while following",
                    ));
                }
                Ok(_) => thread::sleep(FOLLOW_POLL_INTERVAL),
                Err(_) => {
                    operational_failure = true;
                    diagnostics.push(operand_diagnostic(
                        source.command_name,
                        0,
                        source.paths[0],
                        "input metadata cannot be read while following",
                    ));
                }
            },
            Ok(length) => {
                observed_bytes = observed_bytes.saturating_add(length as u64);
                let records = match decoder.push(&read_buffer[..length]) {
                    Ok(records) => records,
                    Err(_) => {
                        operational_failure = true;
                        diagnostics.push(operand_diagnostic(
                            source.command_name,
                            0,
                            source.paths[0],
                            "input is not valid bounded UTF-8 text",
                        ));
                        continue;
                    }
                };
                for record in records {
                    match push_follow_record(&mut pipeline, record) {
                        Ok(OrderedFlowV1::Continue) => {}
                        Ok(OrderedFlowV1::StopUpstream) => {
                            stopped = true;
                            break;
                        }
                        Err(fault) => {
                            stage_fault = Some(fault);
                            break;
                        }
                    }
                }
            }
            Err(_) => {
                operational_failure = true;
                diagnostics.push(operand_diagnostic(
                    source.command_name,
                    0,
                    source.paths[0],
                    "input read failed while following",
                ));
            }
        }
    }

    if stage_fault.is_none() && !cancellation.is_cancelled() {
        let cause = if operational_failure {
            OrderedFinishCauseV1::SourceFailed
        } else {
            OrderedFinishCauseV1::UpstreamStopped
        };
        if let Err(fault) = pipeline
            .finish(cause)
            .and_then(|()| pipeline.flush_output())
        {
            stage_fault = Some(fault);
        }
    }
    let cancelled = cancellation.is_cancelled()
        || matches!(stage_fault, Some(OrderedPipelineFaultV1::Cancelled));
    drop(pipeline);
    sink.finish().map_err(map_sink_error)?;
    if cancelled {
        return Ok(130);
    }
    if let Some(fault) = stage_fault {
        match fault {
            OrderedPipelineFaultV1::TailResource => {
                operational_failure = true;
                diagnostics.push("wingman tail: buffer resource limit exceeded".to_string());
            }
            OrderedPipelineFaultV1::SortResource => {
                operational_failure = true;
                diagnostics
                    .push("wingman sort: materialization resource limit exceeded".to_string());
            }
            OrderedPipelineFaultV1::InvalidNumeric => {
                operational_failure = true;
                diagnostics.push("wingman sort: invalid numeric data".to_string());
            }
            OrderedPipelineFaultV1::Output { kind } => {
                return Err(ReadonlyExecutionErrorV1::Output { kind });
            }
            OrderedPipelineFaultV1::Overflow => {
                return Err(ReadonlyExecutionErrorV1::Output {
                    kind: io::ErrorKind::OutOfMemory,
                });
            }
            OrderedPipelineFaultV1::Unsupported | OrderedPipelineFaultV1::Cancelled => {
                return Err(ReadonlyExecutionErrorV1::UnsupportedPlan);
            }
        }
    }
    for diagnostic in diagnostics {
        write_diagnostic(stderr, &diagnostic)?;
    }
    Ok(if operational_failure { 1 } else { 0 })
}

fn push_follow_record<W: Write>(
    pipeline: &mut OrderedPipelineV1<'_, W>,
    record: RecordFrameV1,
) -> Result<OrderedFlowV1, OrderedPipelineFaultV1> {
    let flow = pipeline.push(record, 0)?;
    pipeline.flush_output()?;
    Ok(flow)
}

fn path_resolution_exit(error: PathResolutionErrorV1) -> u8 {
    match error {
        PathResolutionErrorV1::InvalidCurrentDirectory => 1,
        PathResolutionErrorV1::InvalidSpec
        | PathResolutionErrorV1::TraversalAboveRoot
        | PathResolutionErrorV1::TooLong => 2,
    }
}

fn readonly_source(
    plan: &ExecutionPlanV1,
) -> Result<ReadonlySourceV1<'_>, ReadonlyExecutionErrorV1> {
    let Some(first) = plan.stages.first() else {
        return Err(ReadonlyExecutionErrorV1::UnsupportedPlan);
    };
    let (command_name, paths, number_lines, follow_count) = match first {
        StagePlanV1::ReadTextFiles {
            paths,
            number_lines,
        } => ("cat", paths.iter().collect::<Vec<_>>(), *number_lines, None),
        StagePlanV1::HeadLines {
            path: Some(path), ..
        } => ("head", vec![path], false, None),
        StagePlanV1::TailLines {
            path: Some(path), ..
        } => ("tail", vec![path], false, None),
        StagePlanV1::FollowFile { count, path } => ("tail", vec![path], false, Some(*count)),
        StagePlanV1::CountLines { path: Some(path) } => ("wc", vec![path], false, None),
        StagePlanV1::SearchText {
            paths, recursive, ..
        } if !*recursive && !paths.is_empty() => ("grep", paths.iter().collect(), false, None),
        StagePlanV1::UniqueLines {
            path: Some(path), ..
        } => ("uniq", vec![path], false, None),
        StagePlanV1::SortLines {
            path: Some(path), ..
        } => ("sort", vec![path], false, None),
        _ => return Err(ReadonlyExecutionErrorV1::UnsupportedPlan),
    };

    for stage in plan.stages.iter().skip(1) {
        match stage {
            StagePlanV1::SearchText {
                paths, recursive, ..
            } if paths.is_empty() && !*recursive => {}
            StagePlanV1::HeadLines { path: None, .. }
            | StagePlanV1::TailLines { path: None, .. }
            | StagePlanV1::CountLines { path: None }
            | StagePlanV1::SortLines { path: None, .. }
            | StagePlanV1::UniqueLines { path: None, .. } => {}
            _ => return Err(ReadonlyExecutionErrorV1::UnsupportedPlan),
        }
    }

    Ok(ReadonlySourceV1 {
        command_name,
        paths,
        number_lines,
        grep_is_final: matches!(plan.stages.last(), Some(StagePlanV1::SearchText { .. })),
        grep_direct: matches!(first, StagePlanV1::SearchText { .. }),
        follow_count,
    })
}

struct StreamResultV1 {
    had_operational_failure: bool,
    diagnostics: Vec<String>,
    cancelled: bool,
    grep_matched: bool,
}

fn stream_inputs_ordered<W: Write>(
    inputs: &mut [PreparedInputV1],
    plan: &ExecutionPlanV1,
    source: &ReadonlySourceV1<'_>,
    sink: &mut RecordStreamWriterV1<W>,
    cancellation: &RunnerCancellationV1,
) -> Result<StreamResultV1, ReadonlyExecutionErrorV1> {
    let mut pipeline = OrderedPipelineV1::new(plan, sink, cancellation, &source.paths)
        .map_err(map_ordered_setup_fault)?;
    let mut next_line_number = 1u64;
    let mut pending: Option<RecordFrameV1> = None;
    let mut had_operational_failure = false;
    let mut diagnostics = Vec::new();
    let mut stopped = pipeline.starts_stopped();
    let mut stage_fault = None;

    'inputs: for (index, input) in inputs.iter_mut().enumerate() {
        if stopped || cancellation.is_cancelled() {
            break;
        }
        pipeline.start_input();
        let mut reader = Utf8RecordReaderV1::new(input.file_mut());
        loop {
            if cancellation.is_cancelled() {
                break 'inputs;
            }
            let frame = match reader.next_record() {
                Ok(Some(frame)) => frame,
                Ok(None) => break,
                Err(error) => {
                    if cancellation.is_cancelled() {
                        break 'inputs;
                    }
                    had_operational_failure = true;
                    pending = None;
                    let message = match error {
                        TextReadErrorV1::Decode(_) => "input is not valid bounded UTF-8 text",
                        TextReadErrorV1::Io { .. } => "input read failed",
                    };
                    diagnostics.push(operand_diagnostic(
                        source.command_name,
                        index,
                        source.paths[index],
                        message,
                    ));
                    break;
                }
            };

            let candidate = if let Some(mut previous) = pending.take() {
                let combined_length = previous.text.len().saturating_add(frame.text.len());
                if combined_length > MAX_RECORD_BYTES {
                    had_operational_failure = true;
                    diagnostics.push(operand_diagnostic(
                        source.command_name,
                        index,
                        source.paths[index],
                        "joined record exceeds the text limit",
                    ));
                    break;
                }
                previous.text.push_str(&frame.text);
                previous.terminated = frame.terminated;
                previous
            } else {
                frame
            };

            if candidate.terminated {
                let candidate =
                    number_record(candidate, source.number_lines, &mut next_line_number)?;
                match pipeline.push(candidate, index) {
                    Ok(OrderedFlowV1::Continue) => {}
                    Ok(OrderedFlowV1::StopUpstream) => {
                        stopped = true;
                        break 'inputs;
                    }
                    Err(fault) => {
                        stage_fault = Some(fault);
                        break 'inputs;
                    }
                }
            } else {
                pending = Some(candidate);
            }
        }

        if source.grep_direct {
            if let Some(frame) = pending.take() {
                let frame = number_record(frame, source.number_lines, &mut next_line_number)?;
                match pipeline.push(frame, index) {
                    Ok(OrderedFlowV1::Continue) => {}
                    Ok(OrderedFlowV1::StopUpstream) => {
                        stopped = true;
                        break 'inputs;
                    }
                    Err(fault) => {
                        stage_fault = Some(fault);
                        break 'inputs;
                    }
                }
            }
        }
    }

    if !stopped && stage_fault.is_none() && !cancellation.is_cancelled() {
        if let Some(frame) = pending.take() {
            let frame = number_record(frame, source.number_lines, &mut next_line_number)?;
            if let Err(fault) = pipeline.push(frame, inputs.len().saturating_sub(1)) {
                stage_fault = Some(fault);
            }
        }
    }

    if stage_fault.is_none() && !cancellation.is_cancelled() {
        let finish_cause = if had_operational_failure {
            OrderedFinishCauseV1::SourceFailed
        } else if stopped {
            OrderedFinishCauseV1::UpstreamStopped
        } else {
            OrderedFinishCauseV1::Complete
        };
        if let Err(fault) = pipeline.finish(finish_cause) {
            stage_fault = Some(fault);
        }
    }

    if let Some(fault) = stage_fault {
        match fault {
            OrderedPipelineFaultV1::Unsupported => {
                return Err(ReadonlyExecutionErrorV1::UnsupportedPlan);
            }
            OrderedPipelineFaultV1::Output { kind } => {
                return Err(ReadonlyExecutionErrorV1::Output { kind });
            }
            OrderedPipelineFaultV1::Cancelled => {}
            OrderedPipelineFaultV1::TailResource => {
                had_operational_failure = true;
                diagnostics.push("wingman tail: buffer resource limit exceeded".to_string());
            }
            OrderedPipelineFaultV1::SortResource => {
                had_operational_failure = true;
                diagnostics
                    .push("wingman sort: materialization resource limit exceeded".to_string());
            }
            OrderedPipelineFaultV1::InvalidNumeric => {
                had_operational_failure = true;
                diagnostics.push("wingman sort: invalid numeric data".to_string());
            }
            OrderedPipelineFaultV1::Overflow => {
                return Err(ReadonlyExecutionErrorV1::Output {
                    kind: io::ErrorKind::OutOfMemory,
                });
            }
        }
    }

    Ok(StreamResultV1 {
        had_operational_failure,
        diagnostics,
        cancelled: cancellation.is_cancelled()
            || matches!(stage_fault, Some(OrderedPipelineFaultV1::Cancelled)),
        grep_matched: pipeline.final_search_matched().unwrap_or(false),
    })
}

fn map_ordered_setup_fault(fault: OrderedPipelineFaultV1) -> ReadonlyExecutionErrorV1 {
    match fault {
        OrderedPipelineFaultV1::Output { kind } => ReadonlyExecutionErrorV1::Output { kind },
        _ => ReadonlyExecutionErrorV1::UnsupportedPlan,
    }
}

fn number_record(
    mut frame: RecordFrameV1,
    number_lines: bool,
    next_line_number: &mut u64,
) -> Result<RecordFrameV1, ReadonlyExecutionErrorV1> {
    if number_lines {
        frame.text = format!("{:>6}\t{}", *next_line_number, frame.text);
        *next_line_number =
            next_line_number
                .checked_add(1)
                .ok_or(ReadonlyExecutionErrorV1::Output {
                    kind: io::ErrorKind::OutOfMemory,
                })?;
    }
    Ok(frame)
}

fn operand_diagnostic(
    command_name: &str,
    index: usize,
    path: &ValidatedPathSpecV1,
    message: &str,
) -> String {
    let with_path = format!("wingman {command_name}: '{}': {message}", path.original);
    if with_path.len() <= MAX_PREPARED_DIAGNOSTIC_BYTES {
        with_path
    } else {
        format!("wingman {command_name}: input #{}: {message}", index + 1)
    }
}

fn write_diagnostic(
    stderr: &mut impl Write,
    diagnostic: &str,
) -> Result<(), ReadonlyExecutionErrorV1> {
    stderr
        .write_all(diagnostic.as_bytes())
        .and_then(|()| stderr.write_all(b"\r\n"))
        .and_then(|()| stderr.flush())
        .map_err(|error| ReadonlyExecutionErrorV1::Output { kind: error.kind() })
}

fn map_sink_error(error: TextStreamWriteErrorV1) -> ReadonlyExecutionErrorV1 {
    match error {
        TextStreamWriteErrorV1::Encode(_) => ReadonlyExecutionErrorV1::Output {
            kind: io::ErrorKind::InvalidData,
        },
        TextStreamWriteErrorV1::Io { kind } => ReadonlyExecutionErrorV1::Output { kind },
    }
}
