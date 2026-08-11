use crate::interpreter::{ExecutionPlanV1, ExistingDestinationPolicyV1, StagePlanV1};
use crate::runner_cancel::RunnerCancellationV1;
use crate::runner_cp::{
    close_prepared_source_children, destination_still_matches, path_is_same_or_descendant,
    prepare_destination, prepare_source, source_directory_still_matches, CpPreflightFailureV1,
    PreparedDestinationStateV1, PreparedSourceV1, TransferSourceAccessV1,
};
use crate::runner_io::{
    capture_file_identity, file_matches_identity, identities_share_volume,
    rename_open_file_relative, FileIdentityV1,
};
use crate::runner_ls::names_equal_ignore_case;
use crate::runner_mutation::{write_diagnostic, MutationDiagnosticsV1, MutationExecutionErrorV1};
use crate::windows_path::ValidatedPathSpecV1;
use std::ffi::OsString;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

enum PreparedMoveSourceV1 {
    File {
        display: String,
        resolved: PathBuf,
        basename: OsString,
        handle: File,
        identity: FileIdentityV1,
    },
    Directory {
        display: String,
        resolved: PathBuf,
        basename: OsString,
        tree: crate::runner_cp::PreparedCopyDirectoryV1,
    },
}

pub(crate) fn execute_mv_to<E: Write>(
    plan: &ExecutionPlanV1,
    stderr: &mut E,
    cancellation: &RunnerCancellationV1,
) -> Option<Result<u8, MutationExecutionErrorV1>> {
    let [StagePlanV1::MovePath {
        source,
        destination,
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
        *existing_destination,
        stderr,
        cancellation,
    ))
}

fn execute<E: Write>(
    source_spec: &ValidatedPathSpecV1,
    destination_spec: &ValidatedPathSpecV1,
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
            write_diagnostic(stderr, "wingman mv: current directory cannot be resolved")?;
            return Ok(1);
        }
    };
    let source = match prepare_source(
        source_spec,
        &cwd,
        true,
        TransferSourceAccessV1::Move,
        cancellation,
    ) {
        Ok(PreparedSourceV1::Missing) => {
            let mut diagnostics = MutationDiagnosticsV1::default();
            diagnostics.operand(stderr, "mv", &source_spec.original, "source does not exist")?;
            return Ok(1);
        }
        Ok(PreparedSourceV1::File(source)) => PreparedMoveSourceV1::File {
            display: source.display,
            resolved: source.resolved,
            basename: source.basename,
            handle: source.handle,
            identity: source.identity,
        },
        Ok(PreparedSourceV1::Directory(Some(source))) => PreparedMoveSourceV1::Directory {
            display: source.display,
            resolved: source.resolved,
            basename: source.basename,
            tree: source.tree,
        },
        Ok(PreparedSourceV1::Directory(None)) => unreachable!("move always preflights trees"),
        Err(failure) => return report_preflight_failure(stderr, failure),
    };
    let (source_display, source_resolved, source_basename, source_identity) = match &source {
        PreparedMoveSourceV1::File {
            display,
            resolved,
            basename,
            identity,
            ..
        } => (display, resolved, basename, *identity),
        PreparedMoveSourceV1::Directory {
            display,
            resolved,
            basename,
            tree,
        } => (display, resolved, basename, tree.identity),
    };
    if cancellation.is_cancelled() {
        return Ok(130);
    }
    let destination = match prepare_destination(destination_spec, &cwd, source_basename) {
        Ok(destination) => destination,
        Err(failure) => return report_preflight_failure(stderr, failure),
    };
    if names_equal_ignore_case(
        &source_resolved.display().to_string(),
        &destination.resolved.display().to_string(),
    ) {
        write_diagnostic(
            stderr,
            "wingman mv: source and destination are the same path",
        )?;
        return Ok(2);
    }
    if matches!(source, PreparedMoveSourceV1::Directory { .. })
        && path_is_same_or_descendant(&destination.resolved, source_resolved)
    {
        write_diagnostic(
            stderr,
            "wingman mv: destination cannot be inside the source directory",
        )?;
        return Ok(2);
    }
    if let PreparedDestinationStateV1::ExistingFile { identity } = destination.state {
        if identity == source_identity {
            write_diagnostic(
                stderr,
                "wingman mv: source and destination are the same file",
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
                "mv",
                &destination.display,
                "destination directory already exists",
            )?;
            return Ok(1);
        }
        PreparedDestinationStateV1::MissingParent => {
            let mut diagnostics = MutationDiagnosticsV1::default();
            diagnostics.operand(
                stderr,
                "mv",
                &destination.display,
                "destination parent directory does not exist",
            )?;
            return Ok(1);
        }
        PreparedDestinationStateV1::Missing | PreparedDestinationStateV1::ExistingFile { .. } => {}
    }
    let parent = destination.parent.expect("prepared destination parent");
    let leaf = destination.leaf.expect("prepared destination leaf");
    let parent_identity = match capture_file_identity(&parent) {
        Ok(identity) => identity,
        Err(_) => {
            let mut diagnostics = MutationDiagnosticsV1::default();
            diagnostics.operand(
                stderr,
                "mv",
                &destination.display,
                "destination volume cannot be inspected",
            )?;
            return Ok(1);
        }
    };
    if !identities_share_volume(source_identity, parent_identity) {
        let mut diagnostics = MutationDiagnosticsV1::default();
        diagnostics.operand(
            stderr,
            "mv",
            source_display,
            "cross-volume move is not available",
        )?;
        return Ok(1);
    }
    if cancellation.is_cancelled() {
        return Ok(130);
    }
    let source_still_matches = match &source {
        PreparedMoveSourceV1::File {
            handle, identity, ..
        } => file_matches_identity(handle, *identity).unwrap_or(false),
        PreparedMoveSourceV1::Directory { tree, .. } => source_directory_still_matches(tree),
    };
    if !source_still_matches || !destination_still_matches(&parent, &leaf, &destination.state) {
        let mut diagnostics = MutationDiagnosticsV1::default();
        diagnostics.operand(
            stderr,
            "mv",
            &destination.display,
            "source or destination changed before commit",
        )?;
        return Ok(1);
    }
    let source_handle = match source {
        PreparedMoveSourceV1::File { handle, .. } => handle,
        PreparedMoveSourceV1::Directory { tree, .. } => close_prepared_source_children(tree),
    };
    if rename_open_file_relative(
        &source_handle,
        &parent,
        &leaf,
        true,
        policy == ExistingDestinationPolicyV1::Force,
    )
    .is_err()
    {
        let mut diagnostics = MutationDiagnosticsV1::default();
        diagnostics.operand(stderr, "mv", &destination.display, "move commit failed")?;
        return Ok(1);
    }
    Ok(0)
}

fn report_preflight_failure<E: Write>(
    stderr: &mut E,
    failure: CpPreflightFailureV1,
) -> Result<u8, MutationExecutionErrorV1> {
    match failure {
        CpPreflightFailureV1::KnownSafety { display, message } => {
            write_diagnostic(stderr, &format!("wingman mv: {display}: {message}"))?;
            Ok(2)
        }
        CpPreflightFailureV1::Unavailable { display } => {
            write_diagnostic(
                stderr,
                &format!("wingman mv: {display}: path safety cannot be inspected"),
            )?;
            Ok(1)
        }
        CpPreflightFailureV1::Cancelled => Ok(130),
    }
}
