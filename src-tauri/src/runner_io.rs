use crate::text_stream::{
    encode_record_stream, RecordFrameV1, RecordStreamWriterV1, TextEncodeErrorV1,
    TextStreamWriteErrorV1,
};
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedirectModeV1 {
    Overwrite,
    Append,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedirectSpecV1 {
    pub path: PathBuf,
    pub mode: RedirectModeV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputOpenErrorV1 {
    pub index: usize,
    pub kind: io::ErrorKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IoPreparationErrorV1 {
    Inputs(Vec<InputOpenErrorV1>),
    Output { kind: io::ErrorKind },
    OutputReparsePoint,
    SameFile { input_index: usize },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SinkWriteErrorV1 {
    Encode(TextEncodeErrorV1),
    Io { kind: io::ErrorKind },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentityV1 {
    volume_serial_number: u32,
    file_index: u64,
}

pub struct PreparedInputV1 {
    file: File,
    identity: FileIdentityV1,
}

impl PreparedInputV1 {
    pub(crate) fn file_mut(&mut self) -> &mut File {
        &mut self.file
    }
}

struct PreparedOutputV1 {
    file: File,
}

#[derive(Debug)]
enum OutputOpenErrorV1 {
    Io(io::Error),
    ReparsePoint,
}

impl From<io::Error> for OutputOpenErrorV1 {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub struct PreparedFileIoV1 {
    inputs: Vec<PreparedInputV1>,
    output: Option<PreparedOutputV1>,
}

impl fmt::Debug for PreparedFileIoV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedFileIoV1")
            .field("input_count", &self.inputs.len())
            .field("redirected", &self.output.is_some())
            .finish()
    }
}

impl PreparedFileIoV1 {
    pub fn inputs(&self) -> &[PreparedInputV1] {
        &self.inputs
    }

    pub(crate) fn stream_parts_mut(&mut self) -> (&mut [PreparedInputV1], Option<&mut File>) {
        (
            &mut self.inputs,
            self.output.as_mut().map(|output| &mut output.file),
        )
    }

    pub fn write_stdout_records(
        &mut self,
        records: &[RecordFrameV1],
    ) -> Result<Option<Vec<u8>>, SinkWriteErrorV1> {
        if let Some(output) = self.output.as_mut() {
            let mut writer = RecordStreamWriterV1::new(&mut output.file);
            for record in records {
                writer
                    .push(record.clone())
                    .map_err(map_stream_write_error)?;
            }
            writer.finish().map_err(map_stream_write_error)?;
            Ok(None)
        } else {
            Ok(Some(
                encode_record_stream(records).map_err(SinkWriteErrorV1::Encode)?,
            ))
        }
    }
}

fn map_stream_write_error(error: TextStreamWriteErrorV1) -> SinkWriteErrorV1 {
    match error {
        TextStreamWriteErrorV1::Encode(error) => SinkWriteErrorV1::Encode(error),
        TextStreamWriteErrorV1::Io { kind } => SinkWriteErrorV1::Io { kind },
    }
}

pub fn prepare_file_io(
    input_paths: &[PathBuf],
    redirect: Option<RedirectSpecV1>,
) -> Result<PreparedFileIoV1, IoPreparationErrorV1> {
    let mut inputs = Vec::with_capacity(input_paths.len());
    let mut input_errors = Vec::new();
    for (index, path) in input_paths.iter().enumerate() {
        match open_regular_input(path) {
            Ok(input) => inputs.push(input),
            Err(error) => input_errors.push(InputOpenErrorV1 {
                index,
                kind: error.kind(),
            }),
        }
    }
    if !input_errors.is_empty() {
        return Err(IoPreparationErrorV1::Inputs(input_errors));
    }

    let output = match redirect {
        None => None,
        Some(spec) => {
            let mut output_file =
                open_output_without_truncation(&spec).map_err(|error| match error {
                    OutputOpenErrorV1::Io(error) => {
                        IoPreparationErrorV1::Output { kind: error.kind() }
                    }
                    OutputOpenErrorV1::ReparsePoint => IoPreparationErrorV1::OutputReparsePoint,
                })?;
            let output_identity = file_identity(&output_file)
                .map_err(|error| IoPreparationErrorV1::Output { kind: error.kind() })?;
            if let Some(input_index) = inputs
                .iter()
                .position(|input| input.identity == output_identity)
            {
                return Err(IoPreparationErrorV1::SameFile { input_index });
            }
            if spec.mode == RedirectModeV1::Overwrite {
                output_file
                    .set_len(0)
                    .and_then(|()| output_file.seek(SeekFrom::Start(0)).map(|_| ()))
                    .map_err(|error| IoPreparationErrorV1::Output { kind: error.kind() })?;
            }
            Some(PreparedOutputV1 { file: output_file })
        }
    };

    Ok(PreparedFileIoV1 { inputs, output })
}

pub(crate) fn prepare_discovered_output(
    input_paths: &[PathBuf],
    spec: &RedirectSpecV1,
) -> Result<File, IoPreparationErrorV1> {
    let mut identities = Vec::with_capacity(input_paths.len());
    let mut input_errors = Vec::new();
    for (index, path) in input_paths.iter().enumerate() {
        match open_regular_input(path) {
            Ok(input) => identities.push((index, input.identity)),
            Err(error) => input_errors.push(InputOpenErrorV1 {
                index,
                kind: error.kind(),
            }),
        }
    }
    if !input_errors.is_empty() {
        return Err(IoPreparationErrorV1::Inputs(input_errors));
    }

    let mut output_file = open_output_without_truncation(spec).map_err(|error| match error {
        OutputOpenErrorV1::Io(error) => IoPreparationErrorV1::Output { kind: error.kind() },
        OutputOpenErrorV1::ReparsePoint => IoPreparationErrorV1::OutputReparsePoint,
    })?;
    let output_identity = file_identity(&output_file)
        .map_err(|error| IoPreparationErrorV1::Output { kind: error.kind() })?;
    if let Some((input_index, _)) = identities
        .iter()
        .find(|(_, identity)| *identity == output_identity)
    {
        return Err(IoPreparationErrorV1::SameFile {
            input_index: *input_index,
        });
    }
    if spec.mode == RedirectModeV1::Overwrite {
        output_file
            .set_len(0)
            .and_then(|()| output_file.seek(SeekFrom::Start(0)).map(|_| ()))
            .map_err(|error| IoPreparationErrorV1::Output { kind: error.kind() })?;
    }
    Ok(output_file)
}

fn open_regular_input(path: &Path) -> io::Result<PreparedInputV1> {
    let file = File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "input is not a regular file",
        ));
    }
    let identity = file_identity(&file)?;
    Ok(PreparedInputV1 { file, identity })
}

#[cfg(windows)]
fn open_output_without_truncation(spec: &RedirectSpecV1) -> Result<File, OutputOpenErrorV1> {
    open_output_after_parent_is_pinned(spec, || {})
}

#[cfg(windows)]
fn open_output_after_parent_is_pinned<F>(
    spec: &RedirectSpecV1,
    after_parent_is_pinned: F,
) -> Result<File, OutputOpenErrorV1>
where
    F: FnOnce(),
{
    use std::ffi::OsString;
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_IF, FILE_OPEN_REPARSE_POINT,
        FILE_SYNCHRONOUS_IO_NONALERT,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_APPEND_DATA, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_READ_ATTRIBUTES, SYNCHRONIZE,
    };

    let absolute = if spec.path.is_absolute() {
        spec.path.clone()
    } else {
        std::env::current_dir()?.join(&spec.path)
    };
    if !absolute.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "output path is not absolute after cwd resolution",
        )
        .into());
    }

    let root = absolute
        .ancestors()
        .last()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "output has no root"))?;
    let relative = absolute.strip_prefix(root).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "output path cannot be split from its root",
        )
    })?;
    let mut components = relative
        .components()
        .map(|component| match component {
            std::path::Component::Normal(value) => Ok(value.to_os_string()),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "output path is not lexically resolved",
            )),
        })
        .collect::<io::Result<Vec<OsString>>>()?;
    let leaf = components.pop().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "output path has no file name")
    })?;

    let mut root_options = OpenOptions::new();
    root_options
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
    let mut parent = root_options.open(root)?;
    reject_reparse_handle(&parent)?;
    if !parent.metadata()?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            "output root is not a directory",
        )
        .into());
    }

    for component in components {
        let next = open_relative_to_directory(
            &parent,
            &component,
            FILE_GENERIC_READ | SYNCHRONIZE,
            FILE_OPEN,
            FILE_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
        )?;
        reject_reparse_handle(&next)?;
        parent = next;
    }

    match open_relative_to_directory(
        &parent,
        &leaf,
        FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        FILE_OPEN,
        FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
    ) {
        Ok(existing) => reject_reparse_handle(&existing)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }

    after_parent_is_pinned();

    let desired_access = match spec.mode {
        RedirectModeV1::Overwrite => FILE_GENERIC_READ | FILE_GENERIC_WRITE | SYNCHRONIZE,
        RedirectModeV1::Append => {
            FILE_GENERIC_READ | FILE_APPEND_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE
        }
    };
    let file = open_relative_to_directory(
        &parent,
        &leaf,
        desired_access,
        FILE_OPEN_IF,
        FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
    )?;
    reject_reparse_handle(&file)?;
    if !file.metadata()?.is_file() {
        return Err(
            io::Error::new(io::ErrorKind::InvalidInput, "output is not a regular file").into(),
        );
    }
    Ok(file)
}

#[cfg(windows)]
fn open_relative_to_directory(
    parent: &File,
    name: &std::ffi::OsStr,
    desired_access: u32,
    create_disposition: u32,
    create_options: u32,
) -> io::Result<File> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows_sys::Wdk::Storage::FileSystem::NtCreateFile;
    use windows_sys::Win32::Foundation::{
        RtlNtStatusToDosError, INVALID_HANDLE_VALUE, OBJ_CASE_INSENSITIVE, UNICODE_STRING,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_NORMAL, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

    let wide = name.encode_wide().collect::<Vec<_>>();
    let byte_length = wide.len().checked_mul(2).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "output component is too long")
    })?;
    let byte_length = u16::try_from(byte_length)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "output component is too long"))?;
    let object_name = UNICODE_STRING {
        Length: byte_length,
        MaximumLength: byte_length,
        Buffer: wide.as_ptr().cast_mut(),
    };
    let object_attributes = OBJECT_ATTRIBUTES {
        Length: u32::try_from(std::mem::size_of::<OBJECT_ATTRIBUTES>()).unwrap(),
        RootDirectory: parent.as_raw_handle().cast(),
        ObjectName: &object_name,
        Attributes: OBJ_CASE_INSENSITIVE,
        SecurityDescriptor: std::ptr::null(),
        SecurityQualityOfService: std::ptr::null(),
    };
    let mut io_status = IO_STATUS_BLOCK::default();
    let mut handle = INVALID_HANDLE_VALUE;
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            desired_access,
            &object_attributes,
            &mut io_status,
            std::ptr::null(),
            FILE_ATTRIBUTE_NORMAL,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            create_disposition,
            create_options,
            std::ptr::null(),
            0,
        )
    };
    if status < 0 {
        return Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(status) } as i32,
        ));
    }
    Ok(unsafe { File::from_raw_handle(handle.cast()) })
}

#[cfg(windows)]
fn reject_reparse_handle(file: &File) -> Result<(), OutputOpenErrorV1> {
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    if file_information(file)?.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        Err(OutputOpenErrorV1::ReparsePoint)
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn open_output_without_truncation(_spec: &RedirectSpecV1) -> Result<File, OutputOpenErrorV1> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Wingman output path safety requires Windows",
    )
    .into())
}

#[cfg(windows)]
fn file_identity(file: &File) -> io::Result<FileIdentityV1> {
    let information = file_information(file)?;
    Ok(FileIdentityV1 {
        volume_serial_number: information.dwVolumeSerialNumber,
        file_index: (u64::from(information.nFileIndexHigh) << 32)
            | u64::from(information.nFileIndexLow),
    })
}

#[cfg(windows)]
fn file_information(
    file: &File,
) -> io::Result<windows_sys::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), &mut information) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(information)
}

#[cfg(not(windows))]
fn file_identity(_file: &File) -> io::Result<FileIdentityV1> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Wingman file identity requires Windows",
    ))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::fs;
    use std::os::windows::fs::symlink_dir;
    use std::process::Command;
    use uuid::Uuid;

    #[test]
    fn parent_handle_prevents_a_late_junction_swap_from_redirecting_creation() {
        let sandbox = std::env::temp_dir().join(format!(
            "wingman-output-race-test-{}-{}",
            std::process::id(),
            Uuid::new_v4().as_simple()
        ));
        let requested_parent = sandbox.join("requested");
        let pinned_parent = sandbox.join("pinned");
        let alternate_target = sandbox.join("alternate");
        fs::create_dir_all(&requested_parent).unwrap();
        fs::create_dir(&alternate_target).unwrap();

        let requested_parent_for_swap = requested_parent.clone();
        let pinned_parent_for_swap = pinned_parent.clone();
        let alternate_target_for_swap = alternate_target.clone();
        let spec = RedirectSpecV1 {
            path: requested_parent.join("out.txt"),
            mode: RedirectModeV1::Overwrite,
        };
        let output = open_output_after_parent_is_pinned(&spec, move || {
            fs::rename(&requested_parent_for_swap, &pinned_parent_for_swap).unwrap();
            create_directory_reparse(&alternate_target_for_swap, &requested_parent_for_swap);
        })
        .expect("open relative to the already verified parent handle");
        drop(output);

        assert!(pinned_parent.join("out.txt").is_file());
        assert!(!alternate_target.join("out.txt").exists());
        fs::remove_dir(&requested_parent).unwrap();
        fs::remove_dir_all(&sandbox).unwrap();
    }

    fn create_directory_reparse(target: &Path, link: &Path) {
        if symlink_dir(target, link).is_ok() {
            return;
        }
        let output = Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .expect("start cmd junction fixture helper");
        assert!(output.status.success(), "create junction fixture");
    }
}
