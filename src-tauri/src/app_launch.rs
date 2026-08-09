use std::ffi::{OsStr, OsString};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
