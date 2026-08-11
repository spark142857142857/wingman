use crate::interpreter::{ExecutionPlanV1, ExistingDestinationPolicyV1, StagePlanV1};
use crate::runner_cancel::RunnerCancellationV1;
use crate::runner_io::{
    capture_file_identity, create_verified_staging_child_directory,
    create_verified_staging_child_file, delete_open_file, file_matches_identity,
    list_verified_directory, open_verified_child_directory, open_verified_child_directory_for_move,
    open_verified_child_file_for_inspection, open_verified_child_file_for_move,
    open_verified_child_file_for_read, open_verified_root_directory, rename_open_file_relative,
    DirectoryAccessErrorV1, FileAccessErrorV1, FileIdentityV1, VerifiedDirectoryEntryKindV1,
};
use crate::runner_ls::names_equal_ignore_case;
use crate::runner_mutation::{write_diagnostic, MutationDiagnosticsV1, MutationExecutionErrorV1};
use crate::windows_path::{resolve_path_spec, PathResolutionErrorV1, ValidatedPathSpecV1};
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

const MAX_COPY_ENTRIES: usize = 100_000;
const MAX_COPY_DEPTH: usize = 256;

pub(crate) struct PreparedSourceFileV1 {
    pub(crate) display: String,
    pub(crate) resolved: PathBuf,
    pub(crate) basename: OsString,
    pub(crate) handle: File,
    pub(crate) identity: FileIdentityV1,
}

pub(crate) enum PreparedSourceV1 {
    File(PreparedSourceFileV1),
    Missing { basename: OsString },
    DirectoryWithoutRecursive { basename: OsString },
    Directory(Option<PreparedSourceDirectoryV1>),
}

pub(crate) struct PreparedSourceDirectoryV1 {
    pub(crate) display: String,
    pub(crate) resolved: PathBuf,
    pub(crate) basename: OsString,
    pub(crate) tree: PreparedCopyDirectoryV1,
}

pub(crate) struct PreparedCopyDirectoryV1 {
    pub(crate) handle: File,
    pub(crate) identity: FileIdentityV1,
    entries: Vec<PreparedCopyEntryV1>,
}

enum PreparedCopyEntryV1 {
    File {
        name: OsString,
        display_name: String,
        handle: File,
        identity: FileIdentityV1,
    },
    Directory {
        name: OsString,
        display_name: String,
        directory: PreparedCopyDirectoryV1,
    },
}

struct CopyPreflightStateV1 {
    visited: usize,
}

struct StagedCopyDirectoryV1 {
    handle: File,
    entries: Vec<StagedCopyEntryV1>,
}

enum StagedCopyEntryV1 {
    File(File),
    Directory(StagedCopyDirectoryV1),
}

pub(crate) enum PreparedDestinationStateV1 {
    Missing,
    ExistingFile { identity: FileIdentityV1 },
    ExistingDirectory,
    MissingParent,
}

pub(crate) struct PreparedDestinationV1 {
    pub(crate) display: String,
    pub(crate) resolved: PathBuf,
    pub(crate) parent: Option<File>,
    pub(crate) leaf: Option<OsString>,
    pub(crate) state: PreparedDestinationStateV1,
}

pub(crate) enum CpPreflightFailureV1 {
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
pub(crate) enum SourceDirectoryRecheckResultV1 {
    Matches,
    Changed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TransferSourceAccessV1 {
    Copy,
    Move,
}

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
    Some(execute(
        source,
        destination,
        *recursive,
        *existing_destination,
        stderr,
        cancellation,
    ))
}

fn execute<E: Write>(
    source_spec: &ValidatedPathSpecV1,
    destination_spec: &ValidatedPathSpecV1,
    recursive: bool,
    policy: ExistingDestinationPolicyV1,
    stderr: &mut E,
    cancellation: &RunnerCancellationV1,
) -> Result<u8, MutationExecutionErrorV1> {
    if cancellation.is_cancelled() {
        return Ok(130);
    }
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd.display().to_string(),
        Err(_) => {
            write_diagnostic(stderr, "wingman cp: current directory cannot be resolved")?;
            return Ok(1);
        }
    };
    let source = match prepare_source(
        source_spec,
        &cwd,
        recursive,
        TransferSourceAccessV1::Copy,
        cancellation,
    ) {
        Ok(source) => source,
        Err(failure) => return report_preflight_failure(stderr, failure),
    };
    let basename = match &source {
        PreparedSourceV1::File(source) => source.basename.clone(),
        PreparedSourceV1::Missing { basename }
        | PreparedSourceV1::DirectoryWithoutRecursive { basename } => basename.clone(),
        PreparedSourceV1::Directory(Some(source)) => source.basename.clone(),
        PreparedSourceV1::Directory(None) => {
            let mut diagnostics = MutationDiagnosticsV1::default();
            diagnostics.operand(
                stderr,
                "cp",
                &source_spec.original,
                "directory source requires recursive copy",
            )?;
            return Ok(1);
        }
    };
    let destination = match prepare_destination(destination_spec, &cwd, &basename) {
        Ok(destination) => destination,
        Err(failure) => return report_preflight_failure(stderr, failure),
    };
    match source {
        PreparedSourceV1::Missing { .. } => {
            let mut diagnostics = MutationDiagnosticsV1::default();
            diagnostics.operand(stderr, "cp", &source_spec.original, "source does not exist")?;
            Ok(1)
        }
        PreparedSourceV1::DirectoryWithoutRecursive { .. } => {
            let mut diagnostics = MutationDiagnosticsV1::default();
            diagnostics.operand(
                stderr,
                "cp",
                &source_spec.original,
                "directory source requires recursive copy",
            )?;
            Ok(1)
        }
        PreparedSourceV1::Directory(None) => unreachable!("root handled before destination"),
        PreparedSourceV1::Directory(Some(mut source)) => {
            execute_directory_copy(&mut source, destination, policy, "cp", stderr, cancellation)
        }
        PreparedSourceV1::File(mut source) => {
            execute_file_copy(&mut source, destination, policy, "cp", stderr, cancellation)
        }
    }
}

pub(crate) fn execute_file_copy<E: Write>(
    source: &mut PreparedSourceFileV1,
    destination: PreparedDestinationV1,
    policy: ExistingDestinationPolicyV1,
    command: &str,
    stderr: &mut E,
    cancellation: &RunnerCancellationV1,
) -> Result<u8, MutationExecutionErrorV1> {
    if cancellation.is_cancelled() {
        return Ok(130);
    }
    if names_equal_ignore_case(
        &source.resolved.display().to_string(),
        &destination.resolved.display().to_string(),
    ) {
        write_diagnostic(
            stderr,
            &format!("wingman {command}: source and destination are the same path"),
        )?;
        return Ok(2);
    }
    if let PreparedDestinationStateV1::ExistingFile { identity } = destination.state {
        if identity == source.identity {
            write_diagnostic(
                stderr,
                &format!("wingman {command}: source and destination are the same file"),
            )?;
            return Ok(2);
        }
        if policy == ExistingDestinationPolicyV1::NoClobber {
            return Ok(0);
        }
    }
    match destination.state {
        PreparedDestinationStateV1::ExistingDirectory => {
            let mut diagnostics = MutationDiagnosticsV1::default();
            diagnostics.operand(
                stderr,
                command,
                &destination.display,
                "destination directory already exists",
            )?;
            return Ok(1);
        }
        PreparedDestinationStateV1::MissingParent => {
            let mut diagnostics = MutationDiagnosticsV1::default();
            diagnostics.operand(
                stderr,
                command,
                &destination.display,
                "destination parent directory does not exist",
            )?;
            return Ok(1);
        }
        PreparedDestinationStateV1::Missing | PreparedDestinationStateV1::ExistingFile { .. } => {}
    }
    let parent = destination.parent.expect("prepared destination parent");
    let leaf = destination.leaf.expect("prepared destination leaf");
    let mut diagnostics = MutationDiagnosticsV1::default();
    let mut staging = match create_staging_file(&parent) {
        Ok(staging) => staging,
        Err(_) => {
            diagnostics.operand(
                stderr,
                command,
                &destination.display,
                "staging file creation failed",
            )?;
            return Ok(1);
        }
    };
    if let Err(cancelled) = copy_file_contents(&mut source.handle, &mut staging, cancellation) {
        let cleanup_failed = delete_open_file(&staging).is_err();
        drop(staging);
        if cancelled {
            if cleanup_failed {
                diagnostics.operand(
                    stderr,
                    command,
                    &destination.display,
                    "staging cleanup failed after cancellation",
                )?;
            }
            return Ok(130);
        }
        diagnostics.operand(stderr, command, &source.display, "file copy failed")?;
        if cleanup_failed {
            diagnostics.operand(
                stderr,
                command,
                &destination.display,
                "staging cleanup failed",
            )?;
        }
        return Ok(1);
    }
    if staging.sync_all().is_err() {
        diagnostics.operand(
            stderr,
            command,
            &destination.display,
            "staging flush failed",
        )?;
        cleanup_staging(
            &staging,
            command,
            stderr,
            &mut diagnostics,
            &destination.display,
        )?;
        return Ok(1);
    }
    if cancellation.is_cancelled() {
        cleanup_staging(
            &staging,
            command,
            stderr,
            &mut diagnostics,
            &destination.display,
        )?;
        return Ok(130);
    }
    if !file_matches_identity(&source.handle, source.identity).unwrap_or(false)
        || !destination_still_matches(&parent, &leaf, &destination.state)
    {
        diagnostics.operand(
            stderr,
            command,
            &destination.display,
            "source or destination changed before commit",
        )?;
        cleanup_staging(
            &staging,
            command,
            stderr,
            &mut diagnostics,
            &destination.display,
        )?;
        return Ok(1);
    }
    if cancellation.is_cancelled() {
        cleanup_staging(
            &staging,
            command,
            stderr,
            &mut diagnostics,
            &destination.display,
        )?;
        return Ok(130);
    }
    if rename_open_file_relative(
        &staging,
        &parent,
        &leaf,
        true,
        policy == ExistingDestinationPolicyV1::Force,
    )
    .is_err()
    {
        diagnostics.operand(stderr, command, &destination.display, "copy commit failed")?;
        cleanup_staging(
            &staging,
            command,
            stderr,
            &mut diagnostics,
            &destination.display,
        )?;
        return Ok(1);
    }
    Ok(0)
}

pub(crate) fn execute_directory_copy<E: Write>(
    source: &mut PreparedSourceDirectoryV1,
    destination: PreparedDestinationV1,
    policy: ExistingDestinationPolicyV1,
    command: &str,
    stderr: &mut E,
    cancellation: &RunnerCancellationV1,
) -> Result<u8, MutationExecutionErrorV1> {
    if cancellation.is_cancelled() {
        return Ok(130);
    }
    if path_is_same_or_descendant(&destination.resolved, &source.resolved) {
        write_diagnostic(
            stderr,
            &format!("wingman {command}: recursive destination cannot be the source or inside it"),
        )?;
        return Ok(2);
    }
    if policy == ExistingDestinationPolicyV1::NoClobber
        && matches!(
            destination.state,
            PreparedDestinationStateV1::ExistingFile { .. }
        )
    {
        return Ok(0);
    }
    match destination.state {
        PreparedDestinationStateV1::ExistingDirectory => {
            let mut diagnostics = MutationDiagnosticsV1::default();
            diagnostics.operand(
                stderr,
                command,
                &destination.display,
                "destination directory already exists",
            )?;
            return Ok(1);
        }
        PreparedDestinationStateV1::MissingParent => {
            let mut diagnostics = MutationDiagnosticsV1::default();
            diagnostics.operand(
                stderr,
                command,
                &destination.display,
                "destination parent directory does not exist",
            )?;
            return Ok(1);
        }
        PreparedDestinationStateV1::Missing | PreparedDestinationStateV1::ExistingFile { .. } => {}
    }
    let parent = destination.parent.expect("prepared destination parent");
    let leaf = destination.leaf.expect("prepared destination leaf");
    let mut diagnostics = MutationDiagnosticsV1::default();
    let staging_root = match create_staging_directory(&parent) {
        Ok(staging) => StagedCopyDirectoryV1 {
            handle: staging,
            entries: Vec::new(),
        },
        Err(_) => {
            diagnostics.operand(
                stderr,
                command,
                &destination.display,
                "staging directory creation failed",
            )?;
            return Ok(1);
        }
    };
    let staging = match stage_copy_directory(&mut source.tree, staging_root, cancellation) {
        Ok(staging) => staging,
        Err(failure) => {
            let cleanup_failed = cleanup_staged_directory(&failure.staging);
            if failure.cancelled {
                if cleanup_failed {
                    diagnostics.operand(
                        stderr,
                        command,
                        &destination.display,
                        "staging cleanup failed after cancellation",
                    )?;
                }
                return Ok(130);
            }
            diagnostics.operand(
                stderr,
                command,
                &source.display,
                "recursive staging copy failed",
            )?;
            if cleanup_failed {
                diagnostics.operand(
                    stderr,
                    command,
                    &destination.display,
                    "staging cleanup failed",
                )?;
            }
            return Ok(1);
        }
    };
    if cancellation.is_cancelled() {
        if cleanup_staged_directory(&staging) {
            diagnostics.operand(
                stderr,
                command,
                &destination.display,
                "staging cleanup failed after cancellation",
            )?;
        }
        return Ok(130);
    }
    match recheck_source_directory(&source.tree, cancellation) {
        SourceDirectoryRecheckResultV1::Cancelled => {
            if cleanup_staged_directory(&staging) {
                diagnostics.operand(
                    stderr,
                    command,
                    &destination.display,
                    "staging cleanup failed after cancellation",
                )?;
            }
            return Ok(130);
        }
        SourceDirectoryRecheckResultV1::Changed => {
            diagnostics.operand(
                stderr,
                command,
                &destination.display,
                "source or destination changed before commit",
            )?;
            if cleanup_staged_directory(&staging) {
                diagnostics.operand(
                    stderr,
                    command,
                    &destination.display,
                    "staging cleanup failed",
                )?;
            }
            return Ok(1);
        }
        SourceDirectoryRecheckResultV1::Matches => {}
    }
    if !destination_still_matches(&parent, &leaf, &destination.state) {
        diagnostics.operand(
            stderr,
            command,
            &destination.display,
            "source or destination changed before commit",
        )?;
        if cleanup_staged_directory(&staging) {
            diagnostics.operand(
                stderr,
                command,
                &destination.display,
                "staging cleanup failed",
            )?;
        }
        return Ok(1);
    }
    if cancellation.is_cancelled() {
        if cleanup_staged_directory(&staging) {
            diagnostics.operand(
                stderr,
                command,
                &destination.display,
                "staging cleanup failed after cancellation",
            )?;
        }
        return Ok(130);
    }
    let staging_root = close_staged_children(staging);
    if rename_open_file_relative(
        &staging_root,
        &parent,
        &leaf,
        true,
        policy == ExistingDestinationPolicyV1::Force,
    )
    .is_err()
    {
        diagnostics.operand(stderr, command, &destination.display, "copy commit failed")?;
        if delete_open_file(&staging_root).is_err() {
            diagnostics.operand(
                stderr,
                command,
                &destination.display,
                "staging cleanup failed",
            )?;
        }
        return Ok(1);
    }
    Ok(0)
}

fn close_staged_children(directory: StagedCopyDirectoryV1) -> File {
    for entry in directory.entries {
        match entry {
            StagedCopyEntryV1::File(file) => drop(file),
            StagedCopyEntryV1::Directory(child) => drop(close_staged_children(child)),
        }
    }
    directory.handle
}

struct StageCopyFailureV1 {
    cancelled: bool,
    staging: StagedCopyDirectoryV1,
}

fn stage_copy_directory(
    source: &mut PreparedCopyDirectoryV1,
    mut staging: StagedCopyDirectoryV1,
    cancellation: &RunnerCancellationV1,
) -> Result<StagedCopyDirectoryV1, StageCopyFailureV1> {
    for entry in &mut source.entries {
        if cancellation.is_cancelled() {
            return Err(StageCopyFailureV1 {
                cancelled: true,
                staging,
            });
        }
        match entry {
            PreparedCopyEntryV1::File { name, handle, .. } => {
                let file = match create_verified_staging_child_file(&staging.handle, name) {
                    Ok(file) => file,
                    Err(_) => {
                        return Err(StageCopyFailureV1 {
                            cancelled: false,
                            staging,
                        });
                    }
                };
                staging.entries.push(StagedCopyEntryV1::File(file));
                let Some(StagedCopyEntryV1::File(destination)) = staging.entries.last_mut() else {
                    unreachable!();
                };
                if let Err(cancelled) = copy_file_contents(handle, destination, cancellation) {
                    return Err(StageCopyFailureV1 { cancelled, staging });
                }
                if destination.sync_all().is_err() {
                    return Err(StageCopyFailureV1 {
                        cancelled: false,
                        staging,
                    });
                }
            }
            PreparedCopyEntryV1::Directory {
                name, directory, ..
            } => {
                let child = match create_verified_staging_child_directory(&staging.handle, name) {
                    Ok(handle) => StagedCopyDirectoryV1 {
                        handle,
                        entries: Vec::new(),
                    },
                    Err(_) => {
                        return Err(StageCopyFailureV1 {
                            cancelled: false,
                            staging,
                        });
                    }
                };
                match stage_copy_directory(directory, child, cancellation) {
                    Ok(child) => staging.entries.push(StagedCopyEntryV1::Directory(child)),
                    Err(failure) => {
                        staging
                            .entries
                            .push(StagedCopyEntryV1::Directory(failure.staging));
                        return Err(StageCopyFailureV1 {
                            cancelled: failure.cancelled,
                            staging,
                        });
                    }
                }
            }
        }
    }
    Ok(staging)
}

fn cleanup_staged_directory(directory: &StagedCopyDirectoryV1) -> bool {
    let mut failed = false;
    for entry in directory.entries.iter().rev() {
        match entry {
            StagedCopyEntryV1::File(file) => failed |= delete_open_file(file).is_err(),
            StagedCopyEntryV1::Directory(child) => failed |= cleanup_staged_directory(child),
        }
    }
    failed |= delete_open_file(&directory.handle).is_err();
    failed
}

pub(crate) fn recheck_source_directory(
    directory: &PreparedCopyDirectoryV1,
    cancellation: &RunnerCancellationV1,
) -> SourceDirectoryRecheckResultV1 {
    if cancellation.is_cancelled() {
        return SourceDirectoryRecheckResultV1::Cancelled;
    }
    if !file_matches_identity(&directory.handle, directory.identity).unwrap_or(false) {
        return SourceDirectoryRecheckResultV1::Changed;
    }
    let Ok(current) = list_verified_directory(&directory.handle) else {
        return SourceDirectoryRecheckResultV1::Changed;
    };
    if current.len() != directory.entries.len() {
        return SourceDirectoryRecheckResultV1::Changed;
    }
    for (expected, current) in directory.entries.iter().zip(current) {
        if cancellation.is_cancelled() {
            return SourceDirectoryRecheckResultV1::Cancelled;
        }
        match expected {
            PreparedCopyEntryV1::File {
                display_name,
                identity,
                ..
            } if current.kind == VerifiedDirectoryEntryKindV1::File
                && current.display_name == *display_name =>
            {
                let Ok(file) =
                    open_verified_child_file_for_inspection(&directory.handle, &current.name)
                else {
                    return SourceDirectoryRecheckResultV1::Changed;
                };
                if !file_matches_identity(&file, *identity).unwrap_or(false) {
                    return SourceDirectoryRecheckResultV1::Changed;
                }
            }
            PreparedCopyEntryV1::Directory {
                display_name,
                directory: child,
                ..
            } if current.kind == VerifiedDirectoryEntryKindV1::Directory
                && current.display_name == *display_name =>
            {
                let Ok(opened) = open_verified_child_directory(&directory.handle, &current.name)
                else {
                    return SourceDirectoryRecheckResultV1::Changed;
                };
                if !file_matches_identity(&opened, child.identity).unwrap_or(false) {
                    return SourceDirectoryRecheckResultV1::Changed;
                }
                match recheck_source_directory(child, cancellation) {
                    SourceDirectoryRecheckResultV1::Matches => {}
                    result => return result,
                }
            }
            _ => return SourceDirectoryRecheckResultV1::Changed,
        }
    }
    SourceDirectoryRecheckResultV1::Matches
}

pub(crate) fn close_prepared_source_children(directory: PreparedCopyDirectoryV1) -> File {
    for entry in directory.entries {
        match entry {
            PreparedCopyEntryV1::File { handle, .. } => drop(handle),
            PreparedCopyEntryV1::Directory { directory, .. } => {
                drop(close_prepared_source_children(directory));
            }
        }
    }
    directory.handle
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PreparedSourceDeleteResultV1 {
    Success,
    Cancelled,
    Failed,
}

pub(crate) fn delete_prepared_source_directory(
    directory: PreparedCopyDirectoryV1,
    cancellation: &RunnerCancellationV1,
) -> PreparedSourceDeleteResultV1 {
    for entry in directory.entries {
        if cancellation.is_cancelled() {
            return PreparedSourceDeleteResultV1::Cancelled;
        }
        let result = match entry {
            PreparedCopyEntryV1::File { handle, .. } => {
                if delete_open_file(&handle).is_ok() {
                    PreparedSourceDeleteResultV1::Success
                } else {
                    PreparedSourceDeleteResultV1::Failed
                }
            }
            PreparedCopyEntryV1::Directory { directory, .. } => {
                delete_prepared_source_directory(directory, cancellation)
            }
        };
        if result != PreparedSourceDeleteResultV1::Success {
            return result;
        }
    }
    if cancellation.is_cancelled() {
        PreparedSourceDeleteResultV1::Cancelled
    } else if delete_open_file(&directory.handle).is_ok() {
        PreparedSourceDeleteResultV1::Success
    } else {
        PreparedSourceDeleteResultV1::Failed
    }
}

fn create_staging_directory(parent: &File) -> Result<File, DirectoryAccessErrorV1> {
    for _ in 0..8 {
        let name = format!(".wingman-stage-{}.tmp", Uuid::new_v4().as_simple());
        match create_verified_staging_child_directory(parent, OsStr::new(&name)) {
            Err(DirectoryAccessErrorV1::Io {
                kind: std::io::ErrorKind::AlreadyExists,
            }) => continue,
            result => return result,
        }
    }
    Err(DirectoryAccessErrorV1::Io {
        kind: std::io::ErrorKind::AlreadyExists,
    })
}

pub(crate) fn path_is_same_or_descendant(candidate: &Path, ancestor: &Path) -> bool {
    let candidate = candidate.components().collect::<Vec<_>>();
    let ancestor = ancestor.components().collect::<Vec<_>>();
    candidate.len() >= ancestor.len()
        && candidate.iter().zip(ancestor).all(|(left, right)| {
            names_equal_ignore_case(
                &left.as_os_str().to_string_lossy(),
                &right.as_os_str().to_string_lossy(),
            )
        })
}

fn cleanup_staging<E: Write>(
    staging: &File,
    command: &str,
    stderr: &mut E,
    diagnostics: &mut MutationDiagnosticsV1,
    display: &str,
) -> Result<(), MutationExecutionErrorV1> {
    if delete_open_file(staging).is_err() {
        diagnostics.operand(stderr, command, display, "staging cleanup failed")?;
    }
    Ok(())
}

pub(crate) fn prepare_source(
    spec: &ValidatedPathSpecV1,
    cwd: &str,
    recursive: bool,
    access: TransferSourceAccessV1,
    cancellation: &RunnerCancellationV1,
) -> Result<PreparedSourceV1, CpPreflightFailureV1> {
    let display = spec.original.clone();
    let resolved = resolve_copy_path(spec, cwd, &display)?;
    let (root, mut components) =
        split_absolute_path(&resolved).ok_or_else(|| CpPreflightFailureV1::KnownSafety {
            display: display.clone(),
            message: "unsupported source path",
        })?;
    let Some(leaf) = components.pop() else {
        return if recursive {
            Err(CpPreflightFailureV1::KnownSafety {
                display,
                message: "copying a filesystem root is not supported",
            })
        } else {
            Ok(PreparedSourceV1::Directory(None))
        };
    };
    let Some(parent) = traverse_parent(root, &components, &display)? else {
        return Ok(PreparedSourceV1::Missing { basename: leaf });
    };
    let opened_directory = match access {
        TransferSourceAccessV1::Copy => open_verified_child_directory(&parent, &leaf),
        TransferSourceAccessV1::Move => open_verified_child_directory_for_move(&parent, &leaf),
    };
    match opened_directory {
        Ok(_directory) if !recursive => {
            Ok(PreparedSourceV1::DirectoryWithoutRecursive { basename: leaf })
        }
        Ok(directory) => {
            let mut state = CopyPreflightStateV1 { visited: 1 };
            let tree =
                prepare_copy_directory(directory, 0, &mut state, &display, access, cancellation)?;
            Ok(PreparedSourceV1::Directory(Some(
                PreparedSourceDirectoryV1 {
                    display,
                    resolved,
                    basename: leaf,
                    tree,
                },
            )))
        }
        Err(DirectoryAccessErrorV1::ReparsePoint) => Err(CpPreflightFailureV1::KnownSafety {
            display,
            message: "reparse sources are not allowed",
        }),
        Err(DirectoryAccessErrorV1::Io { .. }) => {
            Err(CpPreflightFailureV1::Unavailable { display })
        }
        Err(DirectoryAccessErrorV1::Missing) | Err(DirectoryAccessErrorV1::NotDirectory) => {
            let opened_file = match access {
                TransferSourceAccessV1::Copy => open_verified_child_file_for_read(&parent, &leaf),
                TransferSourceAccessV1::Move => open_verified_child_file_for_move(&parent, &leaf),
            };
            match opened_file {
                Ok(handle) => {
                    let identity = capture_file_identity(&handle).map_err(|_| {
                        CpPreflightFailureV1::Unavailable {
                            display: display.clone(),
                        }
                    })?;
                    Ok(PreparedSourceV1::File(PreparedSourceFileV1 {
                        display,
                        resolved,
                        basename: leaf,
                        handle,
                        identity,
                    }))
                }
                Err(FileAccessErrorV1::Missing) => Ok(PreparedSourceV1::Missing { basename: leaf }),
                Err(FileAccessErrorV1::ReparsePoint) => Err(CpPreflightFailureV1::KnownSafety {
                    display,
                    message: "reparse sources are not allowed",
                }),
                Err(FileAccessErrorV1::NotRegularFile) | Err(FileAccessErrorV1::Io { .. }) => {
                    Err(CpPreflightFailureV1::Unavailable { display })
                }
            }
        }
    }
}

fn prepare_copy_directory(
    handle: File,
    depth: usize,
    state: &mut CopyPreflightStateV1,
    display: &str,
    access: TransferSourceAccessV1,
    cancellation: &RunnerCancellationV1,
) -> Result<PreparedCopyDirectoryV1, CpPreflightFailureV1> {
    if cancellation.is_cancelled() {
        return Err(CpPreflightFailureV1::Cancelled);
    }
    if depth > MAX_COPY_DEPTH || state.visited > MAX_COPY_ENTRIES {
        return Err(CpPreflightFailureV1::KnownSafety {
            display: display.to_string(),
            message: "recursive copy exceeds a resource limit",
        });
    }
    let identity =
        capture_file_identity(&handle).map_err(|_| CpPreflightFailureV1::Unavailable {
            display: display.to_string(),
        })?;
    let listed =
        list_verified_directory(&handle).map_err(|_| CpPreflightFailureV1::Unavailable {
            display: display.to_string(),
        })?;
    let mut entries = Vec::with_capacity(listed.len());
    for entry in listed {
        if cancellation.is_cancelled() {
            return Err(CpPreflightFailureV1::Cancelled);
        }
        state.visited = state.visited.saturating_add(1);
        if state.visited > MAX_COPY_ENTRIES {
            return Err(CpPreflightFailureV1::KnownSafety {
                display: display.to_string(),
                message: "recursive copy exceeds a resource limit",
            });
        }
        match entry.kind {
            VerifiedDirectoryEntryKindV1::ReparsePoint => {
                return Err(CpPreflightFailureV1::KnownSafety {
                    display: display.to_string(),
                    message: "recursive source contains a reparse point",
                });
            }
            VerifiedDirectoryEntryKindV1::File => {
                let opened_file = match access {
                    TransferSourceAccessV1::Copy => {
                        open_verified_child_file_for_read(&handle, &entry.name)
                    }
                    TransferSourceAccessV1::Move => {
                        open_verified_child_file_for_move(&handle, &entry.name)
                    }
                };
                let file = opened_file.map_err(|error| match error {
                    FileAccessErrorV1::ReparsePoint => CpPreflightFailureV1::KnownSafety {
                        display: display.to_string(),
                        message: "recursive source contains a reparse point",
                    },
                    _ => CpPreflightFailureV1::Unavailable {
                        display: display.to_string(),
                    },
                })?;
                let identity = capture_file_identity(&file).map_err(|_| {
                    CpPreflightFailureV1::Unavailable {
                        display: display.to_string(),
                    }
                })?;
                entries.push(PreparedCopyEntryV1::File {
                    name: entry.name,
                    display_name: entry.display_name,
                    handle: file,
                    identity,
                });
            }
            VerifiedDirectoryEntryKindV1::Directory => {
                let opened_directory = match access {
                    TransferSourceAccessV1::Copy => {
                        open_verified_child_directory(&handle, &entry.name)
                    }
                    TransferSourceAccessV1::Move => {
                        open_verified_child_directory_for_move(&handle, &entry.name)
                    }
                };
                let directory = opened_directory.map_err(|error| match error {
                    DirectoryAccessErrorV1::ReparsePoint => CpPreflightFailureV1::KnownSafety {
                        display: display.to_string(),
                        message: "recursive source contains a reparse point",
                    },
                    _ => CpPreflightFailureV1::Unavailable {
                        display: display.to_string(),
                    },
                })?;
                let directory = prepare_copy_directory(
                    directory,
                    depth + 1,
                    state,
                    display,
                    access,
                    cancellation,
                )?;
                entries.push(PreparedCopyEntryV1::Directory {
                    name: entry.name,
                    display_name: entry.display_name,
                    directory,
                });
            }
        }
    }
    Ok(PreparedCopyDirectoryV1 {
        handle,
        identity,
        entries,
    })
}

pub(crate) fn prepare_destination(
    spec: &ValidatedPathSpecV1,
    cwd: &str,
    source_basename: &OsStr,
) -> Result<PreparedDestinationV1, CpPreflightFailureV1> {
    let display = spec.original.clone();
    let resolved = resolve_copy_path(spec, cwd, &display)?;
    let (root, mut components) =
        split_absolute_path(&resolved).ok_or_else(|| CpPreflightFailureV1::KnownSafety {
            display: display.clone(),
            message: "unsupported destination path",
        })?;
    if components.is_empty() {
        let parent = open_root(root, &display)?;
        return inspect_effective_destination(
            display,
            resolved.join(source_basename),
            parent,
            source_basename.to_os_string(),
        );
    }
    let leaf = components.pop().unwrap();
    let Some(parent) = traverse_parent(root, &components, &display)? else {
        return Ok(PreparedDestinationV1 {
            display,
            resolved,
            parent: None,
            leaf: None,
            state: PreparedDestinationStateV1::MissingParent,
        });
    };
    match open_verified_child_directory(&parent, &leaf) {
        Ok(directory) => inspect_effective_destination(
            display,
            resolved.join(source_basename),
            directory,
            source_basename.to_os_string(),
        ),
        Err(DirectoryAccessErrorV1::ReparsePoint) => Err(CpPreflightFailureV1::KnownSafety {
            display,
            message: "reparse destinations are not allowed",
        }),
        Err(DirectoryAccessErrorV1::Io { .. }) => {
            Err(CpPreflightFailureV1::Unavailable { display })
        }
        Err(DirectoryAccessErrorV1::Missing) | Err(DirectoryAccessErrorV1::NotDirectory) => {
            inspect_effective_destination(display, resolved, parent, leaf)
        }
    }
}

fn inspect_effective_destination(
    display: String,
    resolved: PathBuf,
    parent: File,
    leaf: OsString,
) -> Result<PreparedDestinationV1, CpPreflightFailureV1> {
    let state = match open_verified_child_directory(&parent, &leaf) {
        Ok(_) => PreparedDestinationStateV1::ExistingDirectory,
        Err(DirectoryAccessErrorV1::ReparsePoint) => {
            return Err(CpPreflightFailureV1::KnownSafety {
                display,
                message: "reparse destinations are not allowed",
            });
        }
        Err(DirectoryAccessErrorV1::Io { .. }) => {
            return Err(CpPreflightFailureV1::Unavailable { display });
        }
        Err(DirectoryAccessErrorV1::Missing) | Err(DirectoryAccessErrorV1::NotDirectory) => {
            match open_verified_child_file_for_inspection(&parent, &leaf) {
                Ok(file) => PreparedDestinationStateV1::ExistingFile {
                    identity: capture_file_identity(&file).map_err(|_| {
                        CpPreflightFailureV1::Unavailable {
                            display: display.clone(),
                        }
                    })?,
                },
                Err(FileAccessErrorV1::Missing) => PreparedDestinationStateV1::Missing,
                Err(FileAccessErrorV1::ReparsePoint) => {
                    return Err(CpPreflightFailureV1::KnownSafety {
                        display,
                        message: "reparse destinations are not allowed",
                    });
                }
                Err(FileAccessErrorV1::NotRegularFile) => {
                    PreparedDestinationStateV1::ExistingDirectory
                }
                Err(FileAccessErrorV1::Io { .. }) => {
                    return Err(CpPreflightFailureV1::Unavailable { display });
                }
            }
        }
    };
    Ok(PreparedDestinationV1 {
        display,
        resolved,
        parent: Some(parent),
        leaf: Some(leaf),
        state,
    })
}

fn traverse_parent(
    root: &Path,
    components: &[OsString],
    display: &str,
) -> Result<Option<File>, CpPreflightFailureV1> {
    let mut parent = open_root(root, display)?;
    for component in components {
        parent = match open_verified_child_directory(&parent, component) {
            Ok(child) => child,
            Err(DirectoryAccessErrorV1::Missing) | Err(DirectoryAccessErrorV1::NotDirectory) => {
                return Ok(None)
            }
            Err(DirectoryAccessErrorV1::ReparsePoint) => {
                return Err(CpPreflightFailureV1::KnownSafety {
                    display: display.to_string(),
                    message: "reparse ancestors are not allowed",
                });
            }
            Err(DirectoryAccessErrorV1::Io { .. }) => {
                return Err(CpPreflightFailureV1::Unavailable {
                    display: display.to_string(),
                });
            }
        };
    }
    Ok(Some(parent))
}

fn open_root(root: &Path, display: &str) -> Result<File, CpPreflightFailureV1> {
    match open_verified_root_directory(root) {
        Ok(root) => Ok(root),
        Err(DirectoryAccessErrorV1::ReparsePoint) => Err(CpPreflightFailureV1::KnownSafety {
            display: display.to_string(),
            message: "reparse ancestors are not allowed",
        }),
        Err(_) => Err(CpPreflightFailureV1::Unavailable {
            display: display.to_string(),
        }),
    }
}

fn resolve_copy_path(
    spec: &ValidatedPathSpecV1,
    cwd: &str,
    display: &str,
) -> Result<PathBuf, CpPreflightFailureV1> {
    resolve_path_spec(spec, cwd).map_err(|error| match error {
        PathResolutionErrorV1::TraversalAboveRoot
        | PathResolutionErrorV1::TooLong
        | PathResolutionErrorV1::InvalidSpec => CpPreflightFailureV1::KnownSafety {
            display: display.to_string(),
            message: "unsupported path",
        },
        PathResolutionErrorV1::InvalidCurrentDirectory => CpPreflightFailureV1::Unavailable {
            display: display.to_string(),
        },
    })
}

fn report_preflight_failure<E: Write>(
    stderr: &mut E,
    failure: CpPreflightFailureV1,
) -> Result<u8, MutationExecutionErrorV1> {
    match failure {
        CpPreflightFailureV1::KnownSafety { display, message } => {
            write_diagnostic(stderr, &format!("wingman cp: {display}: {message}"))?;
            Ok(2)
        }
        CpPreflightFailureV1::Unavailable { display } => {
            write_diagnostic(
                stderr,
                &format!("wingman cp: {display}: path safety cannot be inspected"),
            )?;
            Ok(1)
        }
        CpPreflightFailureV1::Cancelled => Ok(130),
    }
}

fn create_staging_file(parent: &File) -> Result<File, FileAccessErrorV1> {
    for _ in 0..8 {
        let name = format!(".wingman-stage-{}.tmp", Uuid::new_v4().as_simple());
        match create_verified_staging_child_file(parent, OsStr::new(&name)) {
            Err(FileAccessErrorV1::Io {
                kind: std::io::ErrorKind::AlreadyExists,
            }) => continue,
            result => return result,
        }
    }
    Err(FileAccessErrorV1::Io {
        kind: std::io::ErrorKind::AlreadyExists,
    })
}

fn copy_file_contents(
    source: &mut File,
    destination: &mut File,
    cancellation: &RunnerCancellationV1,
) -> Result<(), bool> {
    let mut buffer = [0u8; 64 * 1024];
    loop {
        if cancellation.is_cancelled() {
            return Err(true);
        }
        let read = source.read(&mut buffer).map_err(|_| false)?;
        if read == 0 {
            return Ok(());
        }
        let mut offset = 0;
        while offset < read {
            if cancellation.is_cancelled() {
                return Err(true);
            }
            let written = destination
                .write(&buffer[offset..read])
                .map_err(|_| false)?;
            if written == 0 {
                return Err(false);
            }
            offset += written;
        }
    }
}

pub(crate) fn destination_still_matches(
    parent: &File,
    leaf: &OsStr,
    expected: &PreparedDestinationStateV1,
) -> bool {
    match (
        expected,
        open_verified_child_file_for_inspection(parent, leaf),
    ) {
        (PreparedDestinationStateV1::Missing, Err(FileAccessErrorV1::Missing)) => true,
        (PreparedDestinationStateV1::ExistingFile { identity }, Ok(current)) => {
            file_matches_identity(&current, *identity).unwrap_or(false)
        }
        _ => false,
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
