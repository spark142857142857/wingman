use crate::runner_io::capture_file_identity;
use std::fs::{File, OpenOptions};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
};

pub(crate) struct RuntimeFilesV1 {
    runner_path: PathBuf,
    _runner_guard: File,
}

impl RuntimeFilesV1 {
    pub(crate) fn resolve(current_exe: &Path) -> Result<Self, String> {
        validate_absolute_normal_path(current_exe, "application executable")?;

        let application_directory = current_exe
            .parent()
            .ok_or_else(|| "application executable has no parent directory".to_string())?;
        let runner_path = application_directory.join("wingman-runner.exe");

        let application = open_pinned_regular_file(current_exe, "application executable")?;
        let runner_guard = open_pinned_regular_file(&runner_path, "Wingman runner")?;
        if capture_file_identity(&application).map_err(|error| error.to_string())?
            == capture_file_identity(&runner_guard).map_err(|error| error.to_string())?
        {
            return Err("Wingman runner aliases the application executable".to_string());
        }

        Ok(Self {
            runner_path,
            _runner_guard: runner_guard,
        })
    }

    pub(crate) fn runner_path(&self) -> &Path {
        &self.runner_path
    }
}

fn validate_absolute_normal_path(path: &Path, label: &str) -> Result<(), String> {
    if !path.is_absolute() {
        return Err(format!("{label} path is not absolute"));
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(format!("{label} path contains a parent traversal"));
    }
    Ok(())
}

fn open_pinned_regular_file(path: &Path, label: &str) -> Result<File, String> {
    validate_absolute_normal_path(path, label)?;
    reject_reparse_ancestors(path, label)?;

    let path_metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect {label}: {error}"))?;
    if path_metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(format!("{label} is a reparse point"));
    }
    if !path_metadata.is_file() {
        return Err(format!("{label} is not a regular file"));
    }
    if path_metadata.len() == 0 {
        return Err(format!("{label} is empty"));
    }

    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|error| format!("could not open {label}: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("could not inspect {label}: {error}"))?;
    if !metadata.is_file() {
        return Err(format!("{label} is not a regular file"));
    }
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(format!("{label} is a reparse point"));
    }
    if metadata.len() == 0 {
        return Err(format!("{label} is empty"));
    }
    Ok(file)
}

fn reject_reparse_ancestors(path: &Path, label: &str) -> Result<(), String> {
    for ancestor in path.ancestors().skip(1) {
        let metadata = std::fs::symlink_metadata(ancestor)
            .map_err(|error| format!("could not inspect {label} ancestor: {error}"))?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(format!("{label} has a reparse-point ancestor"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use uuid::Uuid;

    struct Sandbox(PathBuf);

    impl Sandbox {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "wingman-runtime-files-{}",
                Uuid::new_v4().as_simple()
            ));
            fs::create_dir(&path).expect("create runtime-files sandbox");
            Self(path)
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn valid_fixture() -> (Sandbox, PathBuf) {
        let sandbox = Sandbox::new();
        let current_exe = sandbox.0.join("wingman.exe");
        fs::write(&current_exe, b"application").expect("write application fixture");
        fs::write(sandbox.0.join("wingman-runner.exe"), b"runner").expect("write runner fixture");
        (sandbox, current_exe)
    }

    #[test]
    fn resolves_and_pins_exact_packaged_runtime_files() {
        let (_sandbox, current_exe) = valid_fixture();
        let files = RuntimeFilesV1::resolve(&current_exe).expect("resolve exact runtime files");

        assert_eq!(
            files.runner_path(),
            current_exe.parent().unwrap().join("wingman-runner.exe")
        );
        assert!(fs::remove_file(files.runner_path()).is_err());
    }

    #[test]
    fn rejects_a_runner_hard_linked_to_the_application() {
        let (sandbox, current_exe) = valid_fixture();
        let runner = sandbox.0.join("wingman-runner.exe");
        fs::remove_file(&runner).expect("remove runner fixture");
        fs::hard_link(&current_exe, &runner).expect("hard-link runner fixture");

        let error = RuntimeFilesV1::resolve(&current_exe)
            .err()
            .expect("application alias must be rejected");
        assert!(error.contains("aliases the application executable"));
    }

    #[test]
    fn rejects_directories_and_empty_runtime_files() {
        let (sandbox, current_exe) = valid_fixture();
        let runner = sandbox.0.join("wingman-runner.exe");
        fs::remove_file(&runner).expect("remove runner fixture");
        fs::create_dir(&runner).expect("replace runner with directory");
        assert!(RuntimeFilesV1::resolve(&current_exe)
            .err()
            .expect("directory runner must fail")
            .contains("not a regular file"));

        fs::remove_dir(&runner).expect("remove runner directory");
        fs::write(&runner, []).expect("write empty runner");
        assert!(RuntimeFilesV1::resolve(&current_exe)
            .err()
            .expect("empty runner must fail")
            .contains("is empty"));
    }
}
