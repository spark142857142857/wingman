use crate::interpreter::{ExecutionPlanV1, StagePlanV1};
use crate::runner_cancel::RunnerCancellationV1;
use crate::runner_io::{
    capture_file_identity, create_verified_child_file, file_matches_identity,
    open_verified_child_directory, open_verified_child_file, open_verified_root_directory,
    DirectoryAccessErrorV1, FileAccessErrorV1, FileIdentityV1,
};
use crate::runner_ls::names_equal_ignore_case;
use crate::runner_mutation::{write_diagnostic, MutationDiagnosticsV1, MutationExecutionErrorV1};
use crate::windows_path::{resolve_path_spec, PathResolutionErrorV1, ValidatedPathSpecV1};
use std::ffi::OsString;
use std::fs::{File, FileTimes};
use std::io::Write;
use std::path::{Component, Path};
use std::time::SystemTime;

enum PreparedTouchStateV1 {
    Existing {
        parent: File,
        leaf: OsString,
        identity: FileIdentityV1,
    },
    Missing {
        parent: File,
        leaf: OsString,
    },
    MissingParent,
    NotRegularFile,
}

struct PreparedTouchOperandV1 {
    display: String,
    resolved: String,
    state: PreparedTouchStateV1,
}

struct CreatedTouchFileV1 {
    resolved: String,
    handle: File,
}

enum TouchPreflightFailureV1 {
    KnownSafety { display: String },
    Unavailable { display: String },
}

pub(crate) fn execute_touch_to<E: Write>(
    plan: &ExecutionPlanV1,
    stderr: &mut E,
    cancellation: &RunnerCancellationV1,
) -> Option<Result<u8, MutationExecutionErrorV1>> {
    let [StagePlanV1::TouchFiles { paths }] = plan.stages.as_slice() else {
        return None;
    };
    if plan.redirect.is_some() {
        return None;
    }
    Some(execute(paths, stderr, cancellation))
}

fn execute<E: Write>(
    paths: &[ValidatedPathSpecV1],
    stderr: &mut E,
    cancellation: &RunnerCancellationV1,
) -> Result<u8, MutationExecutionErrorV1> {
    if cancellation.is_cancelled() {
        return Ok(130);
    }
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd.display().to_string(),
        Err(_) => {
            write_diagnostic(
                stderr,
                "wingman touch: current directory cannot be resolved",
            )?;
            return Ok(1);
        }
    };
    let mut prepared = Vec::with_capacity(paths.len());
    for path in paths {
        if cancellation.is_cancelled() {
            return Ok(130);
        }
        match prepare_operand(path, &cwd) {
            Ok(operand) => prepared.push(operand),
            Err(TouchPreflightFailureV1::KnownSafety { display }) => {
                write_diagnostic(
                    stderr,
                    &format!("wingman touch: {display}: reparse paths are not allowed"),
                )?;
                return Ok(2);
            }
            Err(TouchPreflightFailureV1::Unavailable { display }) => {
                write_diagnostic(
                    stderr,
                    &format!("wingman touch: {display}: path safety cannot be inspected"),
                )?;
                return Ok(1);
            }
        }
    }

    let timestamp = SystemTime::now();
    let times = FileTimes::new().set_modified(timestamp);
    let mut operational_failure = false;
    let mut diagnostics = MutationDiagnosticsV1::default();
    let mut created_files = Vec::<CreatedTouchFileV1>::new();
    for operand in prepared {
        if cancellation.is_cancelled() {
            return Ok(130);
        }
        let file = match operand.state {
            PreparedTouchStateV1::MissingParent => {
                operational_failure = true;
                diagnostics.operand(
                    stderr,
                    "touch",
                    &operand.display,
                    "parent directory does not exist",
                )?;
                continue;
            }
            PreparedTouchStateV1::NotRegularFile => {
                operational_failure = true;
                diagnostics.operand(
                    stderr,
                    "touch",
                    &operand.display,
                    "target is not a regular file",
                )?;
                continue;
            }
            PreparedTouchStateV1::Existing {
                parent,
                leaf,
                identity,
            } => match open_verified_child_file(&parent, &leaf) {
                Ok(file) if file_matches_identity(&file, identity).unwrap_or(false) => file,
                Ok(_)
                | Err(FileAccessErrorV1::Missing)
                | Err(FileAccessErrorV1::NotRegularFile)
                | Err(FileAccessErrorV1::ReparsePoint) => {
                    diagnostics.operand(
                        stderr,
                        "touch",
                        &operand.display,
                        "path changed before timestamp update",
                    )?;
                    return Ok(1);
                }
                Err(FileAccessErrorV1::Io { .. }) => {
                    diagnostics.operand(
                        stderr,
                        "touch",
                        &operand.display,
                        "path safety cannot be rechecked",
                    )?;
                    return Ok(1);
                }
            },
            PreparedTouchStateV1::Missing { parent, leaf } => {
                if let Some(created) = created_files
                    .iter()
                    .find(|created| names_equal_ignore_case(&created.resolved, &operand.resolved))
                {
                    match created.handle.try_clone() {
                        Ok(file) => file,
                        Err(_) => {
                            diagnostics.operand(
                                stderr,
                                "touch",
                                &operand.display,
                                "created file identity cannot be retained",
                            )?;
                            return Ok(1);
                        }
                    }
                } else {
                    match create_verified_child_file(&parent, &leaf) {
                        Ok(file) => {
                            let registry_handle = match file.try_clone() {
                                Ok(handle) => handle,
                                Err(_) => {
                                    diagnostics.operand(
                                        stderr,
                                        "touch",
                                        &operand.display,
                                        "created file identity cannot be retained",
                                    )?;
                                    return Ok(1);
                                }
                            };
                            created_files.push(CreatedTouchFileV1 {
                                resolved: operand.resolved.clone(),
                                handle: registry_handle,
                            });
                            file
                        }
                        Err(FileAccessErrorV1::Io {
                            kind: std::io::ErrorKind::AlreadyExists,
                        })
                        | Err(FileAccessErrorV1::Missing)
                        | Err(FileAccessErrorV1::NotRegularFile)
                        | Err(FileAccessErrorV1::ReparsePoint) => {
                            diagnostics.operand(
                                stderr,
                                "touch",
                                &operand.display,
                                "path changed during file creation",
                            )?;
                            return Ok(1);
                        }
                        Err(FileAccessErrorV1::Io { .. }) => {
                            operational_failure = true;
                            diagnostics.operand(
                                stderr,
                                "touch",
                                &operand.display,
                                "file creation failed",
                            )?;
                            continue;
                        }
                    }
                }
            }
        };
        if file.set_times(times).is_err() {
            operational_failure = true;
            diagnostics.operand(stderr, "touch", &operand.display, "timestamp update failed")?;
        }
    }
    Ok(if operational_failure { 1 } else { 0 })
}

fn prepare_operand(
    spec: &ValidatedPathSpecV1,
    cwd: &str,
) -> Result<PreparedTouchOperandV1, TouchPreflightFailureV1> {
    let display = spec.original.clone();
    let resolved = resolve_path_spec(spec, cwd).map_err(|error| match error {
        PathResolutionErrorV1::TraversalAboveRoot
        | PathResolutionErrorV1::TooLong
        | PathResolutionErrorV1::InvalidSpec => TouchPreflightFailureV1::KnownSafety {
            display: display.clone(),
        },
        PathResolutionErrorV1::InvalidCurrentDirectory => TouchPreflightFailureV1::Unavailable {
            display: display.clone(),
        },
    })?;
    let resolved_display = resolved.display().to_string();
    let (root, mut components) =
        split_absolute_path(&resolved).ok_or_else(|| TouchPreflightFailureV1::KnownSafety {
            display: display.clone(),
        })?;
    let Some(leaf) = components.pop() else {
        return Ok(PreparedTouchOperandV1 {
            display,
            resolved: resolved_display,
            state: PreparedTouchStateV1::NotRegularFile,
        });
    };
    let mut parent = map_directory_preflight(open_verified_root_directory(root), &display)?;
    for component in components {
        parent = match open_verified_child_directory(&parent, &component) {
            Ok(child) => child,
            Err(DirectoryAccessErrorV1::Missing) | Err(DirectoryAccessErrorV1::NotDirectory) => {
                return Ok(PreparedTouchOperandV1 {
                    display,
                    resolved: resolved_display,
                    state: PreparedTouchStateV1::MissingParent,
                });
            }
            Err(DirectoryAccessErrorV1::ReparsePoint) => {
                return Err(TouchPreflightFailureV1::KnownSafety { display });
            }
            Err(DirectoryAccessErrorV1::Io { .. }) => {
                return Err(TouchPreflightFailureV1::Unavailable { display });
            }
        };
    }
    let state = match open_verified_child_file(&parent, &leaf) {
        Ok(file) => {
            let identity =
                capture_file_identity(&file).map_err(|_| TouchPreflightFailureV1::Unavailable {
                    display: display.clone(),
                })?;
            PreparedTouchStateV1::Existing {
                parent,
                leaf,
                identity,
            }
        }
        Err(FileAccessErrorV1::Missing) => PreparedTouchStateV1::Missing { parent, leaf },
        Err(FileAccessErrorV1::NotRegularFile) => PreparedTouchStateV1::NotRegularFile,
        Err(FileAccessErrorV1::ReparsePoint) => {
            return Err(TouchPreflightFailureV1::KnownSafety { display });
        }
        Err(FileAccessErrorV1::Io { .. }) => {
            return Err(TouchPreflightFailureV1::Unavailable { display });
        }
    };
    Ok(PreparedTouchOperandV1 {
        display,
        resolved: resolved_display,
        state,
    })
}

fn map_directory_preflight(
    result: Result<File, DirectoryAccessErrorV1>,
    display: &str,
) -> Result<File, TouchPreflightFailureV1> {
    match result {
        Ok(directory) => Ok(directory),
        Err(DirectoryAccessErrorV1::ReparsePoint) => Err(TouchPreflightFailureV1::KnownSafety {
            display: display.to_string(),
        }),
        Err(DirectoryAccessErrorV1::Missing)
        | Err(DirectoryAccessErrorV1::NotDirectory)
        | Err(DirectoryAccessErrorV1::Io { .. }) => Err(TouchPreflightFailureV1::Unavailable {
            display: display.to_string(),
        }),
    }
}

fn split_absolute_path(path: &Path) -> Option<(&Path, Vec<OsString>)> {
    let root = path
        .ancestors()
        .last()
        .filter(|candidate| !candidate.as_os_str().is_empty())?;
    let relative = path.strip_prefix(root).ok()?;
    let components = relative
        .components()
        .map(|component| match component {
            Component::Normal(value) => Some(value.to_os_string()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    Some((root, components))
}
