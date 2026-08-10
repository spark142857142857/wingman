use crate::runner_cancel::RunnerCancellationV1;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

pub fn execute_which_to<W: Write, E: Write>(
    name: &str,
    stdout: &mut W,
    stderr: &mut E,
    cancellation: &RunnerCancellationV1,
) -> io::Result<u8> {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(_) => {
            write_diagnostic(
                stderr,
                "wingman which: unable to read current working directory",
            )?;
            return Ok(1);
        }
    };
    execute_which_with_snapshot_to(
        name,
        &cwd,
        std::env::var_os("PATH").as_deref(),
        std::env::var_os("PATHEXT").as_deref(),
        stdout,
        stderr,
        cancellation,
    )
}

pub fn execute_which_with_snapshot_to<W: Write, E: Write>(
    name: &str,
    cwd: &Path,
    path: Option<&OsStr>,
    pathext: Option<&OsStr>,
    stdout: &mut W,
    stderr: &mut E,
    cancellation: &RunnerCancellationV1,
) -> io::Result<u8> {
    if cancellation.is_cancelled() {
        return Ok(130);
    }
    let Some(cwd_text) = cwd.to_str() else {
        write_diagnostic(
            stderr,
            "wingman which: current working directory is not valid Unicode",
        )?;
        return Ok(1);
    };
    let cwd = normalize_absolute(Path::new(cwd_text));
    if !cwd.is_absolute() {
        write_diagnostic(
            stderr,
            "wingman which: current working directory is not absolute",
        )?;
        return Ok(1);
    }

    let extensions = parse_pathext(pathext);
    let candidates = match executable_candidates(name, &extensions) {
        Some(candidates) => candidates,
        None => return Ok(1),
    };
    let mut directories = vec![cwd.clone()];
    if let Some(path) = path.and_then(OsStr::to_str) {
        directories.extend(path.split(';').map(|entry| {
            let entry = strip_boundary_quotes(entry);
            if entry.is_empty() {
                cwd.clone()
            } else {
                let path = Path::new(entry);
                if path.is_absolute() {
                    normalize_absolute(path)
                } else {
                    normalize_absolute(&cwd.join(path))
                }
            }
        }));
    }

    let mut seen = HashSet::new();
    for directory in directories {
        if cancellation.is_cancelled() {
            return Ok(130);
        }
        let key = directory.to_string_lossy().to_lowercase();
        if !seen.insert(key) {
            continue;
        }
        match std::fs::metadata(&directory) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) | Err(_) => continue,
        }
        for candidate in &candidates {
            if cancellation.is_cancelled() {
                return Ok(130);
            }
            let candidate = directory.join(candidate);
            match std::fs::metadata(&candidate) {
                Ok(metadata) if !metadata.is_dir() => {
                    let normalized = normalize_absolute(&candidate);
                    writeln_crlf(stdout, &normalized.to_string_lossy())?;
                    return Ok(0);
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(_) => {
                    write_diagnostic(
                        stderr,
                        "wingman which: executable candidate cannot be inspected",
                    )?;
                    return Ok(1);
                }
            }
        }
    }
    Ok(1)
}

fn parse_pathext(value: Option<&OsStr>) -> Vec<String> {
    let text = value.and_then(OsStr::to_str).unwrap_or(DEFAULT_PATHEXT);
    let mut extensions = Vec::new();
    let mut seen = HashSet::new();
    for raw in text.split(';') {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let extension = if raw.starts_with('.') {
            raw.to_string()
        } else {
            format!(".{raw}")
        };
        if extension.len() <= 1
            || extension.contains(['\\', '/', ':', '*', '?', '<', '>', '"', '|'])
            || extension.chars().any(char::is_control)
        {
            continue;
        }
        let key = extension.to_lowercase();
        if seen.insert(key) {
            extensions.push(extension);
        }
    }
    if extensions.is_empty() {
        return DEFAULT_PATHEXT.split(';').map(str::to_string).collect();
    }
    extensions
}

fn executable_candidates(name: &str, extensions: &[String]) -> Option<Vec<String>> {
    let extension = name
        .rfind('.')
        .filter(|index| *index > 0 && index + 1 < name.len())
        .map(|index| &name[index..]);
    match extension {
        Some(extension) => extensions
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(extension))
            .then(|| vec![name.to_string()]),
        None => Some(
            extensions
                .iter()
                .map(|extension| format!("{name}{extension}"))
                .collect(),
        ),
    }
}

fn strip_boundary_quotes(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
}

fn normalize_absolute(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn write_diagnostic(writer: &mut impl Write, value: &str) -> io::Result<()> {
    writeln_crlf(writer, value)
}

fn writeln_crlf(writer: &mut impl Write, value: &str) -> io::Result<()> {
    writer.write_all(value.as_bytes())?;
    writer.write_all(b"\r\n")?;
    writer.flush()
}
