use crate::app_launch::GuiLaunchRequestV1;
use crate::runner_io::capture_file_identity;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::ffi::{c_void, OsStr};
use std::fs::File;
use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::Path;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;
use windows_sys::Win32::Foundation::{
    GetLastError, ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, GetFileType, ReadFile, WriteFile, FILE_ATTRIBUTE_NORMAL,
    FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_TYPE_PIPE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
};
use windows_sys::Win32::System::Console::{SetConsoleCtrlHandler, CTRL_C_EVENT};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeServerProcessId, PeekNamedPipe,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, InitializeProcThreadAttributeList, OpenProcess,
    QueryFullProcessImageNameW, TerminateProcess, UpdateProcThreadAttribute,
    CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, EXTENDED_STARTUPINFO_PRESENT, PROCESS_INFORMATION,
    PROCESS_QUERY_LIMITED_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, STARTUPINFOEXW,
};

const HANDOFF_VERSION: u8 = 1;
const MAX_HANDOFF_FRAME_BYTES: usize = 16 * 1024;
const HANDOFF_TIMEOUT: Duration = Duration::from_secs(10);
const INTERNAL_GUI_MARKER: &str = "--wingman-internal-gui";
static LAUNCH_CANCELLED: AtomicBool = AtomicBool::new(false);

#[derive(Debug)]
pub(crate) enum HandoffErrorV1 {
    InvalidInternalInvocation,
    InvalidParent,
    InvalidMessage,
    Cancelled,
    Timeout,
    Io(io::Error),
}

impl std::fmt::Display for HandoffErrorV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInternalInvocation => {
                formatter.write_str("invalid internal GUI invocation")
            }
            Self::InvalidParent => formatter.write_str("internal GUI parent validation failed"),
            Self::InvalidMessage => formatter.write_str("invalid GUI handoff message"),
            Self::Cancelled => formatter.write_str("GUI handoff was cancelled"),
            Self::Timeout => formatter.write_str("GUI handoff timed out"),
            Self::Io(error) => write!(formatter, "GUI handoff I/O failed: {error}"),
        }
    }
}

impl From<io::Error> for HandoffErrorV1 {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HandoffRequestV1 {
    version: u8,
    nonce: String,
    parent_process_id: u32,
    launch: GuiLaunchRequestV1,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum HandoffStatusV1 {
    Ready,
    Failed,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HandoffResponseV1 {
    version: u8,
    nonce: String,
    status: HandoffStatusV1,
    diagnostic: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HandoffAckV1 {
    version: u8,
    nonce: String,
    acknowledged: bool,
}

pub(crate) enum LauncherOutcomeV1 {
    Ready,
    Failed(String),
}

pub(crate) struct LauncherCtrlHandlerGuard {
    installed: bool,
}

impl LauncherCtrlHandlerGuard {
    pub(crate) fn install() -> Self {
        LAUNCH_CANCELLED.store(false, Ordering::Release);
        let installed = unsafe { SetConsoleCtrlHandler(Some(handle_console_control), 1) } != 0;
        Self { installed }
    }
}

impl Drop for LauncherCtrlHandlerGuard {
    fn drop(&mut self) {
        if self.installed {
            unsafe {
                SetConsoleCtrlHandler(Some(handle_console_control), 0);
            }
        }
        LAUNCH_CANCELLED.store(false, Ordering::Release);
    }
}

unsafe extern "system" fn handle_console_control(control_type: u32) -> i32 {
    if control_type == CTRL_C_EVENT {
        LAUNCH_CANCELLED.store(true, Ordering::Release);
        1
    } else {
        0
    }
}

pub(crate) struct GuiChildHandoffV1 {
    pipe: Mutex<OwnedHandle>,
    nonce: String,
    completed: AtomicBool,
}

impl GuiChildHandoffV1 {
    pub(crate) fn report_ready(&self) -> Result<(), HandoffErrorV1> {
        if self.completed.load(Ordering::Acquire) {
            return Ok(());
        }

        let deadline = Instant::now() + HANDOFF_TIMEOUT;
        let pipe = self
            .pipe
            .lock()
            .map_err(|_| HandoffErrorV1::InvalidMessage)?;
        write_frame(
            raw_handle(&pipe),
            &HandoffResponseV1 {
                version: HANDOFF_VERSION,
                nonce: self.nonce.clone(),
                status: HandoffStatusV1::Ready,
                diagnostic: None,
            },
        )?;
        let ack: HandoffAckV1 = read_frame(raw_handle(&pipe), deadline)?;
        if ack.version != HANDOFF_VERSION || ack.nonce != self.nonce || !ack.acknowledged {
            return Err(HandoffErrorV1::InvalidMessage);
        }
        self.completed.store(true, Ordering::Release);
        Ok(())
    }

    pub(crate) fn report_failed(&self, diagnostic: &str) {
        if self.completed.swap(true, Ordering::AcqRel) {
            return;
        }
        let bounded = bounded_diagnostic(diagnostic);
        if let Ok(pipe) = self.pipe.lock() {
            let _ = write_frame(
                raw_handle(&pipe),
                &HandoffResponseV1 {
                    version: HANDOFF_VERSION,
                    nonce: self.nonce.clone(),
                    status: HandoffStatusV1::Failed,
                    diagnostic: Some(bounded),
                },
            );
        }
    }

    pub(crate) fn start_deadline_watchdog(self: &std::sync::Arc<Self>) {
        let handoff = std::sync::Arc::clone(self);
        thread::spawn(move || {
            thread::sleep(HANDOFF_TIMEOUT);
            if !handoff.completed.load(Ordering::Acquire) {
                std::process::exit(1);
            }
        });
    }
}

pub(crate) fn is_internal_gui_invocation(arguments: &[std::ffi::OsString]) -> bool {
    arguments
        .first()
        .is_some_and(|argument| argument == OsStr::new(INTERNAL_GUI_MARKER))
}

pub(crate) fn accept_internal_gui(
    arguments: &[std::ffi::OsString],
) -> Result<(GuiLaunchRequestV1, std::sync::Arc<GuiChildHandoffV1>), HandoffErrorV1> {
    if arguments.len() != 2 || !is_internal_gui_invocation(arguments) {
        return Err(HandoffErrorV1::InvalidInternalInvocation);
    }
    let handle_value = arguments[1]
        .to_str()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value != 0 && *value != usize::MAX)
        .ok_or(HandoffErrorV1::InvalidInternalInvocation)?;
    let pipe = unsafe { OwnedHandle::from_raw_handle(handle_value as RawHandle) };
    if unsafe { GetFileType(raw_handle(&pipe)) } != FILE_TYPE_PIPE {
        return Err(HandoffErrorV1::InvalidInternalInvocation);
    }

    let mut server_process_id = 0_u32;
    if unsafe { GetNamedPipeServerProcessId(raw_handle(&pipe), &mut server_process_id) } == 0 {
        return Err(HandoffErrorV1::InvalidParent);
    }
    validate_parent_process(server_process_id)?;

    let deadline = Instant::now() + HANDOFF_TIMEOUT;
    let request: HandoffRequestV1 = read_frame(raw_handle(&pipe), deadline)?;
    if request.version != HANDOFF_VERSION
        || request.parent_process_id != server_process_id
        || !valid_nonce(&request.nonce)
    {
        return Err(HandoffErrorV1::InvalidMessage);
    }

    let handoff = std::sync::Arc::new(GuiChildHandoffV1 {
        pipe: Mutex::new(pipe),
        nonce: request.nonce,
        completed: AtomicBool::new(false),
    });
    Ok((request.launch, handoff))
}

pub(crate) fn launch_gui(
    executable: &Path,
    launch: GuiLaunchRequestV1,
) -> Result<LauncherOutcomeV1, HandoffErrorV1> {
    if LAUNCH_CANCELLED.load(Ordering::Acquire) {
        return Err(HandoffErrorV1::Cancelled);
    }
    let (server, client) = create_connected_pipe()?;
    let nonce = Uuid::new_v4().as_simple().to_string();
    let mut child = create_internal_process(executable, raw_handle(&client))?;
    drop(client);

    let deadline = Instant::now() + HANDOFF_TIMEOUT;
    let exchange = (|| {
        write_frame(
            raw_handle(&server),
            &HandoffRequestV1 {
                version: HANDOFF_VERSION,
                nonce: nonce.clone(),
                parent_process_id: std::process::id(),
                launch,
            },
        )?;
        let response: HandoffResponseV1 = read_frame(raw_handle(&server), deadline)?;
        if response.version != HANDOFF_VERSION || response.nonce != nonce {
            return Err(HandoffErrorV1::InvalidMessage);
        }
        match response.status {
            HandoffStatusV1::Ready if response.diagnostic.is_none() => {
                write_frame(
                    raw_handle(&server),
                    &HandoffAckV1 {
                        version: HANDOFF_VERSION,
                        nonce,
                        acknowledged: true,
                    },
                )?;
                Ok(LauncherOutcomeV1::Ready)
            }
            HandoffStatusV1::Failed => Ok(LauncherOutcomeV1::Failed(
                response
                    .diagnostic
                    .unwrap_or_else(|| "GUI initialization failed".to_string()),
            )),
            _ => Err(HandoffErrorV1::InvalidMessage),
        }
    })();

    child.disarm_on_success(&exchange);
    exchange
}

struct ChildProcessGuard {
    process: Option<OwnedHandle>,
}

impl ChildProcessGuard {
    fn disarm_on_success(&mut self, outcome: &Result<LauncherOutcomeV1, HandoffErrorV1>) {
        if matches!(outcome, Ok(LauncherOutcomeV1::Ready)) {
            self.process.take();
        }
    }
}

impl Drop for ChildProcessGuard {
    fn drop(&mut self) {
        if let Some(process) = &self.process {
            unsafe {
                TerminateProcess(raw_handle(process), 1);
            }
        }
    }
}

fn create_connected_pipe() -> Result<(OwnedHandle, OwnedHandle), HandoffErrorV1> {
    let pipe_name = format!(
        r"\\.\pipe\wingman-launch-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let pipe_name = wide_null(OsStr::new(&pipe_name));
    let server = unsafe {
        CreateNamedPipeW(
            pipe_name.as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            MAX_HANDOFF_FRAME_BYTES as u32,
            MAX_HANDOFF_FRAME_BYTES as u32,
            HANDOFF_TIMEOUT.as_millis() as u32,
            null(),
        )
    };
    if server == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error().into());
    }
    let server = unsafe { OwnedHandle::from_raw_handle(server as RawHandle) };

    let security = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    let client = unsafe {
        CreateFileW(
            pipe_name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            &security,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        )
    };
    if client == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error().into());
    }
    let client = unsafe { OwnedHandle::from_raw_handle(client as RawHandle) };

    if unsafe { ConnectNamedPipe(raw_handle(&server), null_mut()) } == 0
        && unsafe { GetLastError() } != ERROR_PIPE_CONNECTED
    {
        return Err(io::Error::last_os_error().into());
    }
    Ok((server, client))
}

fn create_internal_process(
    executable: &Path,
    inherited_pipe: HANDLE,
) -> Result<ChildProcessGuard, HandoffErrorV1> {
    let application = wide_null(executable.as_os_str());
    let mut command_line = Vec::new();
    command_line.push(u16::from(b'"'));
    command_line.extend(executable.as_os_str().encode_wide());
    command_line.push(u16::from(b'"'));
    command_line.extend(OsStr::new(" ").encode_wide());
    command_line.extend(OsStr::new(INTERNAL_GUI_MARKER).encode_wide());
    command_line.extend(OsStr::new(" ").encode_wide());
    command_line.extend(OsStr::new(&(inherited_pipe as usize).to_string()).encode_wide());
    command_line.push(0);

    let mut attribute_bytes = 0_usize;
    unsafe {
        InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut attribute_bytes);
    }
    if attribute_bytes == 0 {
        return Err(io::Error::last_os_error().into());
    }
    let word_count = attribute_bytes.div_ceil(size_of::<usize>());
    let mut attribute_storage = vec![0_usize; word_count];
    let attribute_list = attribute_storage.as_mut_ptr().cast::<c_void>();
    if unsafe { InitializeProcThreadAttributeList(attribute_list, 1, 0, &mut attribute_bytes) } == 0
    {
        return Err(io::Error::last_os_error().into());
    }
    let _attribute_guard = ProcThreadAttributeGuard(attribute_list);
    if unsafe {
        UpdateProcThreadAttribute(
            attribute_list,
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            (&inherited_pipe as *const HANDLE).cast(),
            size_of::<HANDLE>(),
            null_mut(),
            null(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error().into());
    }

    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.lpAttributeList = attribute_list;
    let mut process_information: PROCESS_INFORMATION = unsafe { zeroed() };
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            command_line.as_mut_ptr(),
            null(),
            null(),
            1,
            CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | EXTENDED_STARTUPINFO_PRESENT,
            null(),
            null(),
            &startup.StartupInfo as *const _,
            &mut process_information,
        )
    };
    if created == 0 {
        return Err(io::Error::last_os_error().into());
    }

    let process =
        unsafe { OwnedHandle::from_raw_handle(process_information.hProcess as RawHandle) };
    let thread_handle =
        unsafe { OwnedHandle::from_raw_handle(process_information.hThread as RawHandle) };
    drop(thread_handle);
    Ok(ChildProcessGuard {
        process: Some(process),
    })
}

struct ProcThreadAttributeGuard(*mut c_void);

impl Drop for ProcThreadAttributeGuard {
    fn drop(&mut self) {
        unsafe {
            DeleteProcThreadAttributeList(self.0);
        }
    }
}

fn validate_parent_process(process_id: u32) -> Result<(), HandoffErrorV1> {
    if process_id == 0 || process_id == std::process::id() {
        return Err(HandoffErrorV1::InvalidParent);
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if process.is_null() {
        return Err(HandoffErrorV1::InvalidParent);
    }
    let process = unsafe { OwnedHandle::from_raw_handle(process as RawHandle) };

    let mut path_buffer = vec![0_u16; 32_768];
    let mut path_length = path_buffer.len() as u32;
    if unsafe {
        QueryFullProcessImageNameW(
            raw_handle(&process),
            0,
            path_buffer.as_mut_ptr(),
            &mut path_length,
        )
    } == 0
    {
        return Err(HandoffErrorV1::InvalidParent);
    }
    path_buffer.truncate(path_length as usize);
    let parent_path = std::path::PathBuf::from(std::ffi::OsString::from_wide(&path_buffer));
    let current_path = std::env::current_exe().map_err(HandoffErrorV1::Io)?;
    let parent_file = File::open(parent_path).map_err(|_| HandoffErrorV1::InvalidParent)?;
    let current_file = File::open(current_path).map_err(|_| HandoffErrorV1::InvalidParent)?;
    if capture_file_identity(&parent_file).map_err(|_| HandoffErrorV1::InvalidParent)?
        != capture_file_identity(&current_file).map_err(|_| HandoffErrorV1::InvalidParent)?
    {
        return Err(HandoffErrorV1::InvalidParent);
    }
    Ok(())
}

fn write_frame<T: Serialize>(handle: HANDLE, message: &T) -> Result<(), HandoffErrorV1> {
    let body = serde_json::to_vec(message).map_err(|_| HandoffErrorV1::InvalidMessage)?;
    if body.is_empty() || body.len() > MAX_HANDOFF_FRAME_BYTES {
        return Err(HandoffErrorV1::InvalidMessage);
    }
    let length = u32::try_from(body.len()).map_err(|_| HandoffErrorV1::InvalidMessage)?;
    write_all(handle, &length.to_le_bytes())?;
    write_all(handle, &body)
}

fn read_frame<T: DeserializeOwned>(handle: HANDLE, deadline: Instant) -> Result<T, HandoffErrorV1> {
    let mut length = [0_u8; 4];
    read_exact_until(handle, &mut length, deadline)?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_HANDOFF_FRAME_BYTES {
        return Err(HandoffErrorV1::InvalidMessage);
    }
    let mut body = vec![0_u8; length];
    read_exact_until(handle, &mut body, deadline)?;
    serde_json::from_slice(&body).map_err(|_| HandoffErrorV1::InvalidMessage)
}

fn write_all(handle: HANDLE, bytes: &[u8]) -> Result<(), HandoffErrorV1> {
    let mut written_total = 0_usize;
    while written_total < bytes.len() {
        let mut written = 0_u32;
        let remaining = &bytes[written_total..];
        if unsafe {
            WriteFile(
                handle,
                remaining.as_ptr(),
                remaining.len() as u32,
                &mut written,
                null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error().into());
        }
        if written == 0 {
            return Err(HandoffErrorV1::InvalidMessage);
        }
        written_total += written as usize;
    }
    Ok(())
}

fn read_exact_until(
    handle: HANDLE,
    destination: &mut [u8],
    deadline: Instant,
) -> Result<(), HandoffErrorV1> {
    let mut offset = 0_usize;
    while offset < destination.len() {
        if LAUNCH_CANCELLED.load(Ordering::Acquire) {
            return Err(HandoffErrorV1::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(HandoffErrorV1::Timeout);
        }
        let mut available = 0_u32;
        if unsafe {
            PeekNamedPipe(
                handle,
                null_mut(),
                0,
                null_mut(),
                &mut available,
                null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error().into());
        }
        if available == 0 {
            thread::sleep(Duration::from_millis(5));
            continue;
        }

        let read_length = (destination.len() - offset).min(available as usize);
        let mut read = 0_u32;
        if unsafe {
            ReadFile(
                handle,
                destination[offset..].as_mut_ptr(),
                read_length as u32,
                &mut read,
                null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error().into());
        }
        if read == 0 {
            return Err(HandoffErrorV1::InvalidMessage);
        }
        offset += read as usize;
    }
    Ok(())
}

fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn raw_handle(handle: &OwnedHandle) -> HANDLE {
    handle.as_raw_handle() as HANDLE
}

fn valid_nonce(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn bounded_diagnostic(value: &str) -> String {
    value.chars().take(512).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_marker_requires_exactly_one_decimal_handle() {
        assert!(!is_internal_gui_invocation(&[]));
        assert!(is_internal_gui_invocation(&[
            INTERNAL_GUI_MARKER.into(),
            "42".into()
        ]));
        assert!(matches!(
            accept_internal_gui(&[INTERNAL_GUI_MARKER.into(), "0".into()]),
            Err(HandoffErrorV1::InvalidInternalInvocation)
        ));
        assert!(matches!(
            accept_internal_gui(&[INTERNAL_GUI_MARKER.into(), "not-a-handle".into()]),
            Err(HandoffErrorV1::InvalidInternalInvocation)
        ));
    }

    #[test]
    fn diagnostics_and_nonces_are_bounded() {
        assert_eq!(bounded_diagnostic(&"x".repeat(600)).len(), 512);
        assert!(valid_nonce("0123456789abcdef0123456789abcdef"));
        assert!(!valid_nonce("0123456789abcdef"));
        assert!(!valid_nonce("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"));
    }

    #[test]
    fn cancelled_launcher_stops_before_blocking_pipe_io() {
        LAUNCH_CANCELLED.store(true, Ordering::Release);
        let mut destination = [0_u8; 1];
        assert!(matches!(
            read_exact_until(
                INVALID_HANDLE_VALUE,
                &mut destination,
                Instant::now() + HANDOFF_TIMEOUT
            ),
            Err(HandoffErrorV1::Cancelled)
        ));
        LAUNCH_CANCELLED.store(false, Ordering::Release);
    }
}
