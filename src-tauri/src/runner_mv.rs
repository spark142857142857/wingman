use crate::interpreter::{ExecutionPlanV1, ExistingDestinationPolicyV1, StagePlanV1};
use crate::runner_cancel::RunnerCancellationV1;
use crate::runner_cp::{
    close_prepared_source_children, delete_prepared_source_directory, destination_still_matches,
    execute_directory_copy, execute_file_copy, path_is_same_or_descendant, prepare_destination,
    prepare_source, source_directory_still_matches, CpPreflightFailureV1,
    PreparedDestinationStateV1, PreparedSourceDeleteResultV1, PreparedSourceDirectoryV1,
    PreparedSourceFileV1, PreparedSourceV1, TransferSourceAccessV1,
};
use crate::runner_io::{
    capture_file_identity, delete_open_file, file_matches_identity, identities_share_volume,
    rename_open_file_relative,
};
use crate::runner_ls::names_equal_ignore_case;
use crate::runner_mutation::{write_diagnostic, MutationDiagnosticsV1, MutationExecutionErrorV1};
use crate::windows_path::ValidatedPathSpecV1;
use std::io::Write;

enum PreparedMoveSourceV1 {
    File(PreparedSourceFileV1),
    Directory(PreparedSourceDirectoryV1),
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
        Ok(PreparedSourceV1::File(source)) => PreparedMoveSourceV1::File(source),
        Ok(PreparedSourceV1::Directory(Some(source))) => PreparedMoveSourceV1::Directory(source),
        Ok(PreparedSourceV1::Directory(None)) => unreachable!("move always preflights trees"),
        Err(failure) => return report_preflight_failure(stderr, failure),
    };
    let (source_resolved, source_basename, source_identity) = match &source {
        PreparedMoveSourceV1::File(source) => (&source.resolved, &source.basename, source.identity),
        PreparedMoveSourceV1::Directory(source) => {
            (&source.resolved, &source.basename, source.tree.identity)
        }
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
    if matches!(source, PreparedMoveSourceV1::Directory(_))
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
        return execute_copy_fallback(source, destination_spec, policy, stderr, cancellation);
    }
    if cancellation.is_cancelled() {
        return Ok(130);
    }
    let source_still_matches = match &source {
        PreparedMoveSourceV1::File(source) => {
            file_matches_identity(&source.handle, source.identity).unwrap_or(false)
        }
        PreparedMoveSourceV1::Directory(source) => source_directory_still_matches(&source.tree),
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
        PreparedMoveSourceV1::File(source) => source.handle,
        PreparedMoveSourceV1::Directory(source) => close_prepared_source_children(source.tree),
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
    mut source: PreparedMoveSourceV1,
    destination_spec: &ValidatedPathSpecV1,
    policy: ExistingDestinationPolicyV1,
    stderr: &mut E,
    cancellation: &RunnerCancellationV1,
) -> Result<u8, MutationExecutionErrorV1> {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd.display().to_string(),
        Err(_) => {
            write_diagnostic(stderr, "wingman mv: current directory cannot be resolved")?;
            return Ok(1);
        }
    };
    let copy_result = match &mut source {
        PreparedMoveSourceV1::File(source) => execute_file_copy(
            source,
            destination_spec,
            &cwd,
            policy,
            "mv",
            stderr,
            cancellation,
        )?,
        PreparedMoveSourceV1::Directory(source) => execute_directory_copy(
            source,
            destination_spec,
            &cwd,
            policy,
            "mv",
            stderr,
            cancellation,
        )?,
    };
    if copy_result == 130 {
        return Ok(130);
    }
    if copy_result != 0 {
        return Ok(copy_result);
    }

    let source_display = match &source {
        PreparedMoveSourceV1::File(source) => source.display.clone(),
        PreparedMoveSourceV1::Directory(source) => source.display.clone(),
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
        PreparedMoveSourceV1::File(source) => {
            file_matches_identity(&source.handle, source.identity).unwrap_or(false)
        }
        PreparedMoveSourceV1::Directory(source) => source_directory_still_matches(&source.tree),
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
        PreparedMoveSourceV1::File(source) => {
            if cancellation.is_cancelled() {
                PreparedSourceDeleteResultV1::Cancelled
            } else if delete_open_file(&source.handle).is_ok() {
                PreparedSourceDeleteResultV1::Success
            } else {
                PreparedSourceDeleteResultV1::Failed
            }
        }
        PreparedMoveSourceV1::Directory(source) => {
            delete_prepared_source_directory(source.tree, cancellation)
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

    #[test]
    fn copy_fallback_copies_and_deletes_the_same_preflighted_file() {
        let sandbox = std::env::temp_dir().join(format!(
            "wingman-mv-source-swap-test-{}-{}",
            std::process::id(),
            Uuid::new_v4().as_simple()
        ));
        std::fs::create_dir(&sandbox).unwrap();
        let source = sandbox.join("source.txt");
        let moved_original = sandbox.join("moved-original.txt");
        let destination = sandbox.join("destination.txt");
        std::fs::write(&source, b"original").unwrap();
        let source_spec = validate_path_value(&source.display().to_string()).unwrap();
        let cwd = std::env::current_dir().unwrap().display().to_string();
        let prepared = match prepare_source(
            &source_spec,
            &cwd,
            true,
            TransferSourceAccessV1::Move,
            &RunnerCancellationV1::new(),
        ) {
            Ok(prepared) => prepared,
            Err(_) => panic!("prepare source"),
        };
        let PreparedSourceV1::File(prepared) = prepared else {
            panic!("expected a prepared file");
        };
        std::fs::rename(&source, &moved_original).unwrap();
        std::fs::write(&source, b"replacement").unwrap();
        let mut stderr = Vec::new();

        let exit = execute_copy_fallback(
            PreparedMoveSourceV1::File(prepared),
            &validate_path_value(&destination.display().to_string()).unwrap(),
            ExistingDestinationPolicyV1::Replace,
            &mut stderr,
            &RunnerCancellationV1::new(),
        )
        .unwrap();

        assert_eq!(exit, 0, "{}", String::from_utf8_lossy(&stderr));
        assert_eq!(std::fs::read(&destination).unwrap(), b"original");
        assert_eq!(std::fs::read(&source).unwrap(), b"replacement");
        assert!(!moved_original.exists());
        std::fs::remove_dir_all(&sandbox).unwrap();
    }
}
