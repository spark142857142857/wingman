use crate::find_pattern::FindPatternV1;
use crate::interpreter::{ExecutionPlanV1, FindEntryTypeV1, StagePlanV1};
use crate::runner_cancel::RunnerCancellationV1;
use crate::runner_ls::{compare_names, execute_generated_records_with_cwd_to, GeneratedSourceV1};
use crate::runner_readonly::ReadonlyExecutionErrorV1;
use crate::text_stream::RecordFrameV1;
use crate::windows_path::{resolve_path_spec, PathKindV1, ValidatedPathSpecV1};
use std::fs;
use std::io::Write;
use std::os::windows::fs::MetadataExt;
use std::path::Path;
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

pub const MAX_FIND_ENTRIES: usize = 100_000;
pub const MAX_FIND_TRAVERSAL_DEPTH: usize = 256;

struct FindOptionsV1<'a> {
    entry_type: Option<FindEntryTypeV1>,
    pattern: Option<FindPatternV1>,
    min_depth: usize,
    max_depth: Option<usize>,
    _source: &'a ValidatedPathSpecV1,
}

struct WalkStateV1 {
    records: Vec<RecordFrameV1>,
    diagnostics: Vec<String>,
    visited: usize,
}

pub fn execute_find_to<W: Write, E: Write>(
    plan: &ExecutionPlanV1,
    stdout: &mut W,
    stderr: &mut E,
    cancellation: &RunnerCancellationV1,
) -> Option<Result<u8, ReadonlyExecutionErrorV1>> {
    if !matches!(plan.stages.first(), Some(StagePlanV1::FindPaths { .. })) {
        return None;
    }
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(_) => {
            return Some(
                write_diagnostic(
                    stderr,
                    "wingman find: unable to read current working directory",
                )
                .map(|()| 1),
            )
        }
    };
    Some(execute_find_with_cwd_to(
        plan,
        &cwd,
        stdout,
        stderr,
        cancellation,
    ))
}

pub fn execute_find_with_cwd_to<W: Write, E: Write>(
    plan: &ExecutionPlanV1,
    cwd: &Path,
    stdout: &mut W,
    stderr: &mut E,
    cancellation: &RunnerCancellationV1,
) -> Result<u8, ReadonlyExecutionErrorV1> {
    let Some(StagePlanV1::FindPaths {
        path,
        entry_type,
        name_pattern,
        ignore_case,
        min_depth,
        max_depth,
    }) = plan.stages.first()
    else {
        return Err(ReadonlyExecutionErrorV1::UnsupportedPlan);
    };
    if cancellation.is_cancelled() {
        return Ok(130);
    }
    let Some(cwd) = cwd.to_str() else {
        write_diagnostic(
            stderr,
            "wingman find: current working directory is not valid Unicode",
        )?;
        return Ok(1);
    };
    let root = match resolve_path_spec(path, cwd) {
        Ok(root) => root,
        Err(_) => {
            write_diagnostic(stderr, "wingman find: path cannot be resolved safely")?;
            return Ok(2);
        }
    };
    let root_metadata = match fs::symlink_metadata(&root) {
        Ok(metadata) => metadata,
        Err(_) => {
            write_diagnostic(stderr, "wingman find: start path cannot be inspected")?;
            return Ok(1);
        }
    };
    let display = display_root(path, &root);
    let basename = display_basename(&display);
    let options = FindOptionsV1 {
        entry_type: *entry_type,
        pattern: match name_pattern {
            Some(pattern) => Some(
                FindPatternV1::compile(pattern, *ignore_case)
                    .map_err(|_| ReadonlyExecutionErrorV1::UnsupportedPlan)?,
            ),
            None => None,
        },
        min_depth: *min_depth,
        max_depth: *max_depth,
        _source: path,
    };
    let mut state = WalkStateV1 {
        records: Vec::new(),
        diagnostics: Vec::new(),
        visited: 0,
    };
    if let Err(message) = walk(
        &root,
        &display,
        &basename,
        root_metadata,
        0,
        &options,
        &mut state,
        cancellation,
    ) {
        if cancellation.is_cancelled() {
            return Ok(130);
        }
        write_diagnostic(stderr, &format!("wingman find: {message}"))?;
        return Ok(1);
    }
    if cancellation.is_cancelled() {
        return Ok(130);
    }
    let source_failed = !state.diagnostics.is_empty();
    let exit = execute_generated_records_with_cwd_to(
        plan,
        cwd,
        GeneratedSourceV1 {
            command_name: "find",
            records: state.records,
            failed: source_failed,
        },
        stdout,
        stderr,
        cancellation,
    )?;
    if cancellation.is_cancelled() || exit == 130 {
        return Ok(130);
    }
    for diagnostic in state.diagnostics {
        write_diagnostic(stderr, &diagnostic)?;
    }
    Ok(if source_failed { 1 } else { exit })
}

#[allow(clippy::too_many_arguments)]
fn walk(
    path: &Path,
    display: &str,
    basename: &str,
    metadata: fs::Metadata,
    depth: usize,
    options: &FindOptionsV1<'_>,
    state: &mut WalkStateV1,
    cancellation: &RunnerCancellationV1,
) -> Result<(), &'static str> {
    if cancellation.is_cancelled() {
        return Ok(());
    }
    state.visited = state.visited.saturating_add(1);
    if state.visited > MAX_FIND_ENTRIES || depth > MAX_FIND_TRAVERSAL_DEPTH {
        state.records.clear();
        return Err("recursive traversal resource limit exceeded");
    }
    let reparse = metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0;
    if depth >= options.min_depth
        && options.max_depth.is_none_or(|maximum| depth <= maximum)
        && matches_type(&metadata, reparse, options.entry_type)
        && options
            .pattern
            .as_ref()
            .is_none_or(|pattern| pattern.is_match(basename))
    {
        state.records.push(RecordFrameV1 {
            text: display.to_string(),
            terminated: true,
        });
    }
    if reparse || !metadata.is_dir() || options.max_depth.is_some_and(|maximum| depth >= maximum) {
        return Ok(());
    }
    let directory = match fs::read_dir(path) {
        Ok(directory) => directory,
        Err(_) => {
            state
                .diagnostics
                .push(diagnostic(display, "directory cannot be read"));
            return Ok(());
        }
    };
    let mut children = Vec::new();
    for child in directory {
        if cancellation.is_cancelled() {
            return Ok(());
        }
        match child {
            Ok(child) => match child.file_name().to_str() {
                Some(name) => {
                    if state.visited.saturating_add(children.len()) >= MAX_FIND_ENTRIES {
                        state.records.clear();
                        return Err("recursive traversal resource limit exceeded");
                    }
                    children.push((name.to_string(), child.path()));
                }
                None => state
                    .diagnostics
                    .push(diagnostic(display, "child filename is not valid Unicode")),
            },
            Err(_) => state
                .diagnostics
                .push(diagnostic(display, "directory entry cannot be read")),
        }
    }
    children.sort_by(|left, right| compare_names(&left.0, &right.0));
    for (name, child) in children {
        if cancellation.is_cancelled() {
            return Ok(());
        }
        let child_display = if display == "." {
            format!(r".\{name}")
        } else {
            format!(r"{display}\{name}")
        };
        match fs::symlink_metadata(&child) {
            Ok(metadata) => walk(
                &child,
                &child_display,
                &name,
                metadata,
                depth + 1,
                options,
                state,
                cancellation,
            )?,
            Err(_) => state
                .diagnostics
                .push(diagnostic(&child_display, "entry cannot be inspected")),
        }
    }
    Ok(())
}

fn matches_type(metadata: &fs::Metadata, reparse: bool, kind: Option<FindEntryTypeV1>) -> bool {
    match kind {
        None => true,
        Some(FindEntryTypeV1::File) => !reparse && metadata.is_file(),
        Some(FindEntryTypeV1::Directory) => !reparse && metadata.is_dir(),
    }
}

fn display_root(spec: &ValidatedPathSpecV1, resolved: &Path) -> String {
    if spec.kind != PathKindV1::Relative {
        return resolved.to_string_lossy().into_owned();
    }
    let mut components = Vec::new();
    for component in &spec.components {
        match component.as_str() {
            "." => {}
            ".." if components
                .last()
                .is_some_and(|value: &String| value != "..") =>
            {
                components.pop();
            }
            _ => components.push(component.clone()),
        }
    }
    if components.is_empty() {
        ".".to_string()
    } else {
        components.join("\\")
    }
}

fn display_basename(display: &str) -> String {
    display
        .rsplit('\\')
        .find(|part| !part.is_empty())
        .unwrap_or(display)
        .to_string()
}

fn diagnostic(display: &str, message: &str) -> String {
    let detailed = format!("wingman find: '{display}': {message}");
    if detailed.len() <= crate::interpreter::MAX_PREPARED_DIAGNOSTIC_BYTES {
        detailed
    } else {
        format!("wingman find: discovered entry: {message}")
    }
}

fn write_diagnostic(writer: &mut impl Write, value: &str) -> Result<(), ReadonlyExecutionErrorV1> {
    writer
        .write_all(value.as_bytes())
        .and_then(|()| writer.write_all(b"\r\n"))
        .and_then(|()| writer.flush())
        .map_err(|error| ReadonlyExecutionErrorV1::Output { kind: error.kind() })
}
