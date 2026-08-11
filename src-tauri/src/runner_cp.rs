use crate::interpreter::{ExecutionPlanV1, ExistingDestinationPolicyV1, StagePlanV1};
use crate::runner_cancel::RunnerCancellationV1;
use crate::runner_io::{
    capture_file_identity, create_verified_staging_child_file, delete_open_file,
    file_matches_identity, open_verified_child_directory, open_verified_child_file_for_inspection,
    open_verified_child_file_for_read, open_verified_root_directory, rename_open_file_relative,
    DirectoryAccessErrorV1, FileAccessErrorV1, FileIdentityV1,
};
use crate::runner_ls::names_equal_ignore_case;
use crate::runner_mutation::{write_diagnostic, MutationDiagnosticsV1, MutationExecutionErrorV1};
use crate::windows_path::{resolve_path_spec, PathResolutionErrorV1, ValidatedPathSpecV1};
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

struct PreparedSourceFileV1 {
    display: String,
    resolved: PathBuf,
    basename: OsString,
    handle: File,
    identity: FileIdentityV1,
}

enum PreparedSourceV1 {
    File(PreparedSourceFileV1),
    Missing,
    Directory,
}

enum PreparedDestinationStateV1 {
    Missing,
    ExistingFile { identity: FileIdentityV1 },
    ExistingDirectory,
    MissingParent,
}

struct PreparedDestinationV1 {
    display: String,
    resolved: PathBuf,
    parent: Option<File>,
    leaf: Option<OsString>,
    state: PreparedDestinationStateV1,
}

enum CpPreflightFailureV1 {
    KnownSafety {
        display: String,
        message: &'static str,
    },
    Unavailable {
        display: String,
    },
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
    let source = match prepare_source(source_spec, &cwd) {
        Ok(source) => source,
        Err(failure) => return report_preflight_failure(stderr, failure),
    };
    let PreparedSourceV1::File(mut source) = source else {
        let (display, message) = match source {
            PreparedSourceV1::Missing => (&source_spec.original, "source does not exist"),
            PreparedSourceV1::Directory if !recursive => (
                &source_spec.original,
                "directory source requires recursive copy",
            ),
            PreparedSourceV1::Directory => (
                &source_spec.original,
                "recursive directory copy is not available",
            ),
            PreparedSourceV1::File(_) => unreachable!(),
        };
        let mut diagnostics = MutationDiagnosticsV1::default();
        diagnostics.operand(stderr, "cp", display, message)?;
        return Ok(1);
    };
    if cancellation.is_cancelled() {
        return Ok(130);
    }
    let destination = match prepare_destination(destination_spec, &cwd, &source.basename) {
        Ok(destination) => destination,
        Err(failure) => return report_preflight_failure(stderr, failure),
    };
    if names_equal_ignore_case(
        &source.resolved.display().to_string(),
        &destination.resolved.display().to_string(),
    ) {
        write_diagnostic(
            stderr,
            "wingman cp: source and destination are the same path",
        )?;
        return Ok(2);
    }
    if let PreparedDestinationStateV1::ExistingFile { identity } = destination.state {
        if identity == source.identity {
            write_diagnostic(
                stderr,
                "wingman cp: source and destination are the same file",
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
                "cp",
                &destination.display,
                "destination directory already exists",
            )?;
            return Ok(1);
        }
        PreparedDestinationStateV1::MissingParent => {
            let mut diagnostics = MutationDiagnosticsV1::default();
            diagnostics.operand(
                stderr,
                "cp",
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
                "cp",
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
                    "cp",
                    &destination.display,
                    "staging cleanup failed after cancellation",
                )?;
            }
            return Ok(130);
        }
        diagnostics.operand(stderr, "cp", &source.display, "file copy failed")?;
        if cleanup_failed {
            diagnostics.operand(stderr, "cp", &destination.display, "staging cleanup failed")?;
        }
        return Ok(1);
    }
    if staging.sync_all().is_err() {
        diagnostics.operand(stderr, "cp", &destination.display, "staging flush failed")?;
        cleanup_staging(&staging, stderr, &mut diagnostics, &destination.display)?;
        return Ok(1);
    }
    if cancellation.is_cancelled() {
        cleanup_staging(&staging, stderr, &mut diagnostics, &destination.display)?;
        return Ok(130);
    }
    if !file_matches_identity(&source.handle, source.identity).unwrap_or(false)
        || !destination_still_matches(&parent, &leaf, &destination.state)
    {
        diagnostics.operand(
            stderr,
            "cp",
            &destination.display,
            "source or destination changed before commit",
        )?;
        cleanup_staging(&staging, stderr, &mut diagnostics, &destination.display)?;
        return Ok(1);
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
        diagnostics.operand(stderr, "cp", &destination.display, "copy commit failed")?;
        cleanup_staging(&staging, stderr, &mut diagnostics, &destination.display)?;
        return Ok(1);
    }
    Ok(0)
}

fn cleanup_staging<E: Write>(
    staging: &File,
    stderr: &mut E,
    diagnostics: &mut MutationDiagnosticsV1,
    display: &str,
) -> Result<(), MutationExecutionErrorV1> {
    if delete_open_file(staging).is_err() {
        diagnostics.operand(stderr, "cp", display, "staging cleanup failed")?;
    }
    Ok(())
}

fn prepare_source(
    spec: &ValidatedPathSpecV1,
    cwd: &str,
) -> Result<PreparedSourceV1, CpPreflightFailureV1> {
    let display = spec.original.clone();
    let resolved = resolve_copy_path(spec, cwd, &display)?;
    let (root, mut components) =
        split_absolute_path(&resolved).ok_or_else(|| CpPreflightFailureV1::KnownSafety {
            display: display.clone(),
            message: "unsupported source path",
        })?;
    let Some(leaf) = components.pop() else {
        return Ok(PreparedSourceV1::Directory);
    };
    let Some(parent) = traverse_parent(root, &components, &display)? else {
        return Ok(PreparedSourceV1::Missing);
    };
    match open_verified_child_directory(&parent, &leaf) {
        Ok(_) => Ok(PreparedSourceV1::Directory),
        Err(DirectoryAccessErrorV1::ReparsePoint) => Err(CpPreflightFailureV1::KnownSafety {
            display,
            message: "reparse sources are not allowed",
        }),
        Err(DirectoryAccessErrorV1::Io { .. }) => {
            Err(CpPreflightFailureV1::Unavailable { display })
        }
        Err(DirectoryAccessErrorV1::Missing) | Err(DirectoryAccessErrorV1::NotDirectory) => {
            match open_verified_child_file_for_read(&parent, &leaf) {
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
                Err(FileAccessErrorV1::Missing) => Ok(PreparedSourceV1::Missing),
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

fn prepare_destination(
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

fn destination_still_matches(
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
