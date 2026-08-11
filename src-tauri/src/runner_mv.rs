use crate::interpreter::{ExecutionPlanV1, ExistingDestinationPolicyV1, StagePlanV1};
use crate::runner_cancel::RunnerCancellationV1;
use crate::runner_cp::{
    close_prepared_source_children, delete_prepared_source_directory, destination_still_matches,
    execute_cp_to, path_is_same_or_descendant, prepare_destination, prepare_source,
    source_directory_still_matches, CpPreflightFailureV1, PreparedDestinationStateV1,
    PreparedSourceDeleteResultV1, PreparedSourceV1, TransferSourceAccessV1,
};
use crate::runner_io::{
    capture_file_identity, delete_open_file, file_matches_identity, identities_share_volume,
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
        false,
    ))
}

fn execute<E: Write>(
    source_spec: &ValidatedPathSpecV1,
    destination_spec: &ValidatedPathSpecV1,
    policy: ExistingDestinationPolicyV1,
    stderr: &mut E,
    cancellation: &RunnerCancellationV1,
    force_copy_fallback: bool,
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
    let (source_resolved, source_basename, source_identity) = match &source {
        PreparedMoveSourceV1::File {
            resolved,
            basename,
            identity,
            ..
        } => (resolved, basename, *identity),
        PreparedMoveSourceV1::Directory {
            resolved,
            basename,
            tree,
            ..
        } => (resolved, basename, tree.identity),
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
    if force_copy_fallback || !identities_share_volume(source_identity, parent_identity) {
        return execute_copy_fallback(
            source,
            source_spec,
            destination_spec,
            policy,
            stderr,
            cancellation,
        );
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

fn execute_copy_fallback<E: Write>(
    source: PreparedMoveSourceV1,
    source_spec: &ValidatedPathSpecV1,
    destination_spec: &ValidatedPathSpecV1,
    policy: ExistingDestinationPolicyV1,
    stderr: &mut E,
    cancellation: &RunnerCancellationV1,
) -> Result<u8, MutationExecutionErrorV1> {
    let copy_plan = ExecutionPlanV1 {
        stages: vec![StagePlanV1::CopyPath {
            source: source_spec.clone(),
            destination: destination_spec.clone(),
            recursive: true,
            existing_destination: policy,
        }],
        redirect: None,
    };
    let mut copy_diagnostics = Vec::new();
    let copy_result = execute_cp_to(&copy_plan, &mut copy_diagnostics, cancellation)
        .expect("validated move fallback is a copy plan")?;
    if copy_result == 130 {
        return Ok(130);
    }
    if copy_result != 0 {
        write_diagnostic(stderr, "wingman mv: cross-volume staging copy failed")?;
        return Ok(1);
    }

    let source_display = match &source {
        PreparedMoveSourceV1::File { display, .. }
        | PreparedMoveSourceV1::Directory { display, .. } => display.clone(),
    };
    if cancellation.is_cancelled() {
        write_diagnostic(
            stderr,
            &format!(
                "wingman mv: {source_display}: destination committed; source removal cancelled"
            ),
        )?;
        return Ok(130);
    }
    let source_still_matches = match &source {
        PreparedMoveSourceV1::File {
            handle, identity, ..
        } => file_matches_identity(handle, *identity).unwrap_or(false),
        PreparedMoveSourceV1::Directory { tree, .. } => source_directory_still_matches(tree),
    };
    if !source_still_matches {
        write_diagnostic(
            stderr,
            &format!(
                "wingman mv: {source_display}: destination committed; source changed and was not removed"
            ),
        )?;
        return Ok(1);
    }
    let removal = match source {
        PreparedMoveSourceV1::File { handle, .. } => {
            if cancellation.is_cancelled() {
                PreparedSourceDeleteResultV1::Cancelled
            } else if delete_open_file(&handle).is_ok() {
                PreparedSourceDeleteResultV1::Success
            } else {
                PreparedSourceDeleteResultV1::Failed
            }
        }
        PreparedMoveSourceV1::Directory { tree, .. } => {
            delete_prepared_source_directory(tree, cancellation)
        }
    };
    match removal {
        PreparedSourceDeleteResultV1::Success => Ok(0),
        PreparedSourceDeleteResultV1::Cancelled => {
            write_diagnostic(
                stderr,
                &format!(
                    "wingman mv: {source_display}: destination committed; source removal cancelled"
                ),
            )?;
            Ok(130)
        }
        PreparedSourceDeleteResultV1::Failed => {
            write_diagnostic(
                stderr,
                &format!(
                    "wingman mv: {source_display}: destination committed; source removal incomplete"
                ),
            )?;
            Ok(1)
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows_path::validate_path_value;
    use uuid::Uuid;

    #[test]
    fn forced_copy_fallback_commits_destination_then_removes_source() {
        let sandbox = std::env::temp_dir().join(format!(
            "wingman-mv-fallback-test-{}-{}",
            std::process::id(),
            Uuid::new_v4().as_simple()
        ));
        std::fs::create_dir(&sandbox).unwrap();
        let source = sandbox.join("source.txt");
        let destination = sandbox.join("destination.txt");
        std::fs::write(&source, b"fallback").unwrap();
        let mut stderr = Vec::new();

        let exit = execute(
            &validate_path_value(&source.display().to_string()).unwrap(),
            &validate_path_value(&destination.display().to_string()).unwrap(),
            ExistingDestinationPolicyV1::Replace,
            &mut stderr,
            &RunnerCancellationV1::new(),
            true,
        )
        .unwrap();

        assert_eq!(exit, 0, "{}", String::from_utf8_lossy(&stderr));
        assert!(!source.exists());
        assert_eq!(std::fs::read(&destination).unwrap(), b"fallback");
        std::fs::remove_dir_all(&sandbox).unwrap();
    }

    #[test]
    fn forced_copy_fallback_removes_directory_tree_child_first() {
        let sandbox = std::env::temp_dir().join(format!(
            "wingman-mv-directory-fallback-test-{}-{}",
            std::process::id(),
            Uuid::new_v4().as_simple()
        ));
        let source = sandbox.join("source");
        let destination = sandbox.join("destination");
        std::fs::create_dir_all(source.join("nested").join("empty")).unwrap();
        std::fs::write(source.join("nested").join("content.txt"), b"fallback tree").unwrap();
        let mut stderr = Vec::new();

        let exit = execute(
            &validate_path_value(&source.display().to_string()).unwrap(),
            &validate_path_value(&destination.display().to_string()).unwrap(),
            ExistingDestinationPolicyV1::Replace,
            &mut stderr,
            &RunnerCancellationV1::new(),
            true,
        )
        .unwrap();

        assert_eq!(exit, 0, "{}", String::from_utf8_lossy(&stderr));
        assert!(!source.exists());
        assert_eq!(
            std::fs::read(destination.join("nested").join("content.txt")).unwrap(),
            b"fallback tree"
        );
        assert!(destination.join("nested").join("empty").is_dir());
        std::fs::remove_dir_all(&sandbox).unwrap();
    }
}
