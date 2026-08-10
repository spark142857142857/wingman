use crate::grep_pattern::GrepPatternV1;
use crate::interpreter::{
    ExecutionPlanV1, RedirectModeV1 as PlanRedirectModeV1, StagePlanV1,
    MAX_PREPARED_DIAGNOSTIC_BYTES,
};
use crate::ordered_pipeline::{
    OrderedFinishCauseV1, OrderedFlowV1, OrderedPipelineFaultV1, OrderedPipelineV1,
};
use crate::runner_cancel::RunnerCancellationV1;
use crate::runner_io::{
    prepare_discovered_output, IoPreparationErrorV1, RedirectModeV1, RedirectSpecV1,
};
use crate::runner_ls::compare_names;
use crate::runner_readonly::ReadonlyExecutionErrorV1;
use crate::text_stream::{
    RecordFrameV1, RecordStreamWriterV1, TextReadErrorV1, Utf8RecordReaderV1,
};
use crate::windows_path::{resolve_path_spec, ValidatedPathSpecV1};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

pub const MAX_RECURSIVE_GREP_ENTRIES: usize = 100_000;
pub const MAX_RECURSIVE_GREP_DEPTH: usize = 256;

struct RecursiveGrepV1<'a> {
    plan: &'a ExecutionPlanV1,
    pattern: GrepPatternV1,
    paths: &'a [ValidatedPathSpecV1],
    line_numbers: bool,
    invert_match: bool,
    grep_is_final: bool,
}

struct DiscoveredFileV1 {
    path: PathBuf,
    display: String,
}

pub fn execute_recursive_grep_to<W: Write, E: Write>(
    plan: &ExecutionPlanV1,
    stdout: &mut W,
    stderr: &mut E,
    cancellation: &RunnerCancellationV1,
) -> Option<Result<u8, ReadonlyExecutionErrorV1>> {
    let recursive = match recursive_plan(plan) {
        Ok(Some(recursive)) => recursive,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    Some(execute_recursive(recursive, stdout, stderr, cancellation))
}

fn recursive_plan(
    plan: &ExecutionPlanV1,
) -> Result<Option<RecursiveGrepV1<'_>>, ReadonlyExecutionErrorV1> {
    let Some(StagePlanV1::SearchText {
        pattern,
        paths,
        ignore_case,
        line_numbers,
        invert_match,
        fixed_strings,
        recursive: true,
    }) = plan.stages.first()
    else {
        return Ok(None);
    };
    if paths.is_empty() {
        return Err(ReadonlyExecutionErrorV1::UnsupportedPlan);
    }
    let pattern = GrepPatternV1::compile(pattern, *fixed_strings, *ignore_case)
        .map_err(|_| ReadonlyExecutionErrorV1::UnsupportedPlan)?;
    Ok(Some(RecursiveGrepV1 {
        plan,
        pattern,
        paths,
        line_numbers: *line_numbers,
        invert_match: *invert_match,
        grep_is_final: plan.stages.len() == 1,
    }))
}

fn execute_recursive<W: Write, E: Write>(
    grep: RecursiveGrepV1<'_>,
    stdout: &mut W,
    stderr: &mut E,
    cancellation: &RunnerCancellationV1,
) -> Result<u8, ReadonlyExecutionErrorV1> {
    if cancellation.is_cancelled() {
        return Ok(130);
    }
    let cwd = std::env::current_dir()
        .map_err(|error| ReadonlyExecutionErrorV1::Output { kind: error.kind() })?;
    let Some(cwd) = cwd.to_str() else {
        write_diagnostic(
            stderr,
            "wingman grep: current directory is not valid Unicode",
        )?;
        return Ok(1);
    };

    let mut files = Vec::new();
    let mut diagnostics = Vec::new();
    let mut visited = 0usize;
    for (operand_index, operand) in grep.paths.iter().enumerate() {
        if cancellation.is_cancelled() {
            return Ok(130);
        }
        let root = match resolve_path_spec(operand, cwd) {
            Ok(root) => root,
            Err(_) => {
                diagnostics.push(operand_diagnostic(
                    operand_index,
                    operand,
                    "path cannot be resolved safely",
                ));
                continue;
            }
        };
        let display_root = operand.original.replace('/', "\\");
        if let Err(message) = walk_directory(
            &root,
            &display_root,
            0,
            &mut visited,
            &mut files,
            &mut diagnostics,
            cancellation,
        ) {
            diagnostics.push(operand_diagnostic(operand_index, operand, message));
            if message == "recursive traversal resource limit exceeded" {
                files.clear();
                break;
            }
        }
    }
    if cancellation.is_cancelled() {
        return Ok(130);
    }

    let mut redirected_output = if let Some(redirect) = &grep.plan.redirect {
        let path = match resolve_path_spec(&redirect.path, cwd) {
            Ok(path) => path,
            Err(_) => {
                write_diagnostic(
                    stderr,
                    "wingman grep: redirection target cannot be resolved safely",
                )?;
                return Ok(2);
            }
        };
        let input_paths = files
            .iter()
            .map(|file| file.path.clone())
            .collect::<Vec<_>>();
        let spec = RedirectSpecV1 {
            path,
            mode: match redirect.mode {
                PlanRedirectModeV1::Overwrite => RedirectModeV1::Overwrite,
                PlanRedirectModeV1::Append => RedirectModeV1::Append,
            },
        };
        match prepare_discovered_output(&input_paths, &spec) {
            Ok(output) => Some(output),
            Err(IoPreparationErrorV1::Inputs(errors)) => {
                for error in errors {
                    diagnostics.push(discovered_diagnostic(
                        &files[error.index].display,
                        "input cannot be opened",
                    ));
                }
                for diagnostic in &diagnostics {
                    write_diagnostic(stderr, diagnostic)?;
                }
                return Ok(1);
            }
            Err(IoPreparationErrorV1::Output { .. }) => {
                write_diagnostic(stderr, "wingman grep: redirection target cannot be opened")?;
                return Ok(1);
            }
            Err(IoPreparationErrorV1::OutputReparsePoint) => {
                write_diagnostic(
                    stderr,
                    "wingman grep: redirection target is or crosses a reparse point",
                )?;
                return Ok(2);
            }
            Err(IoPreparationErrorV1::SameFile { input_index }) => {
                write_diagnostic(
                    stderr,
                    &discovered_diagnostic(
                        &files[input_index].display,
                        "redirection target is the same file as recursive input",
                    ),
                )?;
                return Ok(2);
            }
        }
    } else {
        None
    };

    let writer: &mut dyn Write = match redirected_output.as_mut() {
        Some(output) => output,
        None => stdout,
    };
    let mut sink = RecordStreamWriterV1::new(writer);
    let mut pipeline = OrderedPipelineV1::new(grep.plan, &mut sink, cancellation, &[])
        .map_err(map_ordered_setup_fault)?;
    let mut pending_output: Option<RecordFrameV1> = None;
    let mut matched = false;
    let mut stopped = pipeline.starts_stopped();
    let mut stage_fault = None;
    'files: for file in files {
        if stopped {
            break;
        }
        if cancellation.is_cancelled() {
            return Ok(130);
        }
        let opened = match File::open(&file.path) {
            Ok(opened) => opened,
            Err(_) => {
                diagnostics.push(discovered_diagnostic(
                    &file.display,
                    "input cannot be opened",
                ));
                continue;
            }
        };
        let metadata = match opened.metadata() {
            Ok(metadata) => metadata,
            Err(_) => {
                diagnostics.push(discovered_diagnostic(
                    &file.display,
                    "input cannot be inspected",
                ));
                continue;
            }
        };
        if is_reparse(&metadata) || !metadata.is_file() {
            continue;
        }
        let mut reader = Utf8RecordReaderV1::new(opened);
        let mut line_number = 1u64;
        loop {
            if cancellation.is_cancelled() {
                return Ok(130);
            }
            let frame = match reader.next_record() {
                Ok(Some(frame)) => frame,
                Ok(None) => break,
                Err(error) => {
                    let message = match error {
                        TextReadErrorV1::Decode(_) => "input is not valid bounded UTF-8 text",
                        TextReadErrorV1::Io { .. } => "input read failed",
                    };
                    diagnostics.push(discovered_diagnostic(&file.display, message));
                    break;
                }
            };
            let current_line = line_number;
            line_number = line_number.saturating_add(1);
            if grep.pattern.is_match(&frame.text) == grep.invert_match {
                continue;
            }
            matched = true;
            let mut selected = frame;
            selected.text = if grep.line_numbers {
                format!("{}:{current_line}:{}", file.display, selected.text)
            } else {
                format!("{}:{}", file.display, selected.text)
            };
            if let Some(mut previous) = pending_output.take() {
                previous.terminated = true;
                match pipeline.push(previous, 0) {
                    Ok(OrderedFlowV1::Continue) => {}
                    Ok(OrderedFlowV1::StopUpstream) => {
                        stopped = true;
                        break 'files;
                    }
                    Err(error) => {
                        stage_fault = Some(error);
                        stopped = true;
                        break 'files;
                    }
                }
            }
            if selected.terminated {
                match pipeline.push(selected, 0) {
                    Ok(OrderedFlowV1::Continue) => {}
                    Ok(OrderedFlowV1::StopUpstream) => {
                        stopped = true;
                        break 'files;
                    }
                    Err(error) => {
                        stage_fault = Some(error);
                        stopped = true;
                        break 'files;
                    }
                }
            } else {
                pending_output = Some(selected);
            }
        }
    }
    if !stopped && stage_fault.is_none() {
        if let Some(final_record) = pending_output {
            if let Err(error) = pipeline.push(final_record, 0) {
                stage_fault = Some(error);
            }
        }
    }
    if stage_fault.is_none() && !cancellation.is_cancelled() {
        let cause = if !diagnostics.is_empty() {
            OrderedFinishCauseV1::SourceFailed
        } else if stopped {
            OrderedFinishCauseV1::UpstreamStopped
        } else {
            OrderedFinishCauseV1::Complete
        };
        if let Err(error) = pipeline.finish(cause) {
            stage_fault = Some(error);
        }
    }
    let downstream_search_matched = pipeline.final_search_matched();
    drop(pipeline);
    if cancellation.is_cancelled() || matches!(stage_fault, Some(OrderedPipelineFaultV1::Cancelled))
    {
        return Ok(130);
    }
    if let Some(error) = stage_fault {
        match error {
            OrderedPipelineFaultV1::TailResource => {
                diagnostics.push("wingman tail: buffer resource limit exceeded".to_string())
            }
            OrderedPipelineFaultV1::SortResource => diagnostics
                .push("wingman sort: materialization resource limit exceeded".to_string()),
            OrderedPipelineFaultV1::InvalidNumeric => {
                diagnostics.push("wingman sort: invalid numeric data".to_string())
            }
            OrderedPipelineFaultV1::Output { .. } if grep.plan.redirect.is_some() => {
                write_diagnostic(
                    stderr,
                    "wingman grep: redirection output failed and may be partial",
                )?;
                return Ok(1);
            }
            OrderedPipelineFaultV1::Output { kind } => {
                return Err(ReadonlyExecutionErrorV1::Output { kind })
            }
            OrderedPipelineFaultV1::Overflow => {
                return Err(ReadonlyExecutionErrorV1::Output {
                    kind: io::ErrorKind::OutOfMemory,
                })
            }
            OrderedPipelineFaultV1::Unsupported | OrderedPipelineFaultV1::Cancelled => {
                return Err(ReadonlyExecutionErrorV1::UnsupportedPlan)
            }
        }
    }
    if let Err(error) = sink.finish().map_err(map_sink_error) {
        if grep.plan.redirect.is_some() {
            write_diagnostic(
                stderr,
                "wingman grep: redirection output failed and may be partial",
            )?;
            return Ok(1);
        }
        return Err(error);
    }
    for diagnostic in &diagnostics {
        if cancellation.is_cancelled() {
            return Ok(130);
        }
        write_diagnostic(stderr, diagnostic)?;
    }
    Ok(
        if !diagnostics.is_empty()
            || (grep.grep_is_final && !matched)
            || downstream_search_matched == Some(false)
        {
            1
        } else {
            0
        },
    )
}

fn walk_directory(
    directory: &Path,
    display: &str,
    depth: usize,
    visited: &mut usize,
    files: &mut Vec<DiscoveredFileV1>,
    diagnostics: &mut Vec<String>,
    cancellation: &RunnerCancellationV1,
) -> Result<(), &'static str> {
    if depth > MAX_RECURSIVE_GREP_DEPTH {
        return Err("recursive traversal resource limit exceeded");
    }
    let metadata = fs::symlink_metadata(directory).map_err(|_| "directory cannot be inspected")?;
    if is_reparse(&metadata) || !metadata.is_dir() {
        return Err("recursive operand is not a traversable directory");
    }
    let read_dir = fs::read_dir(directory).map_err(|_| "directory cannot be read")?;
    let mut entries = Vec::new();
    for entry in read_dir {
        if cancellation.is_cancelled() {
            return Ok(());
        }
        *visited = visited.saturating_add(1);
        if *visited > MAX_RECURSIVE_GREP_ENTRIES {
            return Err("recursive traversal resource limit exceeded");
        }
        match entry {
            Ok(entry) => entries.push(entry),
            Err(_) => diagnostics.push(discovered_diagnostic(
                display,
                "directory entry cannot be read",
            )),
        }
    }
    entries.sort_by(|left, right| {
        let left_name = left.file_name();
        let right_name = right.file_name();
        let left = left_name.to_string_lossy();
        let right = right_name.to_string_lossy();
        compare_names(&left, &right)
    });
    for entry in entries {
        if cancellation.is_cancelled() {
            return Ok(());
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let child_display = format!("{display}\\{name}");
        let metadata = match fs::symlink_metadata(entry.path()) {
            Ok(metadata) => metadata,
            Err(_) => {
                diagnostics.push(discovered_diagnostic(
                    &child_display,
                    "entry cannot be inspected",
                ));
                continue;
            }
        };
        if is_reparse(&metadata) {
            continue;
        }
        if metadata.is_dir() {
            if let Err(message) = walk_directory(
                &entry.path(),
                &child_display,
                depth + 1,
                visited,
                files,
                diagnostics,
                cancellation,
            ) {
                diagnostics.push(discovered_diagnostic(&child_display, message));
                if message == "recursive traversal resource limit exceeded" {
                    return Err(message);
                }
            }
        } else if metadata.is_file() {
            files.push(DiscoveredFileV1 {
                path: entry.path(),
                display: child_display,
            });
        }
    }
    Ok(())
}

#[cfg(windows)]
fn is_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn operand_diagnostic(index: usize, path: &ValidatedPathSpecV1, message: &str) -> String {
    let diagnostic = format!("wingman grep: '{}': {message}", path.original);
    if diagnostic.len() <= MAX_PREPARED_DIAGNOSTIC_BYTES {
        diagnostic
    } else {
        format!("wingman grep: input #{}: {message}", index + 1)
    }
}

fn discovered_diagnostic(display: &str, message: &str) -> String {
    let diagnostic = format!("wingman grep: '{display}': {message}");
    if diagnostic.len() <= MAX_PREPARED_DIAGNOSTIC_BYTES {
        diagnostic
    } else {
        format!("wingman grep: discovered input: {message}")
    }
}

fn write_diagnostic(
    writer: &mut impl Write,
    diagnostic: &str,
) -> Result<(), ReadonlyExecutionErrorV1> {
    writer
        .write_all(diagnostic.as_bytes())
        .and_then(|()| writer.write_all(b"\r\n"))
        .and_then(|()| writer.flush())
        .map_err(|error| ReadonlyExecutionErrorV1::Output { kind: error.kind() })
}

fn map_sink_error(error: crate::text_stream::TextStreamWriteErrorV1) -> ReadonlyExecutionErrorV1 {
    match error {
        crate::text_stream::TextStreamWriteErrorV1::Encode(_) => ReadonlyExecutionErrorV1::Output {
            kind: io::ErrorKind::InvalidData,
        },
        crate::text_stream::TextStreamWriteErrorV1::Io { kind } => {
            ReadonlyExecutionErrorV1::Output { kind }
        }
    }
}

fn map_ordered_setup_fault(error: OrderedPipelineFaultV1) -> ReadonlyExecutionErrorV1 {
    match error {
        OrderedPipelineFaultV1::Output { kind } => ReadonlyExecutionErrorV1::Output { kind },
        _ => ReadonlyExecutionErrorV1::UnsupportedPlan,
    }
}
