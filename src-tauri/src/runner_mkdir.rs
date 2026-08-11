use crate::interpreter::{ExecutionPlanV1, StagePlanV1};
use crate::runner_cancel::RunnerCancellationV1;
use crate::runner_io::{
    create_verified_child_directory, open_verified_child_directory, open_verified_root_directory,
    DirectoryAccessErrorV1,
};
use crate::runner_ls::names_equal_ignore_case;
use crate::runner_mutation::{write_diagnostic, MutationDiagnosticsV1, MutationExecutionErrorV1};
use crate::windows_path::{resolve_path_spec, PathResolutionErrorV1, ValidatedPathSpecV1};
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Component, Path};

enum PreparedMkdirStateV1 {
    Ready {
        parent: File,
        missing: Vec<MissingDirectoryV1>,
    },
    ExistingDirectory,
    PathComponentIsNotDirectory,
}

struct MissingDirectoryV1 {
    name: OsString,
    resolved: String,
}

struct CreatedDirectoryV1 {
    resolved: String,
    handle: File,
}

struct PreparedMkdirOperandV1 {
    display: String,
    state: PreparedMkdirStateV1,
}

enum MkdirPreflightFailureV1 {
    KnownSafety { display: String },
    Unavailable { display: String },
}

pub(crate) fn execute_mkdir_to<E: Write>(
    plan: &ExecutionPlanV1,
    stderr: &mut E,
    cancellation: &RunnerCancellationV1,
) -> Option<Result<u8, MutationExecutionErrorV1>> {
    let [StagePlanV1::CreateDirectories { paths, parents }] = plan.stages.as_slice() else {
        return None;
    };
    if plan.redirect.is_some() {
        return None;
    }
    Some(execute(paths, *parents, stderr, cancellation))
}

fn execute<E: Write>(
    paths: &[ValidatedPathSpecV1],
    parents: bool,
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
                "wingman mkdir: current directory cannot be resolved",
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
            Err(MkdirPreflightFailureV1::KnownSafety { display }) => {
                write_diagnostic(
                    stderr,
                    &format!("wingman mkdir: {display}: reparse paths are not allowed"),
                )?;
                return Ok(2);
            }
            Err(MkdirPreflightFailureV1::Unavailable { display }) => {
                write_diagnostic(
                    stderr,
                    &format!("wingman mkdir: {display}: path safety cannot be inspected"),
                )?;
                return Ok(1);
            }
        }
    }

    let mut operational_failure = false;
    let mut diagnostics = MutationDiagnosticsV1::default();
    let mut created_directories = Vec::<CreatedDirectoryV1>::new();
    for operand in prepared {
        if cancellation.is_cancelled() {
            return Ok(130);
        }
        match operand.state {
            PreparedMkdirStateV1::ExistingDirectory if parents => {}
            PreparedMkdirStateV1::ExistingDirectory => {
                operational_failure = true;
                diagnostics.operand(
                    stderr,
                    "mkdir",
                    &operand.display,
                    "directory already exists",
                )?;
            }
            PreparedMkdirStateV1::PathComponentIsNotDirectory => {
                operational_failure = true;
                diagnostics.operand(
                    stderr,
                    "mkdir",
                    &operand.display,
                    "a path component is not a directory",
                )?;
            }
            PreparedMkdirStateV1::Ready {
                mut parent,
                missing,
            } => {
                let missing_count = missing.len();
                for (index, component) in missing.into_iter().enumerate() {
                    if cancellation.is_cancelled() {
                        return Ok(130);
                    }
                    let is_leaf = index + 1 == missing_count;
                    if let Some(created) = created_directories.iter().find(|created| {
                        names_equal_ignore_case(&created.resolved, &component.resolved)
                    }) {
                        if is_leaf && !parents {
                            operational_failure = true;
                            diagnostics.operand(
                                stderr,
                                "mkdir",
                                &operand.display,
                                "directory already exists",
                            )?;
                            break;
                        }
                        parent = match created.handle.try_clone() {
                            Ok(handle) => handle,
                            Err(_) => {
                                diagnostics.operand(
                                    stderr,
                                    "mkdir",
                                    &operand.display,
                                    "path safety cannot be rechecked",
                                )?;
                                return Ok(1);
                            }
                        };
                        continue;
                    }
                    if !parents && !is_leaf {
                        operational_failure = true;
                        diagnostics.operand(
                            stderr,
                            "mkdir",
                            &operand.display,
                            "parent directory does not exist",
                        )?;
                        break;
                    }
                    match create_verified_child_directory(&parent, &component.name) {
                        Ok(created) => {
                            let registry_handle = match created.try_clone() {
                                Ok(handle) => handle,
                                Err(_) => {
                                    diagnostics.operand(
                                        stderr,
                                        "mkdir",
                                        &operand.display,
                                        "created directory identity cannot be retained",
                                    )?;
                                    return Ok(1);
                                }
                            };
                            created_directories.push(CreatedDirectoryV1 {
                                resolved: component.resolved,
                                handle: registry_handle,
                            });
                            parent = created;
                        }
                        Err(DirectoryAccessErrorV1::ReparsePoint)
                        | Err(DirectoryAccessErrorV1::Missing)
                        | Err(DirectoryAccessErrorV1::NotDirectory) => {
                            diagnostics.operand(
                                stderr,
                                "mkdir",
                                &operand.display,
                                "path changed during directory creation",
                            )?;
                            return Ok(1);
                        }
                        Err(DirectoryAccessErrorV1::Io {
                            kind: io::ErrorKind::AlreadyExists,
                        }) => {
                            diagnostics.operand(
                                stderr,
                                "mkdir",
                                &operand.display,
                                "path changed during directory creation",
                            )?;
                            return Ok(1);
                        }
                        Err(DirectoryAccessErrorV1::Io { .. }) => {
                            operational_failure = true;
                            diagnostics.operand(
                                stderr,
                                "mkdir",
                                &operand.display,
                                "directory creation failed",
                            )?;
                            break;
                        }
                    }
                }
            }
        }
    }
    Ok(if operational_failure { 1 } else { 0 })
}

fn prepare_operand(
    spec: &ValidatedPathSpecV1,
    cwd: &str,
) -> Result<PreparedMkdirOperandV1, MkdirPreflightFailureV1> {
    let display = spec.original.clone();
    let resolved = resolve_path_spec(spec, cwd).map_err(|error| match error {
        PathResolutionErrorV1::TraversalAboveRoot
        | PathResolutionErrorV1::TooLong
        | PathResolutionErrorV1::InvalidSpec => MkdirPreflightFailureV1::KnownSafety {
            display: display.clone(),
        },
        PathResolutionErrorV1::InvalidCurrentDirectory => MkdirPreflightFailureV1::Unavailable {
            display: display.clone(),
        },
    })?;
    let (root, components) =
        split_absolute_path(&resolved).ok_or_else(|| MkdirPreflightFailureV1::KnownSafety {
            display: display.clone(),
        })?;
    let mut parent = map_directory_preflight(open_verified_root_directory(root), &display, false)?
        .expect("verified root is present");
    for (index, component) in components.iter().enumerate() {
        match open_verified_child_directory(&parent, component) {
            Ok(child) => parent = child,
            Err(DirectoryAccessErrorV1::Missing) => {
                let mut missing_path = root.to_path_buf();
                for existing in &components[..index] {
                    missing_path.push(existing);
                }
                let missing = components[index..]
                    .iter()
                    .map(|component| {
                        missing_path.push(component);
                        MissingDirectoryV1 {
                            name: component.clone(),
                            resolved: missing_path.display().to_string(),
                        }
                    })
                    .collect();
                return Ok(PreparedMkdirOperandV1 {
                    display,
                    state: PreparedMkdirStateV1::Ready { parent, missing },
                });
            }
            Err(DirectoryAccessErrorV1::NotDirectory) => {
                return Ok(PreparedMkdirOperandV1 {
                    display,
                    state: PreparedMkdirStateV1::PathComponentIsNotDirectory,
                });
            }
            error => {
                map_directory_preflight(error, &display, true)?;
                unreachable!();
            }
        }
    }
    Ok(PreparedMkdirOperandV1 {
        display,
        state: PreparedMkdirStateV1::ExistingDirectory,
    })
}

fn map_directory_preflight(
    result: Result<File, DirectoryAccessErrorV1>,
    display: &str,
    missing_is_operational: bool,
) -> Result<Option<File>, MkdirPreflightFailureV1> {
    match result {
        Ok(directory) => Ok(Some(directory)),
        Err(DirectoryAccessErrorV1::ReparsePoint) => Err(MkdirPreflightFailureV1::KnownSafety {
            display: display.to_string(),
        }),
        Err(DirectoryAccessErrorV1::Missing) if missing_is_operational => Ok(None),
        Err(DirectoryAccessErrorV1::Missing)
        | Err(DirectoryAccessErrorV1::NotDirectory)
        | Err(DirectoryAccessErrorV1::Io { .. }) => Err(MkdirPreflightFailureV1::Unavailable {
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
