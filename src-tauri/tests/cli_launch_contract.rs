use std::ffi::OsString;
use std::fs;
use wingman_lib::app_launch::{
    parse_public_args, resolve_public_action, GuiLaunchRequestV1, PublicLaunchActionV1,
    PublicLaunchResolutionErrorV1, PublicLaunchSyntaxErrorV1, RequestedShellV1,
    ResolvedPublicActionV1,
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

#[test]
fn relative_unicode_directory_resolves_from_the_callers_cwd() {
    let sandbox = test_directory("unicode");
    let child = sandbox.join("한글 project");
    fs::create_dir(&child).unwrap();

    let resolved = resolve_public_action(
        parse_public_args(["--shell", "cmd", "한글 project"]).unwrap(),
        &sandbox,
    )
    .unwrap();

    assert_eq!(
        resolved,
        ResolvedPublicActionV1::OpenWindow(GuiLaunchRequestV1 {
            shell: RequestedShellV1::Cmd,
            start_directory: child.to_string_lossy().into_owned(),
        })
    );
    fs::remove_dir_all(sandbox).unwrap();
}

#[test]
fn omitted_path_and_shell_use_caller_cwd_and_powershell() {
    let sandbox = test_directory("default");
    let resolved =
        resolve_public_action(parse_public_args([] as [&str; 0]).unwrap(), &sandbox).unwrap();

    assert_eq!(
        resolved,
        ResolvedPublicActionV1::OpenWindow(GuiLaunchRequestV1 {
            shell: RequestedShellV1::WindowsPowerShell,
            start_directory: sandbox.to_string_lossy().into_owned(),
        })
    );
    fs::remove_dir_all(sandbox).unwrap();
}

#[test]
fn missing_file_and_ambiguous_paths_fail_before_gui_launch() {
    let sandbox = test_directory("invalid");
    let file = sandbox.join("file.txt");
    fs::write(&file, b"not a directory").unwrap();

    assert_eq!(
        resolve_public_action(parse_public_args(["missing"]).unwrap(), &sandbox),
        Err(PublicLaunchResolutionErrorV1::Missing)
    );
    assert_eq!(
        resolve_public_action(parse_public_args(["file.txt"]).unwrap(), &sandbox),
        Err(PublicLaunchResolutionErrorV1::NotDirectory)
    );
    assert!(matches!(
        resolve_public_action(parse_public_args([r"C:relative"]).unwrap(), &sandbox),
        Err(PublicLaunchResolutionErrorV1::InvalidStartPath(_))
    ));
    assert!(matches!(
        resolve_public_action(parse_public_args([r"\\?\C:\Windows"]).unwrap(), &sandbox),
        Err(PublicLaunchResolutionErrorV1::InvalidStartPath(_))
    ));

    fs::remove_dir_all(sandbox).unwrap();
}

fn test_directory(label: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "wingman-cli-{label}-{}",
        uuid::Uuid::new_v4().as_simple()
    ));
    fs::create_dir(&path).unwrap();
    path
}
