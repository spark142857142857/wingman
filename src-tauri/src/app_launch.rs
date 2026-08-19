use crate::windows_path::{
    resolve_path_spec, validate_path_value, PathResolutionErrorV1, PathValidationErrorV1,
};
use serde::{Deserialize, Serialize};
use std::ffi::{OsStr, OsString};
use std::os::windows::fs::MetadataExt;
use std::path::Path;
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RequestedShellV1 {
    Cmd,
    WindowsPowerShell,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PublicLaunchActionV1 {
    Help,
    Version,
    OpenWindow {
        shell: Option<RequestedShellV1>,
        start_path: Option<OsString>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PublicLaunchSyntaxErrorV1 {
    MissingShell,
    UnsupportedShell(OsString),
    UnsupportedOption(OsString),
    TooManyOperands,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuiLaunchRequestV1 {
    pub shell: RequestedShellV1,
    pub start_directory: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolvedPublicActionV1 {
    Help,
    Version,
    OpenWindow(GuiLaunchRequestV1),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PublicLaunchResolutionErrorV1 {
    CurrentDirectoryNotUnicode,
    StartPathNotUnicode,
    InvalidCurrentDirectory(PathValidationErrorV1),
    InvalidStartPath(PathValidationErrorV1),
    Resolution(PathResolutionErrorV1),
    Missing,
    NotDirectory,
    ReparsePoint,
    Metadata,
}

pub fn parse_public_args<I, S>(args: I) -> Result<PublicLaunchActionV1, PublicLaunchSyntaxErrorV1>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let mut args = args.into_iter().map(Into::into).peekable();
    let mut shell = None;

    if args
        .peek()
        .is_some_and(|argument| argument == OsStr::new("--help"))
    {
        args.next();
        if args.next().is_some() {
            return Err(PublicLaunchSyntaxErrorV1::TooManyOperands);
        }
        return Ok(PublicLaunchActionV1::Help);
    }

    if args
        .peek()
        .is_some_and(|argument| argument == OsStr::new("--version"))
    {
        args.next();
        if args.next().is_some() {
            return Err(PublicLaunchSyntaxErrorV1::TooManyOperands);
        }
        return Ok(PublicLaunchActionV1::Version);
    }

    if args
        .peek()
        .is_some_and(|argument| argument == OsStr::new("--shell"))
    {
        args.next();
        let value = args.next().ok_or(PublicLaunchSyntaxErrorV1::MissingShell)?;
        shell = Some(match value.as_os_str() {
            value if value == OsStr::new("cmd") => RequestedShellV1::Cmd,
            value if value == OsStr::new("powershell") => RequestedShellV1::WindowsPowerShell,
            _ => return Err(PublicLaunchSyntaxErrorV1::UnsupportedShell(value)),
        });
    }

    let options_terminated = args
        .peek()
        .is_some_and(|argument| argument == OsStr::new("--"));
    if options_terminated {
        args.next();
    } else if let Some(option) = args.peek().filter(|argument| {
        argument
            .to_str()
            .is_some_and(|value| value.starts_with('-'))
    }) {
        return Err(PublicLaunchSyntaxErrorV1::UnsupportedOption(option.clone()));
    }

    let start_path = args.next();
    if args.next().is_some() {
        return Err(PublicLaunchSyntaxErrorV1::TooManyOperands);
    }

    Ok(PublicLaunchActionV1::OpenWindow { shell, start_path })
}

pub fn resolve_public_action(
    action: PublicLaunchActionV1,
    inherited_cwd: &Path,
) -> Result<ResolvedPublicActionV1, PublicLaunchResolutionErrorV1> {
    let (shell, start_path) = match action {
        PublicLaunchActionV1::Help => return Ok(ResolvedPublicActionV1::Help),
        PublicLaunchActionV1::Version => return Ok(ResolvedPublicActionV1::Version),
        PublicLaunchActionV1::OpenWindow { shell, start_path } => (shell, start_path),
    };

    let inherited_cwd = inherited_cwd
        .to_str()
        .ok_or(PublicLaunchResolutionErrorV1::CurrentDirectoryNotUnicode)?;
    validate_path_value(inherited_cwd)
        .map_err(PublicLaunchResolutionErrorV1::InvalidCurrentDirectory)?;

    let resolved = match start_path {
        Some(start_path) => {
            let start_path = start_path
                .to_str()
                .ok_or(PublicLaunchResolutionErrorV1::StartPathNotUnicode)?;
            let spec = validate_path_value(start_path)
                .map_err(PublicLaunchResolutionErrorV1::InvalidStartPath)?;
            resolve_path_spec(&spec, inherited_cwd)
                .map_err(PublicLaunchResolutionErrorV1::Resolution)?
        }
        None => inherited_cwd.into(),
    };

    let metadata = std::fs::symlink_metadata(&resolved).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => PublicLaunchResolutionErrorV1::Missing,
        _ => PublicLaunchResolutionErrorV1::Metadata,
    })?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(PublicLaunchResolutionErrorV1::ReparsePoint);
    }
    if !metadata.is_dir() {
        return Err(PublicLaunchResolutionErrorV1::NotDirectory);
    }

    Ok(ResolvedPublicActionV1::OpenWindow(GuiLaunchRequestV1 {
        shell: shell.unwrap_or(RequestedShellV1::WindowsPowerShell),
        start_directory: resolved.to_string_lossy().into_owned(),
    }))
}
