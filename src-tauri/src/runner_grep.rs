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
    file_matches_identity, prepare_streaming_discovered_output, FileIdentityV1,
    IoPreparationErrorV1, PreparedStreamingOutputV1, RedirectModeV1, RedirectSpecV1,
};
use crate::runner_ls::{compare_names, names_equal_ignore_case};
use crate::runner_readonly::ReadonlyExecutionErrorV1;
use crate::text_stream::{
    RecordFrameV1, RecordStreamWriterV1, TextReadErrorV1, Utf8RecordReaderV1,
};
use crate::windows_path::{resolve_path_spec, ValidatedPathSpecV1};
use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

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

struct ResolvedRootV1 {
    path: PathBuf,
    display: String,
    read_dir: fs::ReadDir,
}

struct DirectoryFrameV1 {
    display: String,
    depth: usize,
    entries: VecDeque<fs::DirEntry>,
}

#[derive(Default)]
struct RecursiveStreamStateV1 {
    visited: usize,
    pending_output: Option<RecordFrameV1>,
    diagnostics: Vec<String>,
    matched: bool,
    stopped: bool,
    stage_fault: Option<OrderedPipelineFaultV1>,
    traversal_limit: bool,
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

    let mut roots = Vec::new();
    let mut initial_diagnostics = Vec::new();
    for (operand_index, operand) in grep.paths.iter().enumerate() {
        if cancellation.is_cancelled() {
            return Ok(130);
        }
        let root = match resolve_path_spec(operand, cwd) {
            Ok(root) => root,
            Err(_) => {
                initial_diagnostics.push(operand_diagnostic(
                    operand_index,
                    operand,
                    "path cannot be resolved safely",
                ));
                continue;
            }
        };
        let display_root = operand.original.replace('/', "\\");
        let metadata = match fs::symlink_metadata(&root) {
            Ok(metadata) => metadata,
            Err(_) => {
                initial_diagnostics.push(operand_diagnostic(
                    operand_index,
                    operand,
                    "directory cannot be inspected",
                ));
                continue;
            }
        };
        if is_reparse(&metadata) || !metadata.is_dir() {
            initial_diagnostics.push(operand_diagnostic(
                operand_index,
                operand,
                "recursive operand is not a traversable directory",
            ));
            continue;
        }
        let read_dir = match fs::read_dir(&root) {
            Ok(read_dir) => read_dir,
            Err(_) => {
                initial_diagnostics.push(operand_diagnostic(
                    operand_index,
                    operand,
                    "directory cannot be read",
                ));
                continue;
            }
        };
        roots.push(ResolvedRootV1 {
            path: root,
            display: display_root,
            read_dir,
        });
    }
    if cancellation.is_cancelled() {
        return Ok(130);
    }

    let mut redirected_output: Option<PreparedStreamingOutputV1> = None;
    let mut output_identity = None;
    if let Some(redirect) = &grep.plan.redirect {
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
        let spec = RedirectSpecV1 {
            path: path.clone(),
            mode: match redirect.mode {
                PlanRedirectModeV1::Overwrite => RedirectModeV1::Overwrite,
                PlanRedirectModeV1::Append => RedirectModeV1::Append,
            },
        };
        let mut prepared = match prepare_streaming_discovered_output(&spec) {
            Ok(output) => output,
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
            Err(IoPreparationErrorV1::Inputs(_) | IoPreparationErrorV1::SameFile { .. }) => {
                return Err(ReadonlyExecutionErrorV1::UnsupportedPlan);
            }
        };
        let existing_inside_root =
            prepared.existed() && roots.iter().any(|root| path_is_within(&path, &root.path));
        let hard_link_alias = if prepared.has_multiple_links() {
            match recursive_roots_contain_identity(&roots, prepared.identity(), cancellation) {
                Ok(found) => found,
                Err(message) => {
                    write_diagnostic(stderr, &format!("wingman grep: {message}"))?;
                    return Ok(2);
                }
            }
        } else {
            false
        };
        if cancellation.is_cancelled() {
            return Ok(130);
        }
        if existing_inside_root || hard_link_alias {
            write_diagnostic(
                stderr,
                "wingman grep: redirection target may be the same file as recursive input",
            )?;
            return Ok(2);
        }
        if let Err(error) = prepared.commit() {
            write_diagnostic(stderr, "wingman grep: redirection target cannot be opened")?;
            if error.kind() == io::ErrorKind::InvalidInput {
                return Ok(2);
            }
            return Ok(1);
        }
        output_identity = Some(prepared.identity());
        redirected_output = Some(prepared);
    }

    let writer: &mut dyn Write = match redirected_output.as_mut() {
        Some(output) => output.file_mut(),
        None => stdout,
    };
    let mut sink = RecordStreamWriterV1::new(writer);
    let mut pipeline = OrderedPipelineV1::new(grep.plan, &mut sink, cancellation, &[])
        .map_err(map_ordered_setup_fault)?;
    let mut state = RecursiveStreamStateV1 {
        diagnostics: initial_diagnostics,
        stopped: pipeline.starts_stopped(),
        ..RecursiveStreamStateV1::default()
    };
    for root in roots {
        if state.stopped || state.stage_fault.is_some() || cancellation.is_cancelled() {
            break;
        }
        if let Err(message) = walk_directory_streaming(
            root.read_dir,
            &root.display,
            &grep,
            &mut pipeline,
            &mut state,
            output_identity,
            cancellation,
        ) {
            state
                .diagnostics
                .push(discovered_diagnostic(&root.display, message));
            if message == "recursive traversal resource limit exceeded" {
                state.traversal_limit = true;
                break;
            }
        }
    }
    if !state.stopped && state.stage_fault.is_none() && !state.traversal_limit {
        if let Some(final_record) = state.pending_output.take() {
            if let Err(error) = pipeline.push(final_record, 0) {
                state.stage_fault = Some(error);
            }
        }
    }
    if state.stage_fault.is_none() && !cancellation.is_cancelled() {
        let cause = if !state.diagnostics.is_empty() {
            OrderedFinishCauseV1::SourceFailed
        } else if state.stopped {
            OrderedFinishCauseV1::UpstreamStopped
        } else {
            OrderedFinishCauseV1::Complete
        };
        if let Err(error) = pipeline.finish(cause) {
            state.stage_fault = Some(error);
        }
    }
    let downstream_search_matched = pipeline.final_search_matched();
    drop(pipeline);
    if cancellation.is_cancelled()
        || matches!(state.stage_fault, Some(OrderedPipelineFaultV1::Cancelled))
    {
        return Ok(130);
    }
    if let Some(error) = state.stage_fault {
        match error {
            OrderedPipelineFaultV1::TailResource => state
                .diagnostics
                .push("wingman tail: buffer resource limit exceeded".to_string()),
            OrderedPipelineFaultV1::SortResource => state
                .diagnostics
                .push("wingman sort: materialization resource limit exceeded".to_string()),
            OrderedPipelineFaultV1::InvalidNumeric => state
                .diagnostics
                .push("wingman sort: invalid numeric data".to_string()),
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
    for diagnostic in &state.diagnostics {
        if cancellation.is_cancelled() {
            return Ok(130);
        }
        write_diagnostic(stderr, diagnostic)?;
    }
    Ok(
        if !state.diagnostics.is_empty()
            || (grep.grep_is_final && !state.matched)
            || downstream_search_matched == Some(false)
        {
            1
        } else {
            0
        },
    )
}

fn walk_directory_streaming<W: Write>(
    read_dir: fs::ReadDir,
    display: &str,
    grep: &RecursiveGrepV1<'_>,
    pipeline: &mut OrderedPipelineV1<'_, W>,
    state: &mut RecursiveStreamStateV1,
    output_identity: Option<FileIdentityV1>,
    cancellation: &RunnerCancellationV1,
) -> Result<(), &'static str> {
    let entries = collect_entries(read_dir, display, state, cancellation)?;
    let mut stack = vec![DirectoryFrameV1 {
        display: display.to_string(),
        depth: 0,
        entries,
    }];
    while let Some(frame) = stack.last_mut() {
        if state.stopped || state.stage_fault.is_some() || cancellation.is_cancelled() {
            return Ok(());
        }
        let Some(entry) = frame.entries.pop_front() else {
            stack.pop();
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let child_display = format!("{}\\{name}", frame.display);
        let metadata = match fs::symlink_metadata(entry.path()) {
            Ok(metadata) => metadata,
            Err(_) => {
                state.diagnostics.push(discovered_diagnostic(
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
            let depth = frame.depth.saturating_add(1);
            if depth > MAX_RECURSIVE_GREP_DEPTH {
                state.diagnostics.push(discovered_diagnostic(
                    &child_display,
                    "recursive traversal resource limit exceeded",
                ));
                return Err("recursive traversal resource limit exceeded");
            }
            let read_dir = match fs::read_dir(entry.path()) {
                Ok(read_dir) => read_dir,
                Err(_) => {
                    state.diagnostics.push(discovered_diagnostic(
                        &child_display,
                        "directory cannot be read",
                    ));
                    continue;
                }
            };
            let entries = collect_entries(read_dir, &child_display, state, cancellation)?;
            stack.push(DirectoryFrameV1 {
                display: child_display,
                depth,
                entries,
            });
        } else if metadata.is_file() {
            process_discovered_file(
                DiscoveredFileV1 {
                    path: entry.path(),
                    display: child_display,
                },
                grep,
                pipeline,
                state,
                output_identity,
                cancellation,
            );
        }
    }
    Ok(())
}

fn collect_entries(
    read_dir: fs::ReadDir,
    display: &str,
    state: &mut RecursiveStreamStateV1,
    cancellation: &RunnerCancellationV1,
) -> Result<VecDeque<fs::DirEntry>, &'static str> {
    let mut entries = Vec::new();
    for entry in read_dir {
        if cancellation.is_cancelled() {
            return Ok(VecDeque::new());
        }
        state.visited = state.visited.saturating_add(1);
        if state.visited > MAX_RECURSIVE_GREP_ENTRIES {
            return Err("recursive traversal resource limit exceeded");
        }
        match entry {
            Ok(entry) => entries.push(entry),
            Err(_) => state.diagnostics.push(discovered_diagnostic(
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
    Ok(entries.into())
}

fn process_discovered_file<W: Write>(
    file: DiscoveredFileV1,
    grep: &RecursiveGrepV1<'_>,
    pipeline: &mut OrderedPipelineV1<'_, W>,
    state: &mut RecursiveStreamStateV1,
    output_identity: Option<FileIdentityV1>,
    cancellation: &RunnerCancellationV1,
) {
    if cancellation.is_cancelled() {
        return;
    }
    let opened = match File::open(&file.path) {
        Ok(opened) => opened,
        Err(_) => {
            state.diagnostics.push(discovered_diagnostic(
                &file.display,
                "input cannot be opened",
            ));
            return;
        }
    };
    let metadata = match opened.metadata() {
        Ok(metadata) => metadata,
        Err(_) => {
            state.diagnostics.push(discovered_diagnostic(
                &file.display,
                "input cannot be inspected",
            ));
            return;
        }
    };
    if is_reparse(&metadata) || !metadata.is_file() {
        return;
    }
    if let Some(identity) = output_identity {
        match file_matches_identity(&opened, identity) {
            Ok(true) => return,
            Ok(false) => {}
            Err(_) => {
                state.diagnostics.push(discovered_diagnostic(
                    &file.display,
                    "input identity cannot be inspected",
                ));
                return;
            }
        }
    }

    let mut reader = Utf8RecordReaderV1::new(opened);
    let mut line_number = 1u64;
    loop {
        if cancellation.is_cancelled() || state.stopped || state.stage_fault.is_some() {
            return;
        }
        let frame = match reader.next_record() {
            Ok(Some(frame)) => frame,
            Ok(None) => return,
            Err(error) => {
                let message = match error {
                    TextReadErrorV1::Decode(_) => "input is not valid bounded UTF-8 text",
                    TextReadErrorV1::Io { .. } => "input read failed",
                };
                state
                    .diagnostics
                    .push(discovered_diagnostic(&file.display, message));
                return;
            }
        };
        let current_line = line_number;
        line_number = line_number.saturating_add(1);
        if grep.pattern.is_match(&frame.text) == grep.invert_match {
            continue;
        }
        state.matched = true;
        let mut selected = frame;
        selected.text = if grep.line_numbers {
            format!("{}:{current_line}:{}", file.display, selected.text)
        } else {
            format!("{}:{}", file.display, selected.text)
        };
        if let Some(mut previous) = state.pending_output.take() {
            previous.terminated = true;
            if !push_recursive_record(previous, pipeline, state) {
                return;
            }
        }
        if selected.terminated {
            if !push_recursive_record(selected, pipeline, state) {
                return;
            }
        } else {
            state.pending_output = Some(selected);
        }
    }
}

fn push_recursive_record<W: Write>(
    record: RecordFrameV1,
    pipeline: &mut OrderedPipelineV1<'_, W>,
    state: &mut RecursiveStreamStateV1,
) -> bool {
    match pipeline.push(record, 0) {
        Ok(OrderedFlowV1::Continue) => true,
        Ok(OrderedFlowV1::StopUpstream) => {
            state.stopped = true;
            false
        }
        Err(error) => {
            state.stage_fault = Some(error);
            state.stopped = true;
            false
        }
    }
}

fn path_is_within(candidate: &Path, root: &Path) -> bool {
    let canonical_candidate = fs::canonicalize(candidate).ok();
    let canonical_root = fs::canonicalize(root).ok();
    let candidate = canonical_candidate.as_deref().unwrap_or(candidate);
    let root = canonical_root.as_deref().unwrap_or(root);
    let mut candidate_components = candidate.components();
    for root_component in root.components() {
        let Some(candidate_component) = candidate_components.next() else {
            return false;
        };
        if !path_component_eq(candidate_component, root_component) {
            return false;
        }
    }
    true
}

fn path_component_eq(left: Component<'_>, right: Component<'_>) -> bool {
    match (left, right) {
        (Component::RootDir, Component::RootDir) => true,
        (Component::Prefix(left), Component::Prefix(right)) => names_equal_ignore_case(
            &left.as_os_str().to_string_lossy(),
            &right.as_os_str().to_string_lossy(),
        ),
        (Component::Normal(left), Component::Normal(right)) => {
            names_equal_ignore_case(&left.to_string_lossy(), &right.to_string_lossy())
        }
        (Component::CurDir, Component::CurDir) | (Component::ParentDir, Component::ParentDir) => {
            true
        }
        _ => false,
    }
}

fn recursive_roots_contain_identity(
    roots: &[ResolvedRootV1],
    identity: FileIdentityV1,
    cancellation: &RunnerCancellationV1,
) -> Result<bool, &'static str> {
    let mut pending = roots
        .iter()
        .map(|root| (root.path.clone(), 0usize))
        .collect::<Vec<_>>();
    let mut visited = 0usize;
    while let Some((directory, depth)) = pending.pop() {
        if cancellation.is_cancelled() {
            return Ok(false);
        }
        let entries = fs::read_dir(directory)
            .map_err(|_| "cannot prove recursive inputs are disjoint from redirection target")?;
        for entry in entries {
            if cancellation.is_cancelled() {
                return Ok(false);
            }
            visited = visited.saturating_add(1);
            if visited > MAX_RECURSIVE_GREP_ENTRIES {
                return Err("cannot prove recursive inputs are disjoint from redirection target");
            }
            let entry = entry.map_err(|_| {
                "cannot prove recursive inputs are disjoint from redirection target"
            })?;
            let metadata = fs::symlink_metadata(entry.path()).map_err(|_| {
                "cannot prove recursive inputs are disjoint from redirection target"
            })?;
            if is_reparse(&metadata) {
                continue;
            }
            if metadata.is_dir() {
                let child_depth = depth.saturating_add(1);
                if child_depth > MAX_RECURSIVE_GREP_DEPTH {
                    return Err(
                        "cannot prove recursive inputs are disjoint from redirection target",
                    );
                }
                pending.push((entry.path(), child_depth));
            } else if metadata.is_file() {
                let input = File::open(entry.path()).map_err(|_| {
                    "cannot prove recursive inputs are disjoint from redirection target"
                })?;
                if file_matches_identity(&input, identity).map_err(|_| {
                    "cannot prove recursive inputs are disjoint from redirection target"
                })? {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
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
