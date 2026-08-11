use crate::interpreter::{ExecutionPlanV1, StagePlanV1};
use crate::runner_cancel::RunnerCancellationV1;
use crate::runner_cp::path_is_same_or_descendant;
use crate::runner_io::{
    capture_file_identity, delete_open_file_with_force, file_matches_identity,
    list_verified_directory, open_child_for_removal, open_verified_child_directory,
    open_verified_root_directory, DirectoryAccessErrorV1, FileIdentityV1, RemovalEntryKindV1,
    VerifiedDirectoryEntryKindV1,
};
use crate::runner_mutation::{write_diagnostic, MutationDiagnosticsV1, MutationExecutionErrorV1};
use crate::windows_path::{resolve_path_spec, PathResolutionErrorV1, ValidatedPathSpecV1};
use std::ffi::OsString;
use std::fs::File;
use std::io::Write;
use std::path::{Component, Path};

const MAX_REMOVE_ENTRIES: usize = 100_000;
const MAX_REMOVE_DEPTH: usize = 256;

struct PreparedRemoveTargetV1 {
    display: String,
    state: PreparedRemoveStateV1,
}

enum PreparedRemoveStateV1 {
    Missing,
    DirectoryRequiresRecursive,
    Ready(PreparedRemoveNodeV1),
}

struct PreparedRemoveNodeV1 {
    name: OsString,
    display: String,
    handle: File,
    identity: FileIdentityV1,
    kind: PreparedRemoveKindV1,
}

enum PreparedRemoveKindV1 {
    File,
    ReparsePoint,
    Directory(Vec<PreparedRemoveNodeV1>),
}

struct RemovePreflightStateV1 {
    visited: usize,
}

enum RemovePreflightFailureV1 {
    KnownSafety {
        display: String,
        message: &'static str,
    },
    Unavailable {
        display: String,
    },
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RemoveExecutionResultV1 {
    Success,
    OperationalFailure,
    SafetyMismatch,
    Cancelled,
}

pub(crate) fn execute_rm_to<E: Write>(
    plan: &ExecutionPlanV1,
    stderr: &mut E,
    cancellation: &RunnerCancellationV1,
) -> Option<Result<u8, MutationExecutionErrorV1>> {
    let [StagePlanV1::RemovePaths {
        paths,
        recursive,
        force,
    }] = plan.stages.as_slice()
    else {
        return None;
    };
    if plan.redirect.is_some() {
        return None;
    }
    Some(execute(paths, *recursive, *force, stderr, cancellation))
}

fn execute<E: Write>(
    paths: &[ValidatedPathSpecV1],
    recursive: bool,
    force: bool,
    stderr: &mut E,
    cancellation: &RunnerCancellationV1,
) -> Result<u8, MutationExecutionErrorV1> {
    if cancellation.is_cancelled() {
        return Ok(130);
    }
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(_) => {
            write_diagnostic(stderr, "wingman rm: current directory cannot be resolved")?;
            return Ok(1);
        }
    };
    let cwd_display = cwd.display().to_string();
    let mut prepared = Vec::with_capacity(paths.len());
    for path in paths {
        if cancellation.is_cancelled() {
            return Ok(130);
        }
        match prepare_target(path, &cwd, &cwd_display, recursive, cancellation) {
            Ok(target) => prepared.push(target),
            Err(RemovePreflightFailureV1::KnownSafety { display, message }) => {
                write_diagnostic(stderr, &format!("wingman rm: {display}: {message}"))?;
                return Ok(2);
            }
            Err(RemovePreflightFailureV1::Unavailable { display }) => {
                write_diagnostic(
                    stderr,
                    &format!("wingman rm: {display}: path safety cannot be inspected"),
                )?;
                return Ok(1);
            }
            Err(RemovePreflightFailureV1::Cancelled) => return Ok(130),
        }
    }

    let mut diagnostics = MutationDiagnosticsV1::default();
    let mut operational_failure = false;
    for target in prepared {
        if cancellation.is_cancelled() {
            return Ok(130);
        }
        match target.state {
            PreparedRemoveStateV1::Missing if force => {}
            PreparedRemoveStateV1::Missing => {
                operational_failure = true;
                diagnostics.operand(stderr, "rm", &target.display, "path does not exist")?;
            }
            PreparedRemoveStateV1::DirectoryRequiresRecursive => {
                operational_failure = true;
                diagnostics.operand(
                    stderr,
                    "rm",
                    &target.display,
                    "is a directory; use -r to remove it",
                )?;
            }
            PreparedRemoveStateV1::Ready(node) => {
                if !node_still_matches(&node) {
                    diagnostics.operand(
                        stderr,
                        "rm",
                        &target.display,
                        "path changed during removal",
                    )?;
                    return Ok(1);
                }
                match delete_node(node, force, stderr, &mut diagnostics, cancellation)? {
                    RemoveExecutionResultV1::Success => {}
                    RemoveExecutionResultV1::OperationalFailure => operational_failure = true,
                    RemoveExecutionResultV1::SafetyMismatch => return Ok(1),
                    RemoveExecutionResultV1::Cancelled => return Ok(130),
                }
            }
        }
    }
    Ok(if operational_failure { 1 } else { 0 })
}

fn prepare_target(
    spec: &ValidatedPathSpecV1,
    cwd: &Path,
    cwd_display: &str,
    recursive: bool,
    cancellation: &RunnerCancellationV1,
) -> Result<PreparedRemoveTargetV1, RemovePreflightFailureV1> {
    let display = spec.original.clone();
    let resolved = resolve_path_spec(spec, cwd_display).map_err(|error| match error {
        PathResolutionErrorV1::TraversalAboveRoot
        | PathResolutionErrorV1::TooLong
        | PathResolutionErrorV1::InvalidSpec => RemovePreflightFailureV1::KnownSafety {
            display: display.clone(),
            message: "unsupported path",
        },
        PathResolutionErrorV1::InvalidCurrentDirectory => RemovePreflightFailureV1::Unavailable {
            display: display.clone(),
        },
    })?;
    if recursive && path_is_same_or_descendant(cwd, &resolved) {
        return Err(RemovePreflightFailureV1::KnownSafety {
            display,
            message: "recursive removal of the current directory or its ancestor is not allowed",
        });
    }
    let (root, mut components) =
        split_absolute_path(&resolved).ok_or_else(|| RemovePreflightFailureV1::KnownSafety {
            display: display.clone(),
            message: "unsupported path",
        })?;
    let Some(leaf) = components.pop() else {
        return if recursive {
            Err(RemovePreflightFailureV1::KnownSafety {
                display,
                message: "recursive removal of a filesystem root is not allowed",
            })
        } else {
            open_verified_root_directory(root).map_err(|_| {
                RemovePreflightFailureV1::Unavailable {
                    display: display.clone(),
                }
            })?;
            Ok(PreparedRemoveTargetV1 {
                display,
                state: PreparedRemoveStateV1::DirectoryRequiresRecursive,
            })
        };
    };
    let Some(parent) = traverse_parent(root, &components, &display)? else {
        return Ok(PreparedRemoveTargetV1 {
            display,
            state: PreparedRemoveStateV1::Missing,
        });
    };
    let (handle, entry_kind) = match open_child_for_removal(&parent, &leaf) {
        Ok(opened) => opened,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PreparedRemoveTargetV1 {
                display,
                state: PreparedRemoveStateV1::Missing,
            });
        }
        Err(_) => return Err(RemovePreflightFailureV1::Unavailable { display }),
    };
    if entry_kind == RemovalEntryKindV1::Directory && !recursive {
        return Ok(PreparedRemoveTargetV1 {
            display,
            state: PreparedRemoveStateV1::DirectoryRequiresRecursive,
        });
    }
    let mut state = RemovePreflightStateV1 { visited: 1 };
    let node = prepare_node(
        leaf,
        spec.original.clone(),
        handle,
        entry_kind,
        0,
        &mut state,
        cancellation,
    )?;
    Ok(PreparedRemoveTargetV1 {
        display,
        state: PreparedRemoveStateV1::Ready(node),
    })
}

fn prepare_node(
    name: OsString,
    display: String,
    handle: File,
    entry_kind: RemovalEntryKindV1,
    depth: usize,
    state: &mut RemovePreflightStateV1,
    cancellation: &RunnerCancellationV1,
) -> Result<PreparedRemoveNodeV1, RemovePreflightFailureV1> {
    if cancellation.is_cancelled() {
        return Err(RemovePreflightFailureV1::Cancelled);
    }
    if depth > MAX_REMOVE_DEPTH || state.visited > MAX_REMOVE_ENTRIES {
        return Err(RemovePreflightFailureV1::KnownSafety {
            display,
            message: "recursive removal exceeds a resource limit",
        });
    }
    let identity =
        capture_file_identity(&handle).map_err(|_| RemovePreflightFailureV1::Unavailable {
            display: display.clone(),
        })?;
    let kind =
        match entry_kind {
            RemovalEntryKindV1::File => PreparedRemoveKindV1::File,
            RemovalEntryKindV1::ReparsePoint => PreparedRemoveKindV1::ReparsePoint,
            RemovalEntryKindV1::Directory => {
                let listed = list_verified_directory(&handle).map_err(|_| {
                    RemovePreflightFailureV1::Unavailable {
                        display: display.clone(),
                    }
                })?;
                let mut children = Vec::with_capacity(listed.len());
                for entry in listed {
                    if cancellation.is_cancelled() {
                        return Err(RemovePreflightFailureV1::Cancelled);
                    }
                    state.visited = state.visited.saturating_add(1);
                    if state.visited > MAX_REMOVE_ENTRIES {
                        return Err(RemovePreflightFailureV1::KnownSafety {
                            display,
                            message: "recursive removal exceeds a resource limit",
                        });
                    }
                    let child_display = format!(r"{display}\{}", entry.display_name);
                    let (child, actual_kind) = open_child_for_removal(&handle, &entry.name)
                        .map_err(|_| RemovePreflightFailureV1::Unavailable {
                            display: child_display.clone(),
                        })?;
                    if !entry_kind_matches(entry.kind, actual_kind) {
                        return Err(RemovePreflightFailureV1::Unavailable {
                            display: child_display,
                        });
                    }
                    children.push(prepare_node(
                        entry.name,
                        child_display,
                        child,
                        actual_kind,
                        depth + 1,
                        state,
                        cancellation,
                    )?);
                }
                PreparedRemoveKindV1::Directory(children)
            }
        };
    Ok(PreparedRemoveNodeV1 {
        name,
        display,
        handle,
        identity,
        kind,
    })
}

fn node_still_matches(node: &PreparedRemoveNodeV1) -> bool {
    if !file_matches_identity(&node.handle, node.identity).unwrap_or(false) {
        return false;
    }
    let PreparedRemoveKindV1::Directory(children) = &node.kind else {
        return true;
    };
    let Ok(listed) = list_verified_directory(&node.handle) else {
        return false;
    };
    listed.len() == children.len()
        && listed.iter().zip(children).all(|(entry, child)| {
            entry.name == child.name
                && entry_kind_matches(entry.kind, prepared_entry_kind(child))
                && node_still_matches(child)
        })
}

fn delete_node<E: Write>(
    node: PreparedRemoveNodeV1,
    force: bool,
    stderr: &mut E,
    diagnostics: &mut MutationDiagnosticsV1,
    cancellation: &RunnerCancellationV1,
) -> Result<RemoveExecutionResultV1, MutationExecutionErrorV1> {
    if cancellation.is_cancelled() {
        return Ok(RemoveExecutionResultV1::Cancelled);
    }
    if !file_matches_identity(&node.handle, node.identity).unwrap_or(false) {
        diagnostics.operand(stderr, "rm", &node.display, "path changed during removal")?;
        return Ok(RemoveExecutionResultV1::SafetyMismatch);
    }
    let mut child_failed = false;
    if let PreparedRemoveKindV1::Directory(children) = node.kind {
        for child in children {
            match delete_node(child, force, stderr, diagnostics, cancellation)? {
                RemoveExecutionResultV1::Success => {}
                RemoveExecutionResultV1::OperationalFailure => child_failed = true,
                result @ (RemoveExecutionResultV1::SafetyMismatch
                | RemoveExecutionResultV1::Cancelled) => return Ok(result),
            }
        }
    }
    if child_failed {
        return Ok(RemoveExecutionResultV1::OperationalFailure);
    }
    if delete_open_file_with_force(&node.handle, force).is_err() {
        diagnostics.operand(stderr, "rm", &node.display, "could not remove item")?;
        Ok(RemoveExecutionResultV1::OperationalFailure)
    } else {
        Ok(RemoveExecutionResultV1::Success)
    }
}

fn entry_kind_matches(listed: VerifiedDirectoryEntryKindV1, actual: RemovalEntryKindV1) -> bool {
    matches!(
        (listed, actual),
        (VerifiedDirectoryEntryKindV1::File, RemovalEntryKindV1::File)
            | (
                VerifiedDirectoryEntryKindV1::Directory,
                RemovalEntryKindV1::Directory
            )
            | (
                VerifiedDirectoryEntryKindV1::ReparsePoint,
                RemovalEntryKindV1::ReparsePoint
            )
    )
}

fn prepared_entry_kind(node: &PreparedRemoveNodeV1) -> RemovalEntryKindV1 {
    match node.kind {
        PreparedRemoveKindV1::File => RemovalEntryKindV1::File,
        PreparedRemoveKindV1::Directory(_) => RemovalEntryKindV1::Directory,
        PreparedRemoveKindV1::ReparsePoint => RemovalEntryKindV1::ReparsePoint,
    }
}

fn traverse_parent(
    root: &Path,
    components: &[OsString],
    display: &str,
) -> Result<Option<File>, RemovePreflightFailureV1> {
    let mut parent = open_verified_root_directory(root).map_err(|error| match error {
        DirectoryAccessErrorV1::ReparsePoint => RemovePreflightFailureV1::KnownSafety {
            display: display.to_string(),
            message: "reparse ancestors are not allowed",
        },
        _ => RemovePreflightFailureV1::Unavailable {
            display: display.to_string(),
        },
    })?;
    for component in components {
        parent = match open_verified_child_directory(&parent, component) {
            Ok(child) => child,
            Err(DirectoryAccessErrorV1::Missing) | Err(DirectoryAccessErrorV1::NotDirectory) => {
                return Ok(None);
            }
            Err(DirectoryAccessErrorV1::ReparsePoint) => {
                return Err(RemovePreflightFailureV1::KnownSafety {
                    display: display.to_string(),
                    message: "reparse ancestors are not allowed",
                });
            }
            Err(DirectoryAccessErrorV1::Io { .. }) => {
                return Err(RemovePreflightFailureV1::Unavailable {
                    display: display.to_string(),
                });
            }
        };
    }
    Ok(Some(parent))
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
