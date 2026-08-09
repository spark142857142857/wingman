use crate::grep_pattern::GrepPatternV1;
use crate::interpreter::{
    ExecutionPlanV1, RedirectModeV1 as PlanRedirectModeV1, StagePlanV1,
    MAX_PREPARED_DIAGNOSTIC_BYTES,
};
use crate::ordered_pipeline::{OrderedFlowV1, OrderedPipelineFaultV1, OrderedPipelineV1};
use crate::runner_cancel::RunnerCancellationV1;
use crate::runner_io::{
    prepare_file_io, IoPreparationErrorV1, PreparedInputV1, RedirectModeV1, RedirectSpecV1,
};
use crate::text_stream::{
    RecordFrameV1, RecordStreamWriterV1, TextReadErrorV1, TextStreamWriteErrorV1,
    Utf8RecordReaderV1, MAX_RECORD_BYTES,
};
use crate::windows_path::{resolve_path_spec, PathResolutionErrorV1, ValidatedPathSpecV1};
use std::cmp::Ordering;
use std::collections::{HashSet, VecDeque};
use std::io::{self, Write};

pub const MAX_TAIL_BUFFER_RECORDS: usize = 65_536;
pub const MAX_TAIL_BUFFER_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_SORT_RECORDS: usize = 262_144;
pub const MAX_SORT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadonlyExecutionErrorV1 {
    UnsupportedPlan,
    Output { kind: io::ErrorKind },
}

struct ReadonlySourceV1<'a> {
    command_name: &'static str,
    paths: Vec<&'a ValidatedPathSpecV1>,
    number_lines: bool,
    record_limit: Option<usize>,
    tail_limit: Option<usize>,
    count_lines: bool,
    grep: Option<GrepFilterV1>,
    grep_is_final: bool,
    grep_direct: bool,
    grep_after_sort: bool,
    sort: Option<SortFilterV1>,
    uniq: Option<UniqFilterV1>,
    uniq_before_sort: bool,
    uniq_after_sort: bool,
    head_before_sort: bool,
}

struct GrepFilterV1 {
    pattern: GrepPatternV1,
    line_numbers: bool,
    invert_match: bool,
}

#[derive(Clone, Copy)]
struct SortFilterV1 {
    reverse: bool,
    numeric: bool,
    unique: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExactDecimalV1 {
    negative: bool,
    digits: Vec<u8>,
    scale: usize,
}

#[derive(Clone, Copy)]
struct UniqFilterV1 {
    count: bool,
    repeated_only: bool,
    unique_only: bool,
}

struct UniqGroupV1 {
    frame: RecordFrameV1,
    occurrences: u64,
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
        Some(output) => execute_stream_to(inputs, plan, &source, output, stderr, cancellation),
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
    let stream = if requires_ordered_execution(plan) {
        stream_inputs_ordered(inputs, plan, source, &mut sink, cancellation)?
    } else {
        stream_inputs(inputs, source, &mut sink, cancellation)?
    };
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

fn requires_ordered_execution(plan: &ExecutionPlanV1) -> bool {
    let sort_index = plan
        .stages
        .iter()
        .position(|stage| matches!(stage, StagePlanV1::SortLines { .. }));
    sort_index.is_some_and(|sort_index| {
        plan.stages[..sort_index].iter().any(|stage| {
            matches!(
                stage,
                StagePlanV1::HeadLines { .. } | StagePlanV1::TailLines { .. }
            )
        })
    })
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
    let (
        command_name,
        paths,
        number_lines,
        mut record_limit,
        mut tail_limit,
        mut count_lines,
        mut sort,
        mut uniq,
    ) = match first {
        StagePlanV1::ReadTextFiles {
            paths,
            number_lines,
        } => (
            "cat",
            paths.iter().collect::<Vec<_>>(),
            *number_lines,
            None,
            None,
            false,
            None,
            None,
        ),
        StagePlanV1::HeadLines {
            count,
            path: Some(path),
        } => (
            "head",
            vec![path],
            false,
            Some(*count),
            None,
            false,
            None,
            None,
        ),
        StagePlanV1::TailLines {
            count,
            path: Some(path),
        } => (
            "tail",
            vec![path],
            false,
            None,
            Some(*count),
            false,
            None,
            None,
        ),
        StagePlanV1::CountLines { path: Some(path) } => {
            ("wc", vec![path], false, None, None, true, None, None)
        }
        StagePlanV1::SearchText {
            paths, recursive, ..
        } if !*recursive && !paths.is_empty() => (
            "grep",
            paths.iter().collect(),
            false,
            None,
            None,
            false,
            None,
            None,
        ),
        StagePlanV1::UniqueLines {
            path: Some(path),
            count,
            repeated_only,
            unique_only,
        } => {
            let filter = UniqFilterV1 {
                count: *count,
                repeated_only: *repeated_only,
                unique_only: *unique_only,
            };
            (
                "uniq",
                vec![path],
                false,
                None,
                None,
                false,
                None,
                Some(filter),
            )
        }
        StagePlanV1::SortLines {
            path: Some(path),
            reverse,
            numeric,
            unique,
        } => {
            let filter = SortFilterV1 {
                reverse: *reverse,
                numeric: *numeric,
                unique: *unique,
            };
            (
                "sort",
                vec![path],
                false,
                None,
                None,
                false,
                Some(filter),
                None,
            )
        }
        _ => return Err(ReadonlyExecutionErrorV1::UnsupportedPlan),
    };
    let mut grep = grep_filter(first)?;
    let mut grep_stage_index = grep.as_ref().map(|_| 0usize);
    let mut sort_stage_index = sort.as_ref().map(|_| 0usize);
    let mut uniq_stage_index = uniq.as_ref().map(|_| 0usize);
    let mut head_stage_index = matches!(first, StagePlanV1::HeadLines { .. }).then_some(0usize);
    let grep_direct = grep.is_some();
    for (index, stage) in plan.stages.iter().enumerate().skip(1) {
        match stage {
            StagePlanV1::SearchText { paths, .. }
                if paths.is_empty()
                    && grep.is_none()
                    && record_limit.is_none()
                    && tail_limit.is_none()
                    && !count_lines =>
            {
                grep = grep_filter(stage)?;
                grep_stage_index = Some(index);
            }
            StagePlanV1::SortLines {
                path: None,
                reverse,
                numeric,
                unique,
            } if sort.is_none() && !count_lines => {
                sort = Some(SortFilterV1 {
                    reverse: *reverse,
                    numeric: *numeric,
                    unique: *unique,
                });
                sort_stage_index = Some(index);
            }
            StagePlanV1::UniqueLines {
                path: None,
                count,
                repeated_only,
                unique_only,
            } if uniq.is_none()
                && record_limit.is_none()
                && tail_limit.is_none()
                && !count_lines =>
            {
                uniq = Some(UniqFilterV1 {
                    count: *count,
                    repeated_only: *repeated_only,
                    unique_only: *unique_only,
                });
                uniq_stage_index = Some(index);
            }
            StagePlanV1::HeadLines { count, path: None }
                if tail_limit.is_none() && !count_lines =>
            {
                record_limit = Some(record_limit.map_or(*count, |current| current.min(*count)));
                head_stage_index.get_or_insert(index);
            }
            StagePlanV1::TailLines { count, path: None }
                if tail_limit.is_none() && !count_lines =>
            {
                tail_limit = Some(*count);
            }
            StagePlanV1::CountLines { path: None }
                if !count_lines && index + 1 == plan.stages.len() =>
            {
                count_lines = true;
            }
            _ => return Err(ReadonlyExecutionErrorV1::UnsupportedPlan),
        }
    }
    let uniq_before_sort = uniq_stage_index
        .zip(sort_stage_index)
        .is_some_and(|(uniq, sort)| uniq < sort);
    let uniq_after_sort = uniq.is_some() && !uniq_before_sort;
    let head_before_sort = head_stage_index
        .zip(sort_stage_index)
        .is_some_and(|(head, sort)| head < sort);
    Ok(ReadonlySourceV1 {
        command_name,
        paths,
        number_lines,
        record_limit,
        tail_limit,
        count_lines,
        grep,
        grep_is_final: grep_stage_index.is_some_and(|index| index + 1 == plan.stages.len()),
        grep_direct,
        grep_after_sort: grep_stage_index
            .zip(sort_stage_index)
            .is_some_and(|(grep, sort)| grep > sort),
        sort,
        uniq,
        uniq_before_sort,
        uniq_after_sort,
        head_before_sort,
    })
}

fn grep_filter(stage: &StagePlanV1) -> Result<Option<GrepFilterV1>, ReadonlyExecutionErrorV1> {
    let StagePlanV1::SearchText {
        pattern,
        ignore_case,
        line_numbers,
        invert_match,
        fixed_strings,
        recursive,
        ..
    } = stage
    else {
        return Ok(None);
    };
    if *recursive {
        return Err(ReadonlyExecutionErrorV1::UnsupportedPlan);
    }
    let pattern = GrepPatternV1::compile(pattern, *fixed_strings, *ignore_case)
        .map_err(|_| ReadonlyExecutionErrorV1::UnsupportedPlan)?;
    Ok(Some(GrepFilterV1 {
        pattern,
        line_numbers: *line_numbers,
        invert_match: *invert_match,
    }))
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
        if let Err(fault) = pipeline.finish(had_operational_failure) {
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

fn stream_inputs<W: Write>(
    inputs: &mut [PreparedInputV1],
    source: &ReadonlySourceV1<'_>,
    sink: &mut RecordStreamWriterV1<W>,
    cancellation: &RunnerCancellationV1,
) -> Result<StreamResultV1, ReadonlyExecutionErrorV1> {
    let mut emitted = 0usize;
    let mut terminated_count = 0u64;
    let mut next_line_number = 1u64;
    let mut pending: Option<RecordFrameV1> = None;
    let mut tail_records = VecDeque::new();
    let mut tail_bytes = 0usize;
    let mut had_operational_failure = false;
    let mut diagnostics = Vec::new();
    let mut tail_resource_failure = false;
    let mut grep_line_number = 1u64;
    let mut grep_matched = false;
    let mut grep_pending_output: Option<RecordFrameV1> = None;
    let mut sort_records = Vec::new();
    let mut sort_bytes = 0usize;
    let mut uniq_pending: Option<UniqGroupV1> = None;
    let mut pre_sort_head_emitted = 0usize;

    if source.record_limit == Some(0) || source.tail_limit == Some(0) {
        if source.count_lines {
            emit_count_record(terminated_count, sink)?;
        }
        return Ok(StreamResultV1 {
            had_operational_failure: false,
            diagnostics,
            cancelled: cancellation.is_cancelled(),
            grep_matched,
        });
    }

    'inputs: for (index, input) in inputs.iter_mut().enumerate() {
        if source.grep_direct {
            grep_line_number = 1;
        }
        if cancellation.is_cancelled() {
            return Ok(StreamResultV1 {
                had_operational_failure,
                diagnostics,
                cancelled: true,
                grep_matched,
            });
        }
        let mut reader = Utf8RecordReaderV1::new(input.file_mut());
        loop {
            if cancellation.is_cancelled() {
                return Ok(StreamResultV1 {
                    had_operational_failure,
                    diagnostics,
                    cancelled: true,
                    grep_matched,
                });
            }
            let frame = match reader.next_record() {
                Ok(Some(frame)) => frame,
                Ok(None) => break,
                Err(error) => {
                    if cancellation.is_cancelled() {
                        return Ok(StreamResultV1 {
                            had_operational_failure,
                            diagnostics,
                            cancelled: true,
                            grep_matched,
                        });
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

            if cancellation.is_cancelled() {
                return Ok(StreamResultV1 {
                    had_operational_failure,
                    diagnostics,
                    cancelled: true,
                    grep_matched,
                });
            }
            if candidate.terminated {
                let candidate =
                    number_record(candidate, source.number_lines, &mut next_line_number)?;
                let Some(candidate) = select_input_grep_record(
                    candidate,
                    source,
                    index,
                    &mut grep_line_number,
                    &mut grep_matched,
                )?
                else {
                    continue;
                };
                if let Some(mut previous) = grep_pending_output.take() {
                    previous.terminated = true;
                    match process_after_sort(
                        previous,
                        source,
                        &mut sort_records,
                        &mut sort_bytes,
                        &mut uniq_pending,
                        &mut pre_sort_head_emitted,
                        &mut tail_records,
                        &mut tail_bytes,
                        &mut terminated_count,
                        &mut emitted,
                        sink,
                    )? {
                        SelectedRecordResultV1::Continue => {}
                        SelectedRecordResultV1::Stop => break 'inputs,
                        SelectedRecordResultV1::ResourceFailure => {
                            tail_resource_failure = true;
                            had_operational_failure = true;
                            diagnostics
                                .push("wingman tail: buffer resource limit exceeded".to_string());
                            break 'inputs;
                        }
                        SelectedRecordResultV1::SortResourceFailure => {
                            had_operational_failure = true;
                            diagnostics.push(
                                "wingman sort: materialization resource limit exceeded".to_string(),
                            );
                            break 'inputs;
                        }
                    }
                }
                match process_after_sort(
                    candidate,
                    source,
                    &mut sort_records,
                    &mut sort_bytes,
                    &mut uniq_pending,
                    &mut pre_sort_head_emitted,
                    &mut tail_records,
                    &mut tail_bytes,
                    &mut terminated_count,
                    &mut emitted,
                    sink,
                )? {
                    SelectedRecordResultV1::Continue => {}
                    SelectedRecordResultV1::Stop => break 'inputs,
                    SelectedRecordResultV1::ResourceFailure => {
                        tail_resource_failure = true;
                        had_operational_failure = true;
                        diagnostics
                            .push("wingman tail: buffer resource limit exceeded".to_string());
                        break 'inputs;
                    }
                    SelectedRecordResultV1::SortResourceFailure => {
                        had_operational_failure = true;
                        diagnostics.push(
                            "wingman sort: materialization resource limit exceeded".to_string(),
                        );
                        break 'inputs;
                    }
                }
                if cancellation.is_cancelled() {
                    return Ok(StreamResultV1 {
                        had_operational_failure,
                        diagnostics,
                        cancelled: true,
                        grep_matched,
                    });
                }
            } else {
                pending = Some(candidate);
            }
        }

        if source.grep_direct {
            if let Some(pending_record) = pending.take() {
                let pending_record =
                    number_record(pending_record, source.number_lines, &mut next_line_number)?;
                if let Some(selected) = select_input_grep_record(
                    pending_record,
                    source,
                    index,
                    &mut grep_line_number,
                    &mut grep_matched,
                )? {
                    if let Some(mut previous) = grep_pending_output.replace(selected) {
                        previous.terminated = true;
                        match process_after_sort(
                            previous,
                            source,
                            &mut sort_records,
                            &mut sort_bytes,
                            &mut uniq_pending,
                            &mut pre_sort_head_emitted,
                            &mut tail_records,
                            &mut tail_bytes,
                            &mut terminated_count,
                            &mut emitted,
                            sink,
                        )? {
                            SelectedRecordResultV1::Continue => {}
                            SelectedRecordResultV1::Stop => {
                                grep_pending_output = None;
                                break 'inputs;
                            }
                            SelectedRecordResultV1::ResourceFailure => {
                                grep_pending_output = None;
                                tail_resource_failure = true;
                                had_operational_failure = true;
                                diagnostics.push(
                                    "wingman tail: buffer resource limit exceeded".to_string(),
                                );
                                break 'inputs;
                            }
                            SelectedRecordResultV1::SortResourceFailure => {
                                grep_pending_output = None;
                                had_operational_failure = true;
                                diagnostics.push(
                                    "wingman sort: materialization resource limit exceeded"
                                        .to_string(),
                                );
                                break 'inputs;
                            }
                        }
                    }
                }
            }
        }
    }

    if source.record_limit.is_none_or(|limit| emitted < limit) {
        if let Some(pending) = pending {
            if cancellation.is_cancelled() {
                return Ok(StreamResultV1 {
                    had_operational_failure,
                    diagnostics,
                    cancelled: true,
                    grep_matched,
                });
            }
            let pending = number_record(pending, source.number_lines, &mut next_line_number)?;
            if let Some(pending) = select_input_grep_record(
                pending,
                source,
                inputs.len().saturating_sub(1),
                &mut grep_line_number,
                &mut grep_matched,
            )? {
                grep_pending_output = Some(pending);
            }
            if cancellation.is_cancelled() {
                return Ok(StreamResultV1 {
                    had_operational_failure,
                    diagnostics,
                    cancelled: true,
                    grep_matched,
                });
            }
        }
    }

    if let Some(pending) = grep_pending_output {
        match process_after_sort(
            pending,
            source,
            &mut sort_records,
            &mut sort_bytes,
            &mut uniq_pending,
            &mut pre_sort_head_emitted,
            &mut tail_records,
            &mut tail_bytes,
            &mut terminated_count,
            &mut emitted,
            sink,
        )? {
            SelectedRecordResultV1::Continue | SelectedRecordResultV1::Stop => {}
            SelectedRecordResultV1::ResourceFailure => {
                tail_resource_failure = true;
                had_operational_failure = true;
                diagnostics.push("wingman tail: buffer resource limit exceeded".to_string());
            }
            SelectedRecordResultV1::SortResourceFailure => {
                had_operational_failure = true;
                diagnostics
                    .push("wingman sort: materialization resource limit exceeded".to_string());
            }
        }
    }

    if source.sort.is_some() {
        if source.uniq_before_sort && had_operational_failure {
            uniq_pending = None;
        }
        if source.uniq_before_sort && !had_operational_failure {
            if let Some(group) = uniq_pending.take() {
                if let Some(frame) = prepare_unique_group(group, source, false)? {
                    if collect_sort_record(frame, &mut sort_records, &mut sort_bytes)
                        == SelectedRecordResultV1::SortResourceFailure
                    {
                        had_operational_failure = true;
                        diagnostics.push(
                            "wingman sort: materialization resource limit exceeded".to_string(),
                        );
                    }
                }
            }
        }
        if had_operational_failure {
            sort_records.clear();
        } else {
            match emit_sorted_records(
                sort_records,
                source,
                &mut uniq_pending,
                &mut tail_records,
                &mut tail_bytes,
                &mut terminated_count,
                &mut emitted,
                &mut grep_matched,
                sink,
                cancellation,
            )? {
                SortOutputResultV1::Complete => {}
                SortOutputResultV1::Cancelled => {
                    return Ok(StreamResultV1 {
                        had_operational_failure,
                        diagnostics,
                        cancelled: true,
                        grep_matched,
                    });
                }
                SortOutputResultV1::InvalidNumeric => {
                    had_operational_failure = true;
                    diagnostics.push("wingman sort: invalid numeric data".to_string());
                }
                SortOutputResultV1::TailResourceFailure => {
                    tail_resource_failure = true;
                    had_operational_failure = true;
                    diagnostics.push("wingman tail: buffer resource limit exceeded".to_string());
                }
            }
        }
    }

    if let Some(group) = uniq_pending.take() {
        match emit_unique_group(
            group,
            source,
            false,
            &mut tail_records,
            &mut tail_bytes,
            &mut terminated_count,
            &mut emitted,
            sink,
        )? {
            SelectedRecordResultV1::Continue | SelectedRecordResultV1::Stop => {}
            SelectedRecordResultV1::ResourceFailure => {
                tail_resource_failure = true;
                had_operational_failure = true;
                diagnostics.push("wingman tail: buffer resource limit exceeded".to_string());
            }
            SelectedRecordResultV1::SortResourceFailure => {
                return Err(ReadonlyExecutionErrorV1::UnsupportedPlan);
            }
        }
    }

    if source.tail_limit.is_some() && !tail_resource_failure {
        while let Some(frame) = tail_records.pop_front() {
            if cancellation.is_cancelled() {
                return Ok(StreamResultV1 {
                    had_operational_failure,
                    diagnostics,
                    cancelled: true,
                    grep_matched,
                });
            }
            if source.count_lines {
                if frame.terminated {
                    terminated_count = terminated_count.checked_add(1).ok_or(
                        ReadonlyExecutionErrorV1::Output {
                            kind: io::ErrorKind::OutOfMemory,
                        },
                    )?;
                }
            } else {
                sink.push(frame).map_err(map_sink_error)?;
            }
        }
    }

    if source.count_lines && !tail_resource_failure {
        if cancellation.is_cancelled() {
            return Ok(StreamResultV1 {
                had_operational_failure,
                diagnostics,
                cancelled: true,
                grep_matched,
            });
        }
        emit_count_record(terminated_count, sink)?;
        if cancellation.is_cancelled() {
            return Ok(StreamResultV1 {
                had_operational_failure,
                diagnostics,
                cancelled: true,
                grep_matched,
            });
        }
    }

    Ok(StreamResultV1 {
        had_operational_failure,
        diagnostics,
        cancelled: false,
        grep_matched,
    })
}

fn emit_count_record<W: Write>(
    count: u64,
    sink: &mut RecordStreamWriterV1<W>,
) -> Result<(), ReadonlyExecutionErrorV1> {
    sink.push(RecordFrameV1 {
        text: count.to_string(),
        terminated: true,
    })
    .map_err(map_sink_error)
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

fn select_grep_record(
    mut frame: RecordFrameV1,
    source: &ReadonlySourceV1<'_>,
    input_index: usize,
    line_number: &mut u64,
    matched_any: &mut bool,
) -> Result<Option<RecordFrameV1>, ReadonlyExecutionErrorV1> {
    let Some(grep) = source.grep.as_ref() else {
        return Ok(Some(frame));
    };
    let current_line = *line_number;
    *line_number = line_number
        .checked_add(1)
        .ok_or(ReadonlyExecutionErrorV1::Output {
            kind: io::ErrorKind::OutOfMemory,
        })?;
    let selected = grep.pattern.is_match(&frame.text) != grep.invert_match;
    if !selected {
        return Ok(None);
    }
    *matched_any = true;
    let path_prefix = source
        .grep_direct
        .then_some(())
        .filter(|_| source.paths.len() > 1)
        .map(|()| source.paths[input_index].original.replace('/', "\\"));
    if let Some(path) = path_prefix {
        if grep.line_numbers {
            frame.text = format!("{path}:{current_line}:{}", frame.text);
        } else {
            frame.text = format!("{path}:{}", frame.text);
        }
    } else if grep.line_numbers {
        frame.text = format!("{current_line}:{}", frame.text);
    }
    Ok(Some(frame))
}

fn select_input_grep_record(
    frame: RecordFrameV1,
    source: &ReadonlySourceV1<'_>,
    input_index: usize,
    line_number: &mut u64,
    matched_any: &mut bool,
) -> Result<Option<RecordFrameV1>, ReadonlyExecutionErrorV1> {
    if source.grep_after_sort {
        Ok(Some(frame))
    } else {
        select_grep_record(frame, source, input_index, line_number, matched_any)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SelectedRecordResultV1 {
    Continue,
    Stop,
    ResourceFailure,
    SortResourceFailure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SortOutputResultV1 {
    Complete,
    Cancelled,
    InvalidNumeric,
    TailResourceFailure,
}

#[allow(clippy::too_many_arguments)]
fn process_after_sort<W: Write>(
    frame: RecordFrameV1,
    source: &ReadonlySourceV1<'_>,
    records: &mut Vec<RecordFrameV1>,
    retained_bytes: &mut usize,
    uniq_pending: &mut Option<UniqGroupV1>,
    pre_sort_head_emitted: &mut usize,
    tail_records: &mut VecDeque<RecordFrameV1>,
    tail_bytes: &mut usize,
    terminated_count: &mut u64,
    emitted: &mut usize,
    sink: &mut RecordStreamWriterV1<W>,
) -> Result<SelectedRecordResultV1, ReadonlyExecutionErrorV1> {
    if source.sort.is_none() {
        return process_after_unique(
            frame,
            source,
            uniq_pending,
            tail_records,
            tail_bytes,
            terminated_count,
            emitted,
            sink,
        );
    }
    let stop_after_record = if source.head_before_sort {
        let limit = source
            .record_limit
            .ok_or(ReadonlyExecutionErrorV1::UnsupportedPlan)?;
        if *pre_sort_head_emitted >= limit {
            return Ok(SelectedRecordResultV1::Stop);
        }
        *pre_sort_head_emitted = pre_sort_head_emitted.saturating_add(1);
        *pre_sort_head_emitted >= limit
    } else {
        false
    };
    if source.uniq_before_sort {
        let Some(mut previous) = uniq_pending.take() else {
            *uniq_pending = Some(UniqGroupV1 {
                frame,
                occurrences: 1,
            });
            return Ok(if stop_after_record {
                SelectedRecordResultV1::Stop
            } else {
                SelectedRecordResultV1::Continue
            });
        };
        if previous.frame.text == frame.text {
            previous.occurrences =
                previous
                    .occurrences
                    .checked_add(1)
                    .ok_or(ReadonlyExecutionErrorV1::Output {
                        kind: io::ErrorKind::OutOfMemory,
                    })?;
            previous.frame.terminated = frame.terminated;
            *uniq_pending = Some(previous);
            return Ok(if stop_after_record {
                SelectedRecordResultV1::Stop
            } else {
                SelectedRecordResultV1::Continue
            });
        }
        let result = match prepare_unique_group(previous, source, true)? {
            Some(output) => collect_sort_record(output, records, retained_bytes),
            None => SelectedRecordResultV1::Continue,
        };
        if result == SelectedRecordResultV1::Continue {
            *uniq_pending = Some(UniqGroupV1 {
                frame,
                occurrences: 1,
            });
        }
        return Ok(
            if result == SelectedRecordResultV1::Continue && stop_after_record {
                SelectedRecordResultV1::Stop
            } else {
                result
            },
        );
    }
    let result = collect_sort_record(frame, records, retained_bytes);
    Ok(
        if result == SelectedRecordResultV1::Continue && stop_after_record {
            SelectedRecordResultV1::Stop
        } else {
            result
        },
    )
}

fn collect_sort_record(
    frame: RecordFrameV1,
    records: &mut Vec<RecordFrameV1>,
    retained_bytes: &mut usize,
) -> SelectedRecordResultV1 {
    let next_bytes = retained_bytes.saturating_add(frame.text.len());
    if records.len() >= MAX_SORT_RECORDS || next_bytes > MAX_SORT_BYTES {
        records.clear();
        *retained_bytes = 0;
        return SelectedRecordResultV1::SortResourceFailure;
    }
    records.push(frame);
    *retained_bytes = next_bytes;
    SelectedRecordResultV1::Continue
}

#[allow(clippy::too_many_arguments)]
fn emit_sorted_records<W: Write>(
    records: Vec<RecordFrameV1>,
    source: &ReadonlySourceV1<'_>,
    uniq_pending: &mut Option<UniqGroupV1>,
    tail_records: &mut VecDeque<RecordFrameV1>,
    tail_bytes: &mut usize,
    terminated_count: &mut u64,
    emitted: &mut usize,
    grep_matched: &mut bool,
    sink: &mut RecordStreamWriterV1<W>,
    cancellation: &RunnerCancellationV1,
) -> Result<SortOutputResultV1, ReadonlyExecutionErrorV1> {
    let Some(filter) = source.sort else {
        return Err(ReadonlyExecutionErrorV1::UnsupportedPlan);
    };
    if records.is_empty() {
        return Ok(SortOutputResultV1::Complete);
    }
    let final_terminated = records.last().is_some_and(|frame| frame.terminated);

    let mut records = if filter.numeric {
        let mut keyed = Vec::with_capacity(records.len());
        for frame in records {
            if cancellation.is_cancelled() {
                return Ok(SortOutputResultV1::Cancelled);
            }
            let Some(key) = parse_exact_decimal(&frame.text) else {
                return Ok(SortOutputResultV1::InvalidNumeric);
            };
            keyed.push((frame, key));
        }
        if filter.unique {
            keyed = retain_first_identical(keyed, |entry| entry.0.text.as_str());
        }
        keyed.sort_by(|left, right| {
            let ordering = compare_exact_decimal(&left.1, &right.1);
            if filter.reverse {
                ordering.reverse()
            } else {
                ordering
            }
        });
        keyed.into_iter().map(|(frame, _)| frame).collect()
    } else {
        let mut records = records;
        if filter.unique {
            records = retain_first_identical(records, |frame| frame.text.as_str());
        }
        records.sort_by(|left, right| {
            let ordering = left.text.cmp(&right.text);
            if filter.reverse {
                ordering.reverse()
            } else {
                ordering
            }
        });
        records
    };

    let last_index = records.len().saturating_sub(1);
    let mut grep_line_number = 1u64;
    for (index, mut frame) in records.drain(..).enumerate() {
        if cancellation.is_cancelled() {
            return Ok(SortOutputResultV1::Cancelled);
        }
        frame.terminated = index != last_index || final_terminated;
        if source.grep_after_sort {
            let Some(selected) =
                select_grep_record(frame, source, 0, &mut grep_line_number, grep_matched)?
            else {
                continue;
            };
            frame = selected;
        }
        match process_after_unique(
            frame,
            source,
            uniq_pending,
            tail_records,
            tail_bytes,
            terminated_count,
            emitted,
            sink,
        )? {
            SelectedRecordResultV1::Continue => {}
            SelectedRecordResultV1::Stop => return Ok(SortOutputResultV1::Complete),
            SelectedRecordResultV1::ResourceFailure => {
                return Ok(SortOutputResultV1::TailResourceFailure);
            }
            SelectedRecordResultV1::SortResourceFailure => {
                return Err(ReadonlyExecutionErrorV1::UnsupportedPlan);
            }
        }
    }
    Ok(SortOutputResultV1::Complete)
}

pub(crate) fn retain_first_identical<T>(records: Vec<T>, text: impl Fn(&T) -> &str) -> Vec<T> {
    let mut seen = HashSet::with_capacity(records.len());
    let retained = records
        .iter()
        .map(|record| seen.insert(text(record)))
        .collect::<Vec<_>>();
    drop(seen);
    records
        .into_iter()
        .zip(retained)
        .filter_map(|(record, keep)| keep.then_some(record))
        .collect()
}

pub(crate) fn parse_exact_decimal(text: &str) -> Option<ExactDecimalV1> {
    let trimmed = text.trim_matches([' ', '\t']);
    if trimmed.is_empty() {
        return Some(ExactDecimalV1 {
            negative: false,
            digits: vec![b'0'],
            scale: 0,
        });
    }
    let bytes = trimmed.as_bytes();
    let (negative, payload) = match bytes.first()? {
        b'+' => (false, &bytes[1..]),
        b'-' => (true, &bytes[1..]),
        _ => (false, bytes),
    };
    if payload.is_empty() {
        return None;
    }
    let mut seen_dot = false;
    let mut digit_count = 0usize;
    let mut fraction_count = 0usize;
    let mut digits = Vec::with_capacity(payload.len());
    for &byte in payload {
        match byte {
            b'.' if !seen_dot => seen_dot = true,
            b'0'..=b'9' => {
                digit_count = digit_count.saturating_add(1);
                if seen_dot {
                    fraction_count = fraction_count.saturating_add(1);
                }
                digits.push(byte);
            }
            _ => return None,
        }
    }
    if digit_count == 0 {
        return None;
    }
    let first_nonzero = digits.iter().position(|digit| *digit != b'0');
    let Some(first_nonzero) = first_nonzero else {
        return Some(ExactDecimalV1 {
            negative: false,
            digits: vec![b'0'],
            scale: 0,
        });
    };
    digits.drain(..first_nonzero);
    while fraction_count > 0 && digits.last() == Some(&b'0') {
        digits.pop();
        fraction_count -= 1;
    }
    Some(ExactDecimalV1 {
        negative,
        digits,
        scale: fraction_count,
    })
}

pub(crate) fn compare_exact_decimal(left: &ExactDecimalV1, right: &ExactDecimalV1) -> Ordering {
    if left.negative != right.negative {
        return if left.negative {
            Ordering::Less
        } else {
            Ordering::Greater
        };
    }
    let magnitude = compare_decimal_magnitude(left, right);
    if left.negative {
        magnitude.reverse()
    } else {
        magnitude
    }
}

fn compare_decimal_magnitude(left: &ExactDecimalV1, right: &ExactDecimalV1) -> Ordering {
    let left_zero = left.digits.as_slice() == b"0";
    let right_zero = right.digits.as_slice() == b"0";
    match (left_zero, right_zero) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        (false, false) => {}
    }
    let left_integer_digits = left.digits.len() as isize - left.scale as isize;
    let right_integer_digits = right.digits.len() as isize - right.scale as isize;
    match left_integer_digits.cmp(&right_integer_digits) {
        Ordering::Equal => {}
        ordering => return ordering,
    }
    let maximum_digits = left.digits.len().max(right.digits.len());
    for index in 0..maximum_digits {
        let left_digit = left.digits.get(index).copied().unwrap_or(b'0');
        let right_digit = right.digits.get(index).copied().unwrap_or(b'0');
        match left_digit.cmp(&right_digit) {
            Ordering::Equal => {}
            ordering => return ordering,
        }
    }
    Ordering::Equal
}

#[allow(clippy::too_many_arguments)]
fn process_after_unique<W: Write>(
    frame: RecordFrameV1,
    source: &ReadonlySourceV1<'_>,
    pending: &mut Option<UniqGroupV1>,
    tail_records: &mut VecDeque<RecordFrameV1>,
    tail_bytes: &mut usize,
    terminated_count: &mut u64,
    emitted: &mut usize,
    sink: &mut RecordStreamWriterV1<W>,
) -> Result<SelectedRecordResultV1, ReadonlyExecutionErrorV1> {
    if source.uniq.is_none() || !source.uniq_after_sort {
        return process_selected_record(
            frame,
            source,
            tail_records,
            tail_bytes,
            terminated_count,
            emitted,
            sink,
        );
    }

    let Some(mut previous) = pending.take() else {
        *pending = Some(UniqGroupV1 {
            frame,
            occurrences: 1,
        });
        return Ok(SelectedRecordResultV1::Continue);
    };
    if previous.frame.text == frame.text {
        previous.occurrences =
            previous
                .occurrences
                .checked_add(1)
                .ok_or(ReadonlyExecutionErrorV1::Output {
                    kind: io::ErrorKind::OutOfMemory,
                })?;
        previous.frame.terminated = frame.terminated;
        *pending = Some(previous);
        return Ok(SelectedRecordResultV1::Continue);
    }

    let result = emit_unique_group(
        previous,
        source,
        true,
        tail_records,
        tail_bytes,
        terminated_count,
        emitted,
        sink,
    )?;
    if result == SelectedRecordResultV1::Continue {
        *pending = Some(UniqGroupV1 {
            frame,
            occurrences: 1,
        });
    }
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
fn emit_unique_group<W: Write>(
    group: UniqGroupV1,
    source: &ReadonlySourceV1<'_>,
    force_terminated: bool,
    tail_records: &mut VecDeque<RecordFrameV1>,
    tail_bytes: &mut usize,
    terminated_count: &mut u64,
    emitted: &mut usize,
    sink: &mut RecordStreamWriterV1<W>,
) -> Result<SelectedRecordResultV1, ReadonlyExecutionErrorV1> {
    let Some(frame) = prepare_unique_group(group, source, force_terminated)? else {
        return Ok(SelectedRecordResultV1::Continue);
    };
    process_selected_record(
        frame,
        source,
        tail_records,
        tail_bytes,
        terminated_count,
        emitted,
        sink,
    )
}

fn prepare_unique_group(
    mut group: UniqGroupV1,
    source: &ReadonlySourceV1<'_>,
    force_terminated: bool,
) -> Result<Option<RecordFrameV1>, ReadonlyExecutionErrorV1> {
    let Some(filter) = source.uniq else {
        return Err(ReadonlyExecutionErrorV1::UnsupportedPlan);
    };
    if (filter.repeated_only && group.occurrences < 2)
        || (filter.unique_only && group.occurrences != 1)
    {
        return Ok(None);
    }
    if filter.count {
        group.frame.text = format!("{} {}", group.occurrences, group.frame.text);
    }
    if force_terminated {
        group.frame.terminated = true;
    }
    Ok(Some(group.frame))
}

fn process_selected_record<W: Write>(
    frame: RecordFrameV1,
    source: &ReadonlySourceV1<'_>,
    tail_records: &mut VecDeque<RecordFrameV1>,
    tail_bytes: &mut usize,
    terminated_count: &mut u64,
    emitted: &mut usize,
    sink: &mut RecordStreamWriterV1<W>,
) -> Result<SelectedRecordResultV1, ReadonlyExecutionErrorV1> {
    if !accept_record(
        frame,
        source,
        tail_records,
        tail_bytes,
        terminated_count,
        sink,
    )? {
        return Ok(SelectedRecordResultV1::ResourceFailure);
    }
    *emitted = emitted.saturating_add(1);
    if !source.head_before_sort && source.record_limit.is_some_and(|limit| *emitted >= limit) {
        Ok(SelectedRecordResultV1::Stop)
    } else {
        Ok(SelectedRecordResultV1::Continue)
    }
}

fn accept_record<W: Write>(
    frame: RecordFrameV1,
    source: &ReadonlySourceV1<'_>,
    tail_records: &mut VecDeque<RecordFrameV1>,
    tail_bytes: &mut usize,
    terminated_count: &mut u64,
    sink: &mut RecordStreamWriterV1<W>,
) -> Result<bool, ReadonlyExecutionErrorV1> {
    if let Some(limit) = source.tail_limit {
        while tail_records.len() >= limit {
            if let Some(discarded) = tail_records.pop_front() {
                *tail_bytes = tail_bytes.saturating_sub(discarded.text.len());
            }
        }
        let frame_bytes = frame.text.len();
        let next_bytes = tail_bytes.saturating_add(frame_bytes);
        if tail_records.len() >= MAX_TAIL_BUFFER_RECORDS || next_bytes > MAX_TAIL_BUFFER_BYTES {
            tail_records.clear();
            *tail_bytes = 0;
            return Ok(false);
        }
        tail_records.push_back(frame);
        *tail_bytes = next_bytes;
    } else if source.count_lines {
        if frame.terminated {
            *terminated_count =
                terminated_count
                    .checked_add(1)
                    .ok_or(ReadonlyExecutionErrorV1::Output {
                        kind: io::ErrorKind::OutOfMemory,
                    })?;
        }
    } else {
        sink.push(frame).map_err(map_sink_error)?;
    }
    Ok(true)
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
