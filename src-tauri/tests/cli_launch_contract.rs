use std::ffi::OsString;
use wingman_lib::app_launch::{
    parse_public_args, PublicLaunchActionV1, PublicLaunchSyntaxErrorV1, RequestedShellV1,
};

#[test]
fn shell_and_start_directory_parse_to_a_typed_window_request() {
    assert_eq!(
        parse_public_args(["--shell", "cmd", "."]),
        Ok(PublicLaunchActionV1::OpenWindow {
            shell: Some(RequestedShellV1::Cmd),
            start_path: Some(OsString::from(".")),
        })
    );
}

#[test]
fn help_is_a_non_gui_action() {
    assert_eq!(
        parse_public_args(["--help"]),
        Ok(PublicLaunchActionV1::Help)
    );
}

#[test]
fn version_is_a_non_gui_action() {
    assert_eq!(
        parse_public_args(["--version"]),
        Ok(PublicLaunchActionV1::Version)
    );
}

#[test]
fn unsupported_option_is_rejected_instead_of_becoming_a_path() {
    assert_eq!(
        parse_public_args(["--shell=cmd"]),
        Err(PublicLaunchSyntaxErrorV1::UnsupportedOption(
            OsString::from("--shell=cmd")
        ))
    );
}

#[test]
fn option_terminator_allows_a_dash_prefixed_start_path() {
    assert_eq!(
        parse_public_args(["--shell", "powershell", "--", "-project"]),
        Ok(PublicLaunchActionV1::OpenWindow {
            shell: Some(RequestedShellV1::WindowsPowerShell),
            start_path: Some(OsString::from("-project")),
        })
    );
}
