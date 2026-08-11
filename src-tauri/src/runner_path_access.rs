//! Shared verified path traversal for filesystem mutation commands.
//!
//! This module owns absolute-path decomposition and handle-relative directory traversal. Callers
//! translate the small result vocabulary into command-specific diagnostics and exit statuses.

use crate::runner_io::{
    open_verified_child_directory, open_verified_root_directory, DirectoryAccessErrorV1,
    VerifiedDirectoryOpenModeV1,
};
use std::ffi::OsString;
use std::fs::File;
use std::path::{Component, Path};

pub(crate) enum VerifiedDirectoryTraversalV1 {
    Existing(File),
    Missing { parent: File, first_missing: usize },
    NotDirectory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VerifiedPathAccessErrorV1 {
    ReparsePoint,
    Unavailable,
}

pub(crate) fn split_absolute_path(path: &Path) -> Option<(&Path, Vec<OsString>)> {
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

pub(crate) fn traverse_verified_directory(
    root: &Path,
    components: &[OsString],
) -> Result<VerifiedDirectoryTraversalV1, VerifiedPathAccessErrorV1> {
    let mut parent = map_directory_access(open_verified_root_directory(root))?;
    for (index, component) in components.iter().enumerate() {
        parent = match open_verified_child_directory(
            &parent,
            component,
            VerifiedDirectoryOpenModeV1::Read,
        ) {
            Ok(child) => child,
            Err(DirectoryAccessErrorV1::Missing) => {
                return Ok(VerifiedDirectoryTraversalV1::Missing {
                    parent,
                    first_missing: index,
                });
            }
            Err(DirectoryAccessErrorV1::NotDirectory) => {
                return Ok(VerifiedDirectoryTraversalV1::NotDirectory);
            }
            Err(error) => return Err(map_directory_error(error)),
        };
    }
    Ok(VerifiedDirectoryTraversalV1::Existing(parent))
}

fn map_directory_access(
    result: Result<File, DirectoryAccessErrorV1>,
) -> Result<File, VerifiedPathAccessErrorV1> {
    result.map_err(map_directory_error)
}

fn map_directory_error(error: DirectoryAccessErrorV1) -> VerifiedPathAccessErrorV1 {
    match error {
        DirectoryAccessErrorV1::ReparsePoint => VerifiedPathAccessErrorV1::ReparsePoint,
        DirectoryAccessErrorV1::Missing
        | DirectoryAccessErrorV1::NotDirectory
        | DirectoryAccessErrorV1::Io { .. } => VerifiedPathAccessErrorV1::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn traversal_distinguishes_existing_missing_and_non_directory_paths() {
        let sandbox = std::env::temp_dir().join(format!(
            "wingman-path-access-test-{}-{}",
            std::process::id(),
            Uuid::new_v4().as_simple()
        ));
        let nested = sandbox.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(sandbox.join("file.txt"), b"not a directory").unwrap();

        let (root, existing_components) = split_absolute_path(&nested).unwrap();
        assert!(matches!(
            traverse_verified_directory(root, &existing_components).unwrap(),
            VerifiedDirectoryTraversalV1::Existing(_)
        ));

        let missing_path = nested.join("missing").join("child");
        let (root, missing_components) = split_absolute_path(&missing_path).unwrap();
        match traverse_verified_directory(root, &missing_components).unwrap() {
            VerifiedDirectoryTraversalV1::Missing { first_missing, .. } => {
                assert_eq!(missing_components[first_missing], "missing");
            }
            _ => panic!("expected first missing component"),
        }

        let blocked_path = sandbox.join("file.txt").join("child");
        let (root, blocked_components) = split_absolute_path(&blocked_path).unwrap();
        assert!(matches!(
            traverse_verified_directory(root, &blocked_components).unwrap(),
            VerifiedDirectoryTraversalV1::NotDirectory
        ));

        std::fs::remove_dir_all(&sandbox).unwrap();
    }
}
