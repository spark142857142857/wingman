use crate::interpreter::{ExecutionPlanV1, RedirectModeV1 as PlanRedirectModeV1, StagePlanV1};
use crate::ordered_pipeline::{
    OrderedFinishCauseV1, OrderedFlowV1, OrderedPipelineFaultV1, OrderedPipelineV1,
};
use crate::runner_cancel::RunnerCancellationV1;
use crate::runner_io::{
    prepare_discovered_output, IoPreparationErrorV1, RedirectModeV1, RedirectSpecV1,
};
use crate::runner_readonly::ReadonlyExecutionErrorV1;
use crate::text_stream::{RecordFrameV1, RecordStreamWriterV1, TextStreamWriteErrorV1};
use crate::windows_path::resolve_path_spec;
use std::cmp::Ordering;
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::mem::MaybeUninit;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows_sys::Win32::Globalization::CompareStringOrdinal;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_ARCHIVE, FILE_ATTRIBUTE_COMPRESSED, FILE_ATTRIBUTE_HIDDEN,
    FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_SYSTEM,
};
use windows_sys::Win32::System::Time::{
    FileTimeToSystemTime, SystemTimeToFileTime, SystemTimeToTzSpecificLocalTime,
};

pub const MAX_LS_ENTRIES: usize = 262_144;
pub const MAX_LS_NAME_BYTES: usize = 64 * 1024 * 1024;

struct ListingOptionsV1 {
    include_hidden: bool,
    long: bool,
    human_readable: bool,
}

struct ListingEntryV1 {
    name: String,
    metadata: fs::Metadata,
}

pub fn execute_ls_to<W: Write, E: Write>(
    plan: &ExecutionPlanV1,
    stdout: &mut W,
    stderr: &mut E,
    cancellation: &RunnerCancellationV1,
) -> Option<Result<u8, ReadonlyExecutionErrorV1>> {
    if !matches!(plan.stages.first(), Some(StagePlanV1::ListEntries { .. })) {
        return None;
    }
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(_) => {
            return Some(
                write_diagnostic(
                    stderr,
                    "wingman ls: unable to read current working directory",
                )
                .map(|()| 1),
            )
        }
    };
    Some(execute_ls_with_cwd_to(
        plan,
        &cwd,
        stdout,
        stderr,
        cancellation,
    ))
}

pub fn execute_ls_with_cwd_to<W: Write, E: Write>(
    plan: &ExecutionPlanV1,
    cwd: &Path,
    stdout: &mut W,
    stderr: &mut E,
    cancellation: &RunnerCancellationV1,
) -> Result<u8, ReadonlyExecutionErrorV1> {
    let Some(StagePlanV1::ListEntries {
        path,
        include_hidden,
        long,
        human_readable,
    }) = plan.stages.first()
    else {
        return Err(ReadonlyExecutionErrorV1::UnsupportedPlan);
    };
    if cancellation.is_cancelled() {
        return Ok(130);
    }
    let Some(cwd) = cwd.to_str() else {
        write_diagnostic(
            stderr,
            "wingman ls: current working directory is not valid Unicode",
        )?;
        return Ok(1);
    };
    let target = match path {
        Some(path) => match resolve_path_spec(path, cwd) {
            Ok(path) => path,
            Err(_) => {
                write_diagnostic(stderr, "wingman ls: path cannot be resolved safely")?;
                return Ok(2);
            }
        },
        None => PathBuf::from(cwd),
    };
    let options = ListingOptionsV1 {
        include_hidden: *include_hidden,
        long: *long,
        human_readable: *human_readable,
    };
    let records = match collect_records(&target, &options, cancellation) {
        Ok(records) => records,
        Err(message) => {
            if cancellation.is_cancelled() {
                return Ok(130);
            }
            write_diagnostic(stderr, &format!("wingman ls: {message}"))?;
            return Ok(1);
        }
    };
    if cancellation.is_cancelled() {
        return Ok(130);
    }

    let mut redirected = match &plan.redirect {
        Some(redirect) => {
            let path = match resolve_path_spec(&redirect.path, cwd) {
                Ok(path) => path,
                Err(_) => {
                    write_diagnostic(
                        stderr,
                        "wingman ls: redirection target cannot be resolved safely",
                    )?;
                    return Ok(2);
                }
            };
            let spec = RedirectSpecV1 {
                path,
                mode: match redirect.mode {
                    PlanRedirectModeV1::Overwrite => RedirectModeV1::Overwrite,
                    PlanRedirectModeV1::Append => RedirectModeV1::Append,
                },
            };
            match prepare_discovered_output(&[], &spec) {
                Ok(output) => Some(output),
                Err(IoPreparationErrorV1::OutputReparsePoint) => {
                    write_diagnostic(
                        stderr,
                        "wingman ls: redirection target is or crosses a reparse point",
                    )?;
                    return Ok(2);
                }
                Err(_) => {
                    write_diagnostic(stderr, "wingman ls: redirection target cannot be opened")?;
                    return Ok(1);
                }
            }
        }
        None => None,
    };
    let writer: &mut dyn Write = match redirected.as_mut() {
        Some(output) => output,
        None => stdout,
    };
    let mut sink = RecordStreamWriterV1::new(writer);
    let source_paths = path.as_ref().into_iter().collect::<Vec<_>>();
    let mut pipeline = OrderedPipelineV1::new(plan, &mut sink, cancellation, &source_paths)
        .map_err(map_setup_fault)?;
    let mut stopped = pipeline.starts_stopped();
    let mut fault = None;
    if !stopped {
        for record in records {
            match pipeline.push(record, 0) {
                Ok(OrderedFlowV1::Continue) => {}
                Ok(OrderedFlowV1::StopUpstream) => {
                    stopped = true;
                    break;
                }
                Err(error) => {
                    fault = Some(error);
                    break;
                }
            }
        }
    }
    if fault.is_none() && !cancellation.is_cancelled() {
        if let Err(error) = pipeline.finish(if stopped {
            OrderedFinishCauseV1::UpstreamStopped
        } else {
            OrderedFinishCauseV1::Complete
        }) {
            fault = Some(error);
        }
    }
    let grep_matched = pipeline.final_search_matched();
    drop(pipeline);
    if cancellation.is_cancelled() || matches!(fault, Some(OrderedPipelineFaultV1::Cancelled)) {
        return Ok(130);
    }
    if let Some(error) = fault {
        let message = match error {
            OrderedPipelineFaultV1::TailResource => "wingman tail: buffer resource limit exceeded",
            OrderedPipelineFaultV1::SortResource => {
                "wingman sort: materialization resource limit exceeded"
            }
            OrderedPipelineFaultV1::InvalidNumeric => "wingman sort: invalid numeric data",
            OrderedPipelineFaultV1::Output { .. } if plan.redirect.is_some() => {
                write_diagnostic(
                    stderr,
                    "wingman ls: redirection output failed and may be partial",
                )?;
                return Ok(1);
            }
            OrderedPipelineFaultV1::Output { kind } => {
                return Err(ReadonlyExecutionErrorV1::Output { kind })
            }
            OrderedPipelineFaultV1::Overflow => {
                return Err(ReadonlyExecutionErrorV1::Output {
                    kind: std::io::ErrorKind::OutOfMemory,
                })
            }
            OrderedPipelineFaultV1::Unsupported | OrderedPipelineFaultV1::Cancelled => {
                return Err(ReadonlyExecutionErrorV1::UnsupportedPlan)
            }
        };
        write_diagnostic(stderr, message)?;
        return Ok(1);
    }
    if let Err(error) = sink.finish().map_err(map_sink_error) {
        if plan.redirect.is_some() {
            write_diagnostic(
                stderr,
                "wingman ls: redirection output failed and may be partial",
            )?;
            return Ok(1);
        }
        return Err(error);
    }
    Ok(if grep_matched == Some(false) { 1 } else { 0 })
}

fn collect_records(
    target: &Path,
    options: &ListingOptionsV1,
    cancellation: &RunnerCancellationV1,
) -> Result<Vec<RecordFrameV1>, &'static str> {
    let followed = fs::metadata(target).map_err(|_| "path cannot be inspected")?;
    let mut entries = Vec::new();
    let mut name_bytes = 0usize;
    if followed.is_dir() {
        let directory = fs::read_dir(target).map_err(|_| "directory cannot be read")?;
        for entry in directory {
            if cancellation.is_cancelled() {
                return Ok(Vec::new());
            }
            let entry = entry.map_err(|_| "directory entry cannot be read")?;
            let name = unicode_name(&entry.file_name())?;
            let metadata = fs::symlink_metadata(entry.path())
                .map_err(|_| "directory entry cannot be inspected")?;
            let attributes = metadata.file_attributes();
            if !options.include_hidden
                && attributes & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM) != 0
            {
                continue;
            }
            name_bytes = name_bytes.saturating_add(name.len());
            if entries.len() >= MAX_LS_ENTRIES || name_bytes > MAX_LS_NAME_BYTES {
                return Err("directory listing resource limit exceeded");
            }
            entries.push(ListingEntryV1 { name, metadata });
        }
    } else {
        let name = target
            .file_name()
            .ok_or("path has no displayable filename")
            .and_then(unicode_name)?;
        let metadata = fs::symlink_metadata(target).map_err(|_| "path cannot be inspected")?;
        entries.push(ListingEntryV1 { name, metadata });
    }
    entries.sort_by(|left, right| compare_names(&left.name, &right.name));
    entries
        .into_iter()
        .map(|entry| {
            let text = if options.long {
                format_long(&entry, options.human_readable)?
            } else {
                entry.name
            };
            Ok(RecordFrameV1 {
                text,
                terminated: true,
            })
        })
        .collect()
}

fn unicode_name(name: &OsStr) -> Result<String, &'static str> {
    name.to_str()
        .map(str::to_string)
        .ok_or("filename is not valid Unicode")
}

fn format_long(entry: &ListingEntryV1, human: bool) -> Result<String, &'static str> {
    let attributes = entry.metadata.file_attributes();
    let kind = if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        'l'
    } else if entry.metadata.is_dir() {
        'd'
    } else if entry.metadata.is_file() {
        '-'
    } else {
        '?'
    };
    let mask = [
        (FILE_ATTRIBUTE_READONLY, 'R'),
        (FILE_ATTRIBUTE_ARCHIVE, 'A'),
        (FILE_ATTRIBUTE_SYSTEM, 'S'),
        (FILE_ATTRIBUTE_HIDDEN, 'H'),
        (FILE_ATTRIBUTE_COMPRESSED, 'C'),
    ]
    .into_iter()
    .map(|(flag, letter)| if attributes & flag != 0 { letter } else { '-' })
    .collect::<String>();
    let size = if kind == '-' {
        if human {
            human_size(entry.metadata.file_size())
        } else {
            entry.metadata.file_size().to_string()
        }
    } else {
        "-".to_string()
    };
    let modified = format_local_filetime(entry.metadata.last_write_time())?;
    Ok(format!("{kind} {mask} {size} {modified} {}", entry.name))
}

fn human_size(size: u64) -> String {
    if size < 1024 {
        return format!("{size}B");
    }
    const UNITS: [&str; 6] = ["KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    let size = u128::from(size);
    let mut divisor = 1024u128;
    let mut unit = 0usize;
    loop {
        let tenths = (size.saturating_mul(10) + divisor / 2) / divisor;
        if tenths < 10_240 || unit + 1 == UNITS.len() {
            return format!("{}.{:01}{}", tenths / 10, tenths % 10, UNITS[unit]);
        }
        divisor *= 1024;
        unit += 1;
    }
}

fn compare_names(left: &str, right: &str) -> Ordering {
    compare_ordinal(left, right, true).then_with(|| compare_ordinal(left, right, false))
}

fn compare_ordinal(left: &str, right: &str, ignore_case: bool) -> Ordering {
    let left = left.encode_utf16().collect::<Vec<_>>();
    let right = right.encode_utf16().collect::<Vec<_>>();
    let result = unsafe {
        CompareStringOrdinal(
            left.as_ptr(),
            left.len() as i32,
            right.as_ptr(),
            right.len() as i32,
            ignore_case.into(),
        )
    };
    match result {
        1 => Ordering::Less,
        3 => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

fn format_local_filetime(raw: u64) -> Result<String, &'static str> {
    let whole_seconds = raw - raw % 10_000_000;
    let filetime = FILETIME {
        dwLowDateTime: whole_seconds as u32,
        dwHighDateTime: (whole_seconds >> 32) as u32,
    };
    let utc = unsafe {
        let mut value = MaybeUninit::<SYSTEMTIME>::uninit();
        if FileTimeToSystemTime(&filetime, value.as_mut_ptr()) == 0 {
            return Err("last-write time cannot be converted");
        }
        value.assume_init()
    };
    let local = unsafe {
        let mut value = MaybeUninit::<SYSTEMTIME>::uninit();
        if SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, value.as_mut_ptr()) == 0 {
            return Err("last-write time zone conversion failed");
        }
        value.assume_init()
    };
    let utc_ticks = systemtime_ticks(&utc).ok_or("last-write UTC conversion failed")?;
    let local_ticks = systemtime_ticks(&local).ok_or("last-write local conversion failed")?;
    let offset_minutes = (i128::from(local_ticks) - i128::from(utc_ticks)) / 600_000_000;
    if !(-24 * 60..=24 * 60).contains(&offset_minutes) {
        return Err("last-write time zone offset is invalid");
    }
    let sign = if offset_minutes < 0 { '-' } else { '+' };
    let offset = offset_minutes.unsigned_abs();
    Ok(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{sign}{:02}:{:02}",
        local.wYear,
        local.wMonth,
        local.wDay,
        local.wHour,
        local.wMinute,
        local.wSecond,
        offset / 60,
        offset % 60
    ))
}

fn systemtime_ticks(value: &SYSTEMTIME) -> Option<u64> {
    unsafe {
        let mut filetime = MaybeUninit::<FILETIME>::uninit();
        (SystemTimeToFileTime(value, filetime.as_mut_ptr()) != 0).then(|| {
            let filetime = filetime.assume_init();
            (u64::from(filetime.dwHighDateTime) << 32) | u64::from(filetime.dwLowDateTime)
        })
    }
}

fn write_diagnostic(writer: &mut impl Write, value: &str) -> Result<(), ReadonlyExecutionErrorV1> {
    writer
        .write_all(value.as_bytes())
        .and_then(|()| writer.write_all(b"\r\n"))
        .and_then(|()| writer.flush())
        .map_err(|error| ReadonlyExecutionErrorV1::Output { kind: error.kind() })
}

fn map_setup_fault(error: OrderedPipelineFaultV1) -> ReadonlyExecutionErrorV1 {
    match error {
        OrderedPipelineFaultV1::Output { kind } => ReadonlyExecutionErrorV1::Output { kind },
        _ => ReadonlyExecutionErrorV1::UnsupportedPlan,
    }
}

fn map_sink_error(error: TextStreamWriteErrorV1) -> ReadonlyExecutionErrorV1 {
    match error {
        TextStreamWriteErrorV1::Encode(_) => ReadonlyExecutionErrorV1::Output {
            kind: std::io::ErrorKind::InvalidData,
        },
        TextStreamWriteErrorV1::Io { kind } => ReadonlyExecutionErrorV1::Output { kind },
    }
}
