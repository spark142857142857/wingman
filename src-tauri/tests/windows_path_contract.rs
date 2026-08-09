use wingman_lib::windows_path::{
    resolve_path_spec, validate_path_value, PathKindV1, PathResolutionErrorV1,
    PathValidationErrorV1, ValidatedPathSpecV1,
};

#[test]
fn accepted_path_forms_are_classified_without_host_cwd_resolution() {
    for (input, kind, components) in [
        ("file.txt", PathKindV1::Relative, vec!["file.txt"]),
        (
            r".\src\main.ts",
            PathKindV1::Relative,
            vec![".", "src", "main.ts"],
        ),
        (
            "../src/main.ts",
            PathKindV1::Relative,
            vec!["..", "src", "main.ts"],
        ),
        (
            "C:\\work\\한글 파일.txt",
            PathKindV1::DriveAbsolute,
            vec!["work", "한글 파일.txt"],
        ),
        (
            "C:/work/project",
            PathKindV1::DriveAbsolute,
            vec!["work", "project"],
        ),
        (
            r"\\server\share\folder",
            PathKindV1::UncAbsolute,
            vec!["server", "share", "folder"],
        ),
    ] {
        assert_eq!(
            validate_path_value(input).unwrap(),
            ValidatedPathSpecV1 {
                original: input.to_string(),
                kind,
                components: components.into_iter().map(str::to_string).collect(),
            },
            "input: {input}"
        );
    }
}

#[test]
fn ambiguous_namespace_stream_wildcard_and_root_relative_forms_are_rejected() {
    for (input, expected) in [
        ("", PathValidationErrorV1::Empty),
        ("C:relative.txt", PathValidationErrorV1::DriveRelative),
        (r"\root-relative.txt", PathValidationErrorV1::RootRelative),
        ("/home/user/file", PathValidationErrorV1::RootRelative),
        (
            "//server/share/file",
            PathValidationErrorV1::SlashPrefixedUnc,
        ),
        (r"\\?\C:\file.txt", PathValidationErrorV1::DeviceNamespace),
        (
            r"\\.\PhysicalDrive0",
            PathValidationErrorV1::DeviceNamespace,
        ),
        (r"\??\C:\file.txt", PathValidationErrorV1::DeviceNamespace),
        (
            "file.txt:stream",
            PathValidationErrorV1::AlternateDataStream,
        ),
        ("*.log", PathValidationErrorV1::Wildcard),
        ("src/?.ts", PathValidationErrorV1::Wildcard),
        ("folder/name.", PathValidationErrorV1::AmbiguousComponent),
        ("folder/name ", PathValidationErrorV1::AmbiguousComponent),
        ("folder/NUL.txt", PathValidationErrorV1::ReservedDevice),
        ("folder/com1.log", PathValidationErrorV1::ReservedDevice),
        (r"\\server", PathValidationErrorV1::UncMissingShare),
    ] {
        assert_eq!(validate_path_value(input), Err(expected), "input: {input}");
    }
}

#[test]
fn literals_dot_components_and_unicode_spelling_are_preserved() {
    let input = ".\\~\\$HOME\\e\u{301}\\..\\file";
    let validated = validate_path_value(input).unwrap();
    assert_eq!(validated.original, input);
    assert_eq!(
        validated.components,
        vec![".", "~", "$HOME", "e\u{301}", "..", "file"]
    );
}

#[test]
fn runner_resolution_uses_its_inherited_cwd_and_folds_dot_components() {
    let relative = validate_path_value(r"..\logs\.\app.txt").unwrap();
    assert_eq!(
        resolve_path_spec(&relative, r"C:\work\project").unwrap(),
        std::path::PathBuf::from(r"C:\work\logs\app.txt")
    );

    let drive = validate_path_value(r"D:/data/../logs/app.txt").unwrap();
    assert_eq!(
        resolve_path_spec(&drive, r"C:\ignored").unwrap(),
        std::path::PathBuf::from(r"D:\logs\app.txt")
    );

    let unc = validate_path_value(r"\\server\share\a\..\b").unwrap();
    assert_eq!(
        resolve_path_spec(&unc, r"C:\ignored").unwrap(),
        std::path::PathBuf::from(r"\\server\share\b")
    );
}

#[test]
fn runner_revalidates_the_serialized_spec_and_rejects_root_escape() {
    let escape = validate_path_value(r"..\..\file.txt").unwrap();
    assert_eq!(
        resolve_path_spec(&escape, r"C:\one"),
        Err(PathResolutionErrorV1::TraversalAboveRoot)
    );

    let mut tampered = validate_path_value(r"safe\file.txt").unwrap();
    tampered.components = vec!["different.txt".to_string()];
    assert_eq!(
        resolve_path_spec(&tampered, r"C:\work"),
        Err(PathResolutionErrorV1::InvalidSpec)
    );

    let relative = validate_path_value("file.txt").unwrap();
    assert_eq!(
        resolve_path_spec(&relative, "not-an-absolute-cwd"),
        Err(PathResolutionErrorV1::InvalidCurrentDirectory)
    );
}
