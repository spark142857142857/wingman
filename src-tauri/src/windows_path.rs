use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const MAX_RESOLVED_PATH_UTF16_UNITS: usize = 4096;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PathKindV1 {
    Relative,
    DriveAbsolute,
    UncAbsolute,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ValidatedPathSpecV1 {
    pub original: String,
    pub kind: PathKindV1,
    pub components: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathValidationErrorV1 {
    Empty,
    TooLong,
    DriveRelative,
    RootRelative,
    SlashPrefixedUnc,
    DeviceNamespace,
    AlternateDataStream,
    Wildcard,
    InvalidCharacter,
    AmbiguousComponent,
    ReservedDevice,
    UncMissingServer,
    UncMissingShare,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathResolutionErrorV1 {
    InvalidSpec,
    InvalidCurrentDirectory,
    TraversalAboveRoot,
    TooLong,
}

pub fn validate_path_value(input: &str) -> Result<ValidatedPathSpecV1, PathValidationErrorV1> {
    if input.is_empty() {
        return Err(PathValidationErrorV1::Empty);
    }
    if input.encode_utf16().count() > MAX_RESOLVED_PATH_UTF16_UNITS {
        return Err(PathValidationErrorV1::TooLong);
    }
    let lowercase = input.to_ascii_lowercase();
    if lowercase.starts_with(r"\\?\")
        || lowercase.starts_with(r"\\.\")
        || lowercase.starts_with(r"\??\")
    {
        return Err(PathValidationErrorV1::DeviceNamespace);
    }
    if input.starts_with("//") {
        return Err(PathValidationErrorV1::SlashPrefixedUnc);
    }

    let bytes = input.as_bytes();
    let (kind, raw_components, drive_colon_index) =
        if let Some(remainder) = input.strip_prefix(r"\\") {
            if remainder.starts_with(['\\', '/']) {
                return Err(PathValidationErrorV1::UncMissingServer);
            }
            let components = split_components(remainder);
            if components.is_empty() {
                return Err(PathValidationErrorV1::UncMissingServer);
            }
            if components.len() < 2 {
                return Err(PathValidationErrorV1::UncMissingShare);
            }
            (PathKindV1::UncAbsolute, components, None)
        } else if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            if bytes.len() == 2 || !matches!(bytes[2], b'\\' | b'/') {
                return Err(PathValidationErrorV1::DriveRelative);
            }
            (
                PathKindV1::DriveAbsolute,
                split_components(&input[3..]),
                Some(1),
            )
        } else {
            if input.starts_with(['\\', '/']) {
                return Err(PathValidationErrorV1::RootRelative);
            }
            (PathKindV1::Relative, split_components(input), None)
        };

    for (index, character) in input.char_indices() {
        if character == ':' && drive_colon_index != Some(index) {
            return Err(PathValidationErrorV1::AlternateDataStream);
        }
        if matches!(character, '*' | '?') {
            return Err(PathValidationErrorV1::Wildcard);
        }
        if (character < ' ' || matches!(character, '<' | '>' | '"' | '|'))
            && !matches!(character, '\\' | '/')
        {
            return Err(PathValidationErrorV1::InvalidCharacter);
        }
    }
    for component in &raw_components {
        validate_component(component)?;
    }

    Ok(ValidatedPathSpecV1 {
        original: input.to_string(),
        kind,
        components: raw_components.into_iter().map(str::to_string).collect(),
    })
}

pub fn resolve_path_spec(
    spec: &ValidatedPathSpecV1,
    inherited_cwd: &str,
) -> Result<PathBuf, PathResolutionErrorV1> {
    if validate_path_value(&spec.original).ok().as_ref() != Some(spec) {
        return Err(PathResolutionErrorV1::InvalidSpec);
    }
    let cwd = validate_path_value(inherited_cwd)
        .map_err(|_| PathResolutionErrorV1::InvalidCurrentDirectory)?;
    let (mut root, mut resolved_components) =
        absolute_root_and_components(&cwd).ok_or(PathResolutionErrorV1::InvalidCurrentDirectory)?;

    let spec_components = match spec.kind {
        PathKindV1::Relative => spec.components.as_slice(),
        PathKindV1::DriveAbsolute => {
            root = ResolutionRoot::Drive(spec.original[..2].to_string());
            resolved_components.clear();
            spec.components.as_slice()
        }
        PathKindV1::UncAbsolute => {
            root = ResolutionRoot::Unc {
                server: spec.components[0].clone(),
                share: spec.components[1].clone(),
            };
            resolved_components.clear();
            &spec.components[2..]
        }
    };
    fold_components(&mut resolved_components, spec_components)?;

    let mut native = match root {
        ResolutionRoot::Drive(drive) => format!("{drive}\\"),
        ResolutionRoot::Unc { server, share } => format!(r"\\{server}\{share}"),
    };
    if !resolved_components.is_empty() {
        if !native.ends_with('\\') {
            native.push('\\');
        }
        native.push_str(&resolved_components.join("\\"));
    }
    if native.encode_utf16().count() > MAX_RESOLVED_PATH_UTF16_UNITS {
        return Err(PathResolutionErrorV1::TooLong);
    }
    Ok(PathBuf::from(native))
}

enum ResolutionRoot {
    Drive(String),
    Unc { server: String, share: String },
}

fn absolute_root_and_components(
    spec: &ValidatedPathSpecV1,
) -> Option<(ResolutionRoot, Vec<String>)> {
    match spec.kind {
        PathKindV1::Relative => None,
        PathKindV1::DriveAbsolute => {
            let mut components = Vec::new();
            fold_components(&mut components, &spec.components).ok()?;
            Some((
                ResolutionRoot::Drive(spec.original[..2].to_string()),
                components,
            ))
        }
        PathKindV1::UncAbsolute => {
            let mut components = Vec::new();
            fold_components(&mut components, &spec.components[2..]).ok()?;
            Some((
                ResolutionRoot::Unc {
                    server: spec.components[0].clone(),
                    share: spec.components[1].clone(),
                },
                components,
            ))
        }
    }
}

fn fold_components(
    destination: &mut Vec<String>,
    components: &[String],
) -> Result<(), PathResolutionErrorV1> {
    for component in components {
        match component.as_str() {
            "." => {}
            ".." => {
                if destination.pop().is_none() {
                    return Err(PathResolutionErrorV1::TraversalAboveRoot);
                }
            }
            _ => destination.push(component.clone()),
        }
    }
    Ok(())
}

fn split_components(path: &str) -> Vec<&str> {
    path.split(['\\', '/'])
        .filter(|component| !component.is_empty())
        .collect()
}

fn validate_component(component: &str) -> Result<(), PathValidationErrorV1> {
    if component != "."
        && component != ".."
        && (component.ends_with('.') || component.ends_with(' '))
    {
        return Err(PathValidationErrorV1::AmbiguousComponent);
    }
    let base = component.split('.').next().unwrap_or_default();
    let uppercase = base.to_ascii_uppercase();
    let numbered_device = uppercase.len() == 4
        && (uppercase.starts_with("COM") || uppercase.starts_with("LPT"))
        && matches!(uppercase.as_bytes()[3], b'1'..=b'9');
    if matches!(
        uppercase.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || numbered_device
    {
        return Err(PathValidationErrorV1::ReservedDevice);
    }
    Ok(())
}
