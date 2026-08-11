use crate::text_stream::{
    encode_record_stream, RecordFrameV1, RecordStreamWriterV1, TextEncodeErrorV1,
    TextStreamWriteErrorV1,
};
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VerifiedDirectoryEntryKindV1 {
    File,
    Directory,
    ReparsePoint,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RemovalEntryKindV1 {
    File,
    Directory,
    ReparsePoint,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VerifiedDirectoryEntryV1 {
    pub(crate) name: std::ffi::OsString,
    pub(crate) display_name: String,
    pub(crate) kind: VerifiedDirectoryEntryKindV1,
}

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
pub(crate) struct FileIdentityV1 {
    volume_serial_number: u32,
    file_index: u64,
}

#[derive(Debug)]
pub(crate) enum DirectoryAccessErrorV1 {
    Missing,
    NotDirectory,
    ReparsePoint,
    Io { kind: io::ErrorKind },
}

#[derive(Debug)]
pub(crate) enum FileAccessErrorV1 {
    Missing,
    NotRegularFile,
    ReparsePoint,
    Io { kind: io::ErrorKind },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VerifiedDirectoryOpenModeV1 {
    Read,
    MoveSource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VerifiedDirectoryCreateModeV1 {
    MutationTarget,
    Staging,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VerifiedFileOpenModeV1 {
    TouchTarget,
    ReadSource,
    MoveSource,
    Inspect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VerifiedFileCreateModeV1 {
    TouchTarget,
    Staging,
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

pub(crate) struct PreparedStreamingOutputV1 {
    file: File,
    identity: FileIdentityV1,
    existed: bool,
    link_count: u32,
    mode: RedirectModeV1,
}

impl PreparedStreamingOutputV1 {
    pub(crate) fn existed(&self) -> bool {
        self.existed
    }

    pub(crate) fn has_multiple_links(&self) -> bool {
        self.link_count > 1
    }

    pub(crate) fn identity(&self) -> FileIdentityV1 {
        self.identity
    }

    pub(crate) fn commit(&mut self) -> io::Result<()> {
        if self.mode == RedirectModeV1::Overwrite {
            self.file
                .set_len(0)
                .and_then(|()| self.file.seek(SeekFrom::Start(0)).map(|_| ()))?;
        }
        Ok(())
    }

    pub(crate) fn file_mut(&mut self) -> &mut File {
        &mut self.file
    }
}

pub(crate) fn file_matches_identity(file: &File, identity: FileIdentityV1) -> io::Result<bool> {
    file_identity(file).map(|candidate| candidate == identity)
}

pub(crate) fn identities_share_volume(left: FileIdentityV1, right: FileIdentityV1) -> bool {
    left.volume_serial_number == right.volume_serial_number
}

pub(crate) fn capture_file_identity(file: &File) -> io::Result<FileIdentityV1> {
    file_identity(file)
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

pub(crate) fn prepare_streaming_discovered_output(
    spec: &RedirectSpecV1,
) -> Result<PreparedStreamingOutputV1, IoPreparationErrorV1> {
    let existed = match std::fs::symlink_metadata(&spec.path) {
        Ok(_) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(_) => true,
    };
    let file = open_output_without_truncation(spec).map_err(|error| match error {
        OutputOpenErrorV1::Io(error) => IoPreparationErrorV1::Output { kind: error.kind() },
        OutputOpenErrorV1::ReparsePoint => IoPreparationErrorV1::OutputReparsePoint,
    })?;
    let link_count = file_link_count(&file)
        .map_err(|error| IoPreparationErrorV1::Output { kind: error.kind() })?;
    let identity = file_identity(&file)
        .map_err(|error| IoPreparationErrorV1::Output { kind: error.kind() })?;
    Ok(PreparedStreamingOutputV1 {
        file,
        identity,
        existed,
        link_count,
        mode: spec.mode,
    })
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
pub(crate) fn open_verified_root_directory(root: &Path) -> Result<File, DirectoryAccessErrorV1> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    };

    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
    let directory = options.open(root).map_err(map_directory_open_error)?;
    verify_directory_handle(&directory)?;
    Ok(directory)
}

#[cfg(windows)]
pub(crate) fn open_verified_child_directory(
    parent: &File,
    name: &std::ffi::OsStr,
    mode: VerifiedDirectoryOpenModeV1,
) -> Result<File, DirectoryAccessErrorV1> {
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT,
    };
    use windows_sys::Win32::Storage::FileSystem::{DELETE, FILE_GENERIC_READ, SYNCHRONIZE};

    let desired_access = match mode {
        VerifiedDirectoryOpenModeV1::Read => FILE_GENERIC_READ | SYNCHRONIZE,
        VerifiedDirectoryOpenModeV1::MoveSource => FILE_GENERIC_READ | DELETE | SYNCHRONIZE,
    };

    let directory = open_relative_to_directory(
        parent,
        name,
        desired_access,
        FILE_OPEN,
        FILE_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
    )
    .map_err(map_directory_open_error)?;
    verify_directory_handle(&directory)?;
    Ok(directory)
}

#[cfg(windows)]
pub(crate) fn create_verified_child_directory(
    parent: &File,
    name: &std::ffi::OsStr,
    mode: VerifiedDirectoryCreateModeV1,
) -> Result<File, DirectoryAccessErrorV1> {
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_CREATE, FILE_DIRECTORY_FILE, FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_GENERIC_READ, FILE_GENERIC_WRITE, SYNCHRONIZE,
    };

    let desired_access = match mode {
        VerifiedDirectoryCreateModeV1::MutationTarget => {
            FILE_GENERIC_READ | FILE_GENERIC_WRITE | SYNCHRONIZE
        }
        VerifiedDirectoryCreateModeV1::Staging => {
            FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE | SYNCHRONIZE
        }
    };

    let directory = open_relative_to_directory(
        parent,
        name,
        desired_access,
        FILE_CREATE,
        FILE_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
    )
    .map_err(map_directory_open_error)?;
    verify_directory_handle(&directory)?;
    Ok(directory)
}

#[cfg(windows)]
pub(crate) fn list_verified_directory(
    directory: &File,
) -> io::Result<Vec<VerifiedDirectoryEntryV1>> {
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::ERROR_NO_MORE_FILES;
    use windows_sys::Win32::Storage::FileSystem::{
        FileIdBothDirectoryInfo, FileIdBothDirectoryRestartInfo, GetFileInformationByHandleEx,
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ID_BOTH_DIR_INFO,
    };

    const BUFFER_BYTES: usize = 64 * 1024;
    let word_count = BUFFER_BYTES.div_ceil(std::mem::size_of::<u64>());
    let mut storage = vec![0u64; word_count];
    let mut entries = Vec::new();
    let mut restart = true;
    loop {
        let class = if restart {
            FileIdBothDirectoryRestartInfo
        } else {
            FileIdBothDirectoryInfo
        };
        restart = false;
        if unsafe {
            GetFileInformationByHandleEx(
                directory.as_raw_handle().cast(),
                class,
                storage.as_mut_ptr().cast(),
                u32::try_from(BUFFER_BYTES).unwrap(),
            )
        } == 0
        {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                break;
            }
            return Err(error);
        }
        let mut offset = 0usize;
        loop {
            let minimum = std::mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
            if offset
                .checked_add(minimum)
                .is_none_or(|end| end > BUFFER_BYTES)
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "directory enumeration returned an invalid record",
                ));
            }
            let record = unsafe {
                &*storage
                    .as_ptr()
                    .cast::<u8>()
                    .add(offset)
                    .cast::<FILE_ID_BOTH_DIR_INFO>()
            };
            let name_bytes = usize::try_from(record.FileNameLength).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "directory name is too long")
            })?;
            if name_bytes % std::mem::size_of::<u16>() != 0
                || offset
                    .checked_add(minimum)
                    .and_then(|start| start.checked_add(name_bytes))
                    .is_none_or(|end| end > BUFFER_BYTES)
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "directory enumeration returned an invalid name",
                ));
            }
            let name_wide = unsafe {
                std::slice::from_raw_parts(
                    record.FileName.as_ptr(),
                    name_bytes / std::mem::size_of::<u16>(),
                )
            };
            let display_name = String::from_utf16(name_wide).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "filename is not valid Unicode")
            })?;
            if display_name != "." && display_name != ".." {
                let kind = if record.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                    VerifiedDirectoryEntryKindV1::ReparsePoint
                } else if record.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
                    VerifiedDirectoryEntryKindV1::Directory
                } else {
                    VerifiedDirectoryEntryKindV1::File
                };
                entries.push(VerifiedDirectoryEntryV1 {
                    name: std::ffi::OsString::from_wide(name_wide),
                    display_name,
                    kind,
                });
            }
            if record.NextEntryOffset == 0 {
                break;
            }
            let next = usize::try_from(record.NextEntryOffset).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "directory offset is invalid")
            })?;
            if next < minimum {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "directory offset is invalid",
                ));
            }
            offset = offset.checked_add(next).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "directory offset overflow")
            })?;
        }
    }
    entries.sort_by(|left, right| {
        crate::runner_ls::compare_names(&left.display_name, &right.display_name)
    });
    Ok(entries)
}

#[cfg(windows)]
pub(crate) fn open_verified_child_file(
    parent: &File,
    name: &std::ffi::OsStr,
    mode: VerifiedFileOpenModeV1,
) -> Result<File, FileAccessErrorV1> {
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_FOR_BACKUP_INTENT, FILE_OPEN_REPARSE_POINT,
        FILE_SYNCHRONOUS_IO_NONALERT,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_GENERIC_READ, FILE_READ_ATTRIBUTES, FILE_WRITE_ATTRIBUTES, SYNCHRONIZE,
    };

    let (desired_access, create_options) = match mode {
        VerifiedFileOpenModeV1::TouchTarget => (
            FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES | SYNCHRONIZE,
            FILE_OPEN_FOR_BACKUP_INTENT | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
        ),
        VerifiedFileOpenModeV1::ReadSource => (
            FILE_GENERIC_READ | SYNCHRONIZE,
            FILE_NON_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
        ),
        VerifiedFileOpenModeV1::MoveSource => (
            FILE_GENERIC_READ | DELETE | SYNCHRONIZE,
            FILE_NON_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
        ),
        VerifiedFileOpenModeV1::Inspect => (
            FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
        ),
    };

    let file = open_relative_to_directory(parent, name, desired_access, FILE_OPEN, create_options)
        .map_err(map_file_open_error)?;
    verify_regular_file_handle(&file)?;
    Ok(file)
}

#[cfg(windows)]
pub(crate) fn open_child_for_removal(
    parent: &File,
    name: &std::ffi::OsStr,
) -> io::Result<(File, RemovalEntryKindV1)> {
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_OPEN, FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT,
    };
    use windows_sys::Win32::Storage::FileSystem::{DELETE, FILE_GENERIC_READ, SYNCHRONIZE};

    let file = open_relative_to_directory(
        parent,
        name,
        DELETE | FILE_GENERIC_READ | SYNCHRONIZE,
        FILE_OPEN,
        FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
    )?;
    let kind = if is_reparse_handle(&file)? {
        RemovalEntryKindV1::ReparsePoint
    } else {
        let metadata = file.metadata()?;
        if metadata.is_dir() {
            RemovalEntryKindV1::Directory
        } else if metadata.is_file() {
            RemovalEntryKindV1::File
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "removal target is not a file or directory",
            ));
        }
    };
    Ok((file, kind))
}

#[cfg(windows)]
pub(crate) fn create_verified_child_file(
    parent: &File,
    name: &std::ffi::OsStr,
    mode: VerifiedFileCreateModeV1,
) -> Result<File, FileAccessErrorV1> {
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_CREATE, FILE_NON_DIRECTORY_FILE, FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_READ_ATTRIBUTES, FILE_WRITE_ATTRIBUTES,
        SYNCHRONIZE,
    };

    let desired_access = match mode {
        VerifiedFileCreateModeV1::TouchTarget => {
            FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES | SYNCHRONIZE
        }
        VerifiedFileCreateModeV1::Staging => {
            FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE | SYNCHRONIZE
        }
    };

    let file = open_relative_to_directory(
        parent,
        name,
        desired_access,
        FILE_CREATE,
        FILE_NON_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
    )
    .map_err(map_file_open_error)?;
    verify_regular_file_handle(&file)?;
    Ok(file)
}

#[cfg(windows)]
pub(crate) fn rename_open_file_relative(
    file: &File,
    parent: &File,
    destination_name: &std::ffi::OsStr,
    replace: bool,
    ignore_readonly: bool,
) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Wdk::Storage::FileSystem::{
        FileRenameInformation, FileRenameInformationEx, NtSetInformationFile,
        FILE_RENAME_IGNORE_READONLY_ATTRIBUTE, FILE_RENAME_INFORMATION,
        FILE_RENAME_REPLACE_IF_EXISTS,
    };
    use windows_sys::Win32::Foundation::RtlNtStatusToDosError;
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

    let wide = destination_name.encode_wide().collect::<Vec<_>>();
    let name_bytes = wide
        .len()
        .checked_mul(std::mem::size_of::<u16>())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "rename is too long"))?;
    let total_bytes = std::mem::size_of::<FILE_RENAME_INFORMATION>()
        .checked_add(name_bytes)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "rename is too long"))?;
    let word_size = std::mem::size_of::<usize>();
    let word_count = total_bytes.div_ceil(word_size);
    let mut storage = vec![0usize; word_count];
    let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
    unsafe {
        if ignore_readonly {
            (*info).Anonymous.Flags = if replace {
                FILE_RENAME_REPLACE_IF_EXISTS | FILE_RENAME_IGNORE_READONLY_ATTRIBUTE
            } else {
                FILE_RENAME_IGNORE_READONLY_ATTRIBUTE
            };
        } else {
            (*info).Anonymous.ReplaceIfExists = replace;
        }
        (*info).RootDirectory = parent.as_raw_handle().cast();
        (*info).FileNameLength = u32::try_from(name_bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "rename is too long"))?;
        std::ptr::copy_nonoverlapping(wide.as_ptr(), (*info).FileName.as_mut_ptr(), wide.len());
    }
    let buffer_size = u32::try_from(total_bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "rename is too long"))?;
    let mut io_status = IO_STATUS_BLOCK::default();
    let status = unsafe {
        NtSetInformationFile(
            file.as_raw_handle().cast(),
            &mut io_status,
            info.cast(),
            buffer_size,
            if ignore_readonly {
                FileRenameInformationEx
            } else {
                FileRenameInformation
            },
        )
    };
    if status < 0 {
        return Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(status) } as i32,
        ));
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn delete_open_file(file: &File) -> io::Result<()> {
    delete_open_file_with_force(file, false)
}

#[cfg(windows)]
pub(crate) fn delete_open_file_with_force(file: &File, force: bool) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FileDispositionInfoEx, SetFileInformationByHandle, FILE_DISPOSITION_FLAG_DELETE,
        FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE, FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
        FILE_DISPOSITION_INFO_EX,
    };

    let disposition = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_FLAG_DELETE
            | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS
            | if force {
                FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE
            } else {
                0
            },
    };
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle().cast(),
            FileDispositionInfoEx,
            (&disposition as *const FILE_DISPOSITION_INFO_EX).cast(),
            u32::try_from(std::mem::size_of::<FILE_DISPOSITION_INFO_EX>()).unwrap(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(windows)]
fn verify_directory_handle(directory: &File) -> Result<(), DirectoryAccessErrorV1> {
    if is_reparse_handle(directory)
        .map_err(|error| DirectoryAccessErrorV1::Io { kind: error.kind() })?
    {
        return Err(DirectoryAccessErrorV1::ReparsePoint);
    }
    if !directory
        .metadata()
        .map_err(|error| DirectoryAccessErrorV1::Io { kind: error.kind() })?
        .is_dir()
    {
        return Err(DirectoryAccessErrorV1::NotDirectory);
    }
    Ok(())
}

#[cfg(windows)]
fn map_directory_open_error(error: io::Error) -> DirectoryAccessErrorV1 {
    match error.kind() {
        io::ErrorKind::NotFound => DirectoryAccessErrorV1::Missing,
        io::ErrorKind::NotADirectory => DirectoryAccessErrorV1::NotDirectory,
        kind => DirectoryAccessErrorV1::Io { kind },
    }
}

#[cfg(windows)]
fn verify_regular_file_handle(file: &File) -> Result<(), FileAccessErrorV1> {
    if is_reparse_handle(file).map_err(|error| FileAccessErrorV1::Io { kind: error.kind() })? {
        return Err(FileAccessErrorV1::ReparsePoint);
    }
    if !file
        .metadata()
        .map_err(|error| FileAccessErrorV1::Io { kind: error.kind() })?
        .is_file()
    {
        return Err(FileAccessErrorV1::NotRegularFile);
    }
    Ok(())
}

#[cfg(windows)]
fn map_file_open_error(error: io::Error) -> FileAccessErrorV1 {
    match error.kind() {
        io::ErrorKind::NotFound => FileAccessErrorV1::Missing,
        io::ErrorKind::IsADirectory | io::ErrorKind::NotADirectory => {
            FileAccessErrorV1::NotRegularFile
        }
        kind => FileAccessErrorV1::Io { kind },
    }
}

#[cfg(not(windows))]
pub(crate) fn open_verified_root_directory(_root: &Path) -> Result<File, DirectoryAccessErrorV1> {
    Err(DirectoryAccessErrorV1::Io {
        kind: io::ErrorKind::Unsupported,
    })
}

#[cfg(not(windows))]
pub(crate) fn open_verified_child_directory(
    _parent: &File,
    _name: &std::ffi::OsStr,
    _mode: VerifiedDirectoryOpenModeV1,
) -> Result<File, DirectoryAccessErrorV1> {
    Err(DirectoryAccessErrorV1::Io {
        kind: io::ErrorKind::Unsupported,
    })
}

#[cfg(not(windows))]
pub(crate) fn create_verified_child_directory(
    _parent: &File,
    _name: &std::ffi::OsStr,
    _mode: VerifiedDirectoryCreateModeV1,
) -> Result<File, DirectoryAccessErrorV1> {
    Err(DirectoryAccessErrorV1::Io {
        kind: io::ErrorKind::Unsupported,
    })
}

#[cfg(not(windows))]
pub(crate) fn list_verified_directory(
    _directory: &File,
) -> io::Result<Vec<VerifiedDirectoryEntryV1>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Wingman handle directory enumeration requires Windows",
    ))
}

#[cfg(not(windows))]
pub(crate) fn open_verified_child_file(
    _parent: &File,
    _name: &std::ffi::OsStr,
    _mode: VerifiedFileOpenModeV1,
) -> Result<File, FileAccessErrorV1> {
    Err(FileAccessErrorV1::Io {
        kind: io::ErrorKind::Unsupported,
    })
}

#[cfg(not(windows))]
pub(crate) fn create_verified_child_file(
    _parent: &File,
    _name: &std::ffi::OsStr,
    _mode: VerifiedFileCreateModeV1,
) -> Result<File, FileAccessErrorV1> {
    Err(FileAccessErrorV1::Io {
        kind: io::ErrorKind::Unsupported,
    })
}

#[cfg(not(windows))]
pub(crate) fn open_child_for_removal(
    _parent: &File,
    _name: &std::ffi::OsStr,
) -> io::Result<(File, RemovalEntryKindV1)> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Wingman handle-relative removal requires Windows",
    ))
}

#[cfg(not(windows))]
pub(crate) fn rename_open_file_relative(
    _file: &File,
    _parent: &File,
    _destination_name: &std::ffi::OsStr,
    _replace: bool,
    _ignore_readonly: bool,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Wingman handle-relative rename requires Windows",
    ))
}

#[cfg(not(windows))]
pub(crate) fn delete_open_file(_file: &File) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Wingman handle-relative deletion requires Windows",
    ))
}

#[cfg(not(windows))]
pub(crate) fn delete_open_file_with_force(_file: &File, _force: bool) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Wingman handle-relative deletion requires Windows",
    ))
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
    if is_reparse_handle(file)? {
        Err(OutputOpenErrorV1::ReparsePoint)
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn is_reparse_handle(file: &File) -> io::Result<bool> {
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    file_information(file)
        .map(|information| information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0)
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

#[cfg(windows)]
fn file_link_count(file: &File) -> io::Result<u32> {
    file_information(file).map(|information| information.nNumberOfLinks)
}

#[cfg(not(windows))]
fn file_link_count(_file: &File) -> io::Result<u32> {
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

    #[test]
    fn pinned_directory_handle_prevents_a_late_mkdir_junction_swap() {
        let sandbox = std::env::temp_dir().join(format!(
            "wingman-mkdir-race-test-{}-{}",
            std::process::id(),
            Uuid::new_v4().as_simple()
        ));
        let requested_parent = sandbox.join("requested");
        let pinned_parent = sandbox.join("pinned");
        let alternate_target = sandbox.join("alternate");
        fs::create_dir_all(&requested_parent).unwrap();
        fs::create_dir(&alternate_target).unwrap();

        let parent = open_verified_root_directory(&requested_parent)
            .expect("pin the verified requested directory");
        fs::rename(&requested_parent, &pinned_parent).unwrap();
        create_directory_reparse(&alternate_target, &requested_parent);
        let created = create_verified_child_directory(
            &parent,
            std::ffi::OsStr::new("child"),
            VerifiedDirectoryCreateModeV1::MutationTarget,
        )
        .expect("create relative to the pinned directory");
        drop(created);
        drop(parent);

        assert!(pinned_parent.join("child").is_dir());
        assert!(!alternate_target.join("child").exists());
        fs::remove_dir(&requested_parent).unwrap();
        fs::remove_dir_all(&sandbox).unwrap();
    }

    #[test]
    fn pinned_directory_handle_prevents_a_late_touch_junction_swap() {
        let sandbox = std::env::temp_dir().join(format!(
            "wingman-touch-race-test-{}-{}",
            std::process::id(),
            Uuid::new_v4().as_simple()
        ));
        let requested_parent = sandbox.join("requested");
        let pinned_parent = sandbox.join("pinned");
        let alternate_target = sandbox.join("alternate");
        fs::create_dir_all(&requested_parent).unwrap();
        fs::create_dir(&alternate_target).unwrap();

        let parent = open_verified_root_directory(&requested_parent)
            .expect("pin the verified requested directory");
        fs::rename(&requested_parent, &pinned_parent).unwrap();
        create_directory_reparse(&alternate_target, &requested_parent);
        let created = create_verified_child_file(
            &parent,
            std::ffi::OsStr::new("file.txt"),
            VerifiedFileCreateModeV1::TouchTarget,
        )
        .expect("create relative to the pinned directory");
        drop(created);
        drop(parent);

        assert!(pinned_parent.join("file.txt").is_file());
        assert!(!alternate_target.join("file.txt").exists());
        fs::remove_dir(&requested_parent).unwrap();
        fs::remove_dir_all(&sandbox).unwrap();
    }

    #[test]
    fn staging_file_commits_and_cleans_up_relative_to_the_pinned_parent() {
        let sandbox = std::env::temp_dir().join(format!(
            "wingman-stage-file-test-{}-{}",
            std::process::id(),
            Uuid::new_v4().as_simple()
        ));
        fs::create_dir(&sandbox).unwrap();
        fs::write(sandbox.join("destination.txt"), b"old").unwrap();
        let parent = open_verified_root_directory(&sandbox).unwrap();

        let mut committed = create_verified_child_file(
            &parent,
            std::ffi::OsStr::new(".wingman-a.tmp"),
            VerifiedFileCreateModeV1::Staging,
        )
        .unwrap();
        std::io::Write::write_all(&mut committed, b"new").unwrap();
        committed.sync_all().unwrap();
        rename_open_file_relative(
            &committed,
            &parent,
            std::ffi::OsStr::new("destination.txt"),
            true,
            false,
        )
        .unwrap();
        assert_eq!(fs::read(sandbox.join("destination.txt")).unwrap(), b"new");
        assert!(!sandbox.join(".wingman-a.tmp").exists());

        let discarded = create_verified_child_file(
            &parent,
            std::ffi::OsStr::new(".wingman-b.tmp"),
            VerifiedFileCreateModeV1::Staging,
        )
        .unwrap();
        delete_open_file(&discarded).unwrap();
        drop(discarded);
        assert!(!sandbox.join(".wingman-b.tmp").exists());
        drop(committed);
        drop(parent);
        fs::remove_dir_all(&sandbox).unwrap();
    }

    #[test]
    fn pinned_directory_enumeration_is_sorted_and_classifies_reparse_entries() {
        let sandbox = std::env::temp_dir().join(format!(
            "wingman-enumeration-test-{}-{}",
            std::process::id(),
            Uuid::new_v4().as_simple()
        ));
        let outside = sandbox.with_extension("outside");
        fs::create_dir(&sandbox).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(sandbox.join("z.txt"), b"").unwrap();
        fs::write(sandbox.join("A.txt"), b"").unwrap();
        fs::create_dir(sandbox.join("middle")).unwrap();
        create_directory_reparse(&outside, &sandbox.join("link"));
        let directory = open_verified_root_directory(&sandbox).unwrap();

        let entries = list_verified_directory(&directory).unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|entry| (entry.display_name.as_str(), entry.kind))
                .collect::<Vec<_>>(),
            vec![
                ("A.txt", VerifiedDirectoryEntryKindV1::File),
                ("link", VerifiedDirectoryEntryKindV1::ReparsePoint),
                ("middle", VerifiedDirectoryEntryKindV1::Directory),
                ("z.txt", VerifiedDirectoryEntryKindV1::File),
            ]
        );

        drop(directory);
        fs::remove_dir(sandbox.join("link")).unwrap();
        fs::remove_dir_all(&sandbox).unwrap();
        fs::remove_dir_all(&outside).unwrap();
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
