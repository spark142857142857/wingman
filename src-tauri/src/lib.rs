use once_cell::sync::Lazy;
use parking_lot::Mutex;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::de::{Error as _, IgnoredAny, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

use interpreter::ActiveShell;
use launch_handoff::GuiChildHandoffV1;
use pty_output_flow::PtyOutputFlowV1;
use runtime_files::RuntimeFilesV1;
use session_runtime::{apply_familiar_effect, execute_terminal_input, write_session_input};
use terminal_session::TerminalSessionV1;
use transport::{EditorReadinessBrokerV1, SessionBrokerV1};

const PERFORMANCE_INPUT_ECHO_PROBE_ENV: &str = "WINGMAN_PERF_INPUT_ECHO_PROBE";
const PERFORMANCE_BULK_OUTPUT_PROBE_ENV: &str = "WINGMAN_PERF_BULK_OUTPUT_PROBE";
const PERFORMANCE_BULK_LATENCY_PROBE_ENV: &str = "WINGMAN_PERF_BULK_LATENCY_PROBE";
const PERFORMANCE_BULK_RETENTION_PROBE_ENV: &str = "WINGMAN_PERF_BULK_RETENTION_PROBE";
const PERFORMANCE_SCROLLBACK_PROBE_ENV: &str = "WINGMAN_PERF_SCROLLBACK_PROBE";
const PERFORMANCE_ENDURANCE_PROBE_ENV: &str = "WINGMAN_PERF_ENDURANCE_PROBE";
const PERFORMANCE_SCROLLBACK_ROWS: u32 = 4_000;
const MAX_CLIENT_SESSION_ID: u64 = (1_u64 << 53) - 1;
const MAX_TERMINAL_INPUT_BYTES: usize = 64 * 1024;
const MAX_NATIVE_PASTE_BYTES: usize = 1024 * 1024;
const MAX_PTY_COLS: u16 = 1_000;
const MAX_PTY_ROWS: u16 = 500;
const POWERSHELL_TRANSPORT_SCRIPT: &str = include_str!("powershell_runner_transport.ps1");

pub mod app_launch;
pub mod catalog;
pub mod find_pattern;
pub mod grep_pattern;
pub mod interpreter;
mod launch_handoff;
pub mod lexer;
pub mod ordered_pipeline;
pub mod parser;
pub mod pipeline;
mod pty_output_flow;
pub mod runner;
pub mod runner_cancel;
mod runner_cp;
pub mod runner_find;
pub mod runner_grep;
pub mod runner_io;
pub mod runner_ls;
mod runner_mkdir;
mod runner_mutation;
mod runner_mv;
mod runner_ordered_fault;
mod runner_path_access;
pub mod runner_readonly;
mod runner_rm;
mod runner_touch;
mod runner_transfer;
pub mod runner_which;
mod runtime_files;
mod security_context;
pub mod session_runtime;
pub mod shell_adapter;
pub mod sort_support;
pub mod terminal_session;
pub mod text_stream;
pub mod transport;
pub mod windows_path;

#[derive(Clone, Serialize)]
struct SessionInfo {
    shell: String,
    cwd: String,
    elevated: bool,
    #[serde(rename = "performanceProbeEnabled")]
    performance_probe_enabled: bool,
}

#[derive(Clone, Serialize)]
struct PtyOutput {
    session_id: u64,
    sequence: u64,
    data: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminalInputResult {
    accepted: bool,
    familiar_enabled: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ShellReadinessResult {
    accepted: bool,
    editor_ready: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PerformanceProbeResult {
    accepted: bool,
    enabled: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
enum ShellRequest {
    Powershell,
    Cmd,
}

impl ShellRequest {
    fn active_shell(self) -> ActiveShell {
        match self {
            Self::Powershell => ActiveShell::WindowsPowerShell,
            Self::Cmd => ActiveShell::Cmd,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
enum EndurancePhase {
    Baseline,
    Cycle,
    Complete,
    Failed,
}

struct LatencySamplesV1([f64; 100]);

impl<'de> Deserialize<'de> for LatencySamplesV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct LatencySamplesVisitor;

        impl<'de> Visitor<'de> for LatencySamplesVisitor {
            type Value = LatencySamplesV1;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("exactly 100 latency samples")
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut samples = [0.0; 100];
                for (index, sample) in samples.iter_mut().enumerate() {
                    *sample = sequence
                        .next_element()?
                        .ok_or_else(|| A::Error::invalid_length(index, &self))?;
                }
                if sequence.next_element::<IgnoredAny>()?.is_some() {
                    return Err(A::Error::invalid_length(101, &self));
                }
                Ok(LatencySamplesV1(samples))
            }
        }

        deserializer.deserialize_seq(LatencySamplesVisitor)
    }
}

struct PtySession {
    id: u64,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    shell: ActiveShell,
    cwd: String,
    compat_enabled: bool,
    terminal: TerminalSessionV1,
    readiness: EditorReadinessBrokerV1,
    #[allow(dead_code)]
    broker_pipe_name: String,
    #[allow(dead_code)]
    broker: SessionBrokerV1,
    output_flow: Arc<PtyOutputFlowV1>,
    _runtime_files: Option<RuntimeFilesV1>,
}

impl Drop for PtySession {
    fn drop(&mut self) {
        self.output_flow.close();
    }
}

struct AppState {
    session: Mutex<Option<PtySession>>,
    session_generation: Mutex<u64>,
    performance_input_echo_probe: bool,
    performance_bulk_output_probe: bool,
    performance_bulk_latency_probe: bool,
    performance_bulk_retention_probe: bool,
    performance_scrollback_probe: bool,
    performance_endurance_probe: bool,
}

static APP_STATE: Lazy<AppState> = Lazy::new(|| AppState {
    session: Mutex::new(None),
    session_generation: Mutex::new(0),
    performance_input_echo_probe: performance_input_echo_probe_enabled(
        std::env::var(PERFORMANCE_INPUT_ECHO_PROBE_ENV)
            .ok()
            .as_deref(),
    ),
    performance_bulk_output_probe: performance_input_echo_probe_enabled(
        std::env::var(PERFORMANCE_BULK_OUTPUT_PROBE_ENV)
            .ok()
            .as_deref(),
    ),
    performance_bulk_latency_probe: performance_input_echo_probe_enabled(
        std::env::var(PERFORMANCE_BULK_LATENCY_PROBE_ENV)
            .ok()
            .as_deref(),
    ),
    performance_bulk_retention_probe: performance_input_echo_probe_enabled(
        std::env::var(PERFORMANCE_BULK_RETENTION_PROBE_ENV)
            .ok()
            .as_deref(),
    ),
    performance_scrollback_probe: performance_input_echo_probe_enabled(
        std::env::var(PERFORMANCE_SCROLLBACK_PROBE_ENV)
            .ok()
            .as_deref(),
    ),
    performance_endurance_probe: performance_input_echo_probe_enabled(
        std::env::var(PERFORMANCE_ENDURANCE_PROBE_ENV)
            .ok()
            .as_deref(),
    ),
});

static INITIAL_SHELL: once_cell::sync::OnceCell<app_launch::RequestedShellV1> =
    once_cell::sync::OnceCell::new();
static GUI_HANDOFF: once_cell::sync::OnceCell<Arc<GuiChildHandoffV1>> =
    once_cell::sync::OnceCell::new();

fn performance_input_echo_probe_enabled(value: Option<&str>) -> bool {
    value == Some("1")
}

fn any_performance_probe_enabled() -> bool {
    APP_STATE.performance_input_echo_probe
        || APP_STATE.performance_bulk_output_probe
        || APP_STATE.performance_bulk_latency_probe
        || APP_STATE.performance_bulk_retention_probe
        || APP_STATE.performance_scrollback_probe
        || APP_STATE.performance_endurance_probe
}

fn remove_performance_probe_environment(cmd: &mut CommandBuilder) {
    cmd.env_remove(PERFORMANCE_INPUT_ECHO_PROBE_ENV);
    cmd.env_remove(PERFORMANCE_BULK_OUTPUT_PROBE_ENV);
    cmd.env_remove(PERFORMANCE_BULK_LATENCY_PROBE_ENV);
    cmd.env_remove(PERFORMANCE_BULK_RETENTION_PROBE_ENV);
    cmd.env_remove(PERFORMANCE_SCROLLBACK_PROBE_ENV);
    cmd.env_remove(PERFORMANCE_ENDURANCE_PROBE_ENV);
}

fn terminal_pty_size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows: rows.max(1),
        cols: cols.max(1),
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn validate_terminal_dimensions(cols: u16, rows: u16) -> Result<(), String> {
    if !(1..=MAX_PTY_COLS).contains(&cols) || !(1..=MAX_PTY_ROWS).contains(&rows) {
        return Err("terminal dimensions are out of range".to_string());
    }
    Ok(())
}

fn validate_client_session_id(client_session_id: u64) -> Result<(), String> {
    if !(1..=MAX_CLIENT_SESSION_ID).contains(&client_session_id) {
        return Err("client session ID is out of range".to_string());
    }
    Ok(())
}

fn is_newer_session_generation(current: u64, candidate: u64) -> bool {
    (1..=MAX_CLIENT_SESSION_ID).contains(&candidate) && candidate > current
}

fn reserve_session_generation(client_session_id: u64) -> Result<(), String> {
    validate_client_session_id(client_session_id)?;
    let mut generation = APP_STATE.session_generation.lock();
    if !is_newer_session_generation(*generation, client_session_id) {
        return Err("client session ID is stale".to_string());
    }
    *generation = client_session_id;
    Ok(())
}

fn validate_bridge_data(data: &str, maximum_bytes: usize, kind: &str) -> Result<(), String> {
    if data.len() > maximum_bytes {
        return Err(format!("{kind} exceeds the byte limit"));
    }
    Ok(())
}

#[derive(Default)]
struct Utf8StreamDecoder {
    pending: Vec<u8>,
}

impl Utf8StreamDecoder {
    fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut output = String::new();
        let mut consumed = 0;

        while consumed < self.pending.len() {
            let remaining = &self.pending[consumed..];
            match std::str::from_utf8(remaining) {
                Ok(text) => {
                    output.push_str(text);
                    consumed = self.pending.len();
                }
                Err(error) => {
                    let valid_length = error.valid_up_to();
                    if valid_length > 0 {
                        output.push_str(
                            std::str::from_utf8(&remaining[..valid_length])
                                .expect("valid UTF-8 prefix"),
                        );
                        consumed += valid_length;
                    }

                    if let Some(invalid_length) = error.error_len() {
                        output.push('\u{fffd}');
                        consumed += invalid_length;
                    } else {
                        break;
                    }
                }
            }
        }

        if consumed > 0 {
            self.pending.drain(..consumed);
        }
        output
    }

    fn finish(&mut self) -> String {
        if self.pending.is_empty() {
            String::new()
        } else {
            self.pending.clear();
            "\u{fffd}".to_string()
        }
    }
}

fn resolve_shell(shell: ActiveShell) -> (String, Vec<String>) {
    match shell {
    ActiveShell::Cmd => (
      "cmd.exe".into(),
      vec!["/K".into(), "chcp 65001>nul".into()],
    ),
    ActiveShell::WindowsPowerShell => (
      "powershell.exe".into(),
      vec![
        "-NoLogo".into(),
        "-NoExit".into(),
        "-Command".into(),
        format!("chcp 65001 | Out-Null; [Console]::InputEncoding = New-Object System.Text.UTF8Encoding $false; [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false; {POWERSHELL_TRANSPORT_SCRIPT}"),
      ],
    ),
  }
}

fn detect_cwd(shell: ActiveShell) -> String {
    let output = if shell == ActiveShell::Cmd {
        std::process::Command::new("cmd.exe")
            .args(["/C", "cd"])
            .output()
    } else {
        std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-Command", "(Get-Location).Path"])
            .output()
    };

    match output {
        Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        Err(_) => std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "C:\\".into()),
    }
}

fn monitor_session_exit<F>(
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    session_id: u64,
    on_current_exit: F,
) where
    F: FnOnce() + Send + 'static,
{
    thread::spawn(move || loop {
        let exited = {
            let mut child = child.lock();
            matches!(child.try_wait(), Ok(Some(_)) | Err(_))
        };

        if exited {
            let is_current_session = APP_STATE
                .session
                .lock()
                .as_ref()
                .map(|session| session.id == session_id)
                .unwrap_or(false);
            if is_current_session {
                on_current_exit();
            }
            break;
        }

        thread::sleep(Duration::from_millis(50));
    });
}

fn filter_session_output(session_id: u64, data: &str) -> Option<String> {
    let mut guard = APP_STATE.session.lock();
    let session = guard.as_mut().filter(|session| session.id == session_id)?;
    Some(session.terminal.ingest_pty_output(data))
}

fn deliver_pty_output(
    app: &AppHandle,
    output_flow: &PtyOutputFlowV1,
    session_id: u64,
    sequence: &mut u64,
    data: String,
) -> bool {
    if data.is_empty() {
        return true;
    }
    let Some(current_sequence) = sequence.checked_add(1) else {
        output_flow.close();
        return false;
    };
    *sequence = current_sequence;
    output_flow.deliver(current_sequence, || {
        app.emit(
            "pty-output",
            PtyOutput {
                session_id,
                sequence: current_sequence,
                data,
            },
        )
        .is_ok()
    })
}

#[tauri::command]
fn get_cwd() -> Result<String, String> {
    let guard = APP_STATE.session.lock();
    if let Some(session) = guard.as_ref() {
        Ok(session.cwd.clone())
    } else {
        Ok(detect_cwd(ActiveShell::WindowsPowerShell))
    }
}

#[tauri::command]
fn get_initial_shell() -> &'static str {
    match INITIAL_SHELL
        .get()
        .copied()
        .unwrap_or(app_launch::RequestedShellV1::WindowsPowerShell)
    {
        app_launch::RequestedShellV1::Cmd => "cmd",
        app_launch::RequestedShellV1::WindowsPowerShell => "powershell",
    }
}

#[tauri::command]
fn start_shell(
    app: AppHandle,
    shell: ShellRequest,
    cols: u16,
    rows: u16,
    compat: bool,
    client_session_id: u64,
) -> Result<SessionInfo, String> {
    let result = start_shell_inner(app.clone(), shell, cols, rows, compat, client_session_id);
    if let Some(handoff) = GUI_HANDOFF.get() {
        match &result {
            Ok(_) => {
                if let Err(error) = handoff.report_ready() {
                    app.exit(1);
                    return Err(error.to_string());
                }
            }
            Err(error) => {
                handoff.report_failed(error);
                app.exit(1);
            }
        }
    }
    result
}

fn start_shell_inner(
    app: AppHandle,
    shell: ShellRequest,
    cols: u16,
    rows: u16,
    compat: bool,
    client_session_id: u64,
) -> Result<SessionInfo, String> {
    let _ = compat;
    let active_shell = shell.active_shell();
    validate_terminal_dimensions(cols, rows)?;
    let elevated = security_context::current_process_is_elevated()
        .map_err(|error| format!("could not read process elevation: {error}"))?;
    reserve_session_generation(client_session_id)?;

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(terminal_pty_size(cols, rows))
        .map_err(|e| e.to_string())?;

    let (program, args) = resolve_shell(active_shell);
    let mut cmd = CommandBuilder::new(program);
    let mut terminal = TerminalSessionV1::new(client_session_id, active_shell);
    terminal.disable_pty_readiness();
    let integration_nonce = terminal.integration_nonce().to_string();
    let current_exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let runtime_files = RuntimeFilesV1::resolve(&current_exe)?;
    let broker_pipe_name = format!(
        r"\\.\pipe\wingman-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let readiness_pipe_id = format!(
        "wingman-readiness-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let readiness_pipe_name = format!(r"\\.\pipe\{readiness_pipe_id}");
    let readiness = EditorReadinessBrokerV1::start(&readiness_pipe_name, integration_nonce.clone())
        .map_err(|error| error.to_string())?;
    let broker = SessionBrokerV1::start(&broker_pipe_name).map_err(|error| error.to_string())?;
    cmd.env("WINGMAN_SESSION_NONCE", integration_nonce);
    cmd.env("WINGMAN_READINESS_PIPE", readiness_pipe_id);
    cmd.env(
        "WINGMAN_RUNNER_PATH",
        runtime_files.runner_path().as_os_str(),
    );
    cmd.env("WINGMAN_BROKER_PIPE", &broker_pipe_name);
    remove_performance_probe_environment(&mut cmd);
    for arg in args {
        cmd.arg(arg);
    }
    let cwd_path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from(r"C:\"));
    cmd.cwd(&cwd_path);

    let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    let child = Arc::new(Mutex::new(child));
    let session_id = client_session_id;

    let mut reader = match pair.master.try_clone_reader() {
        Ok(reader) => reader,
        Err(error) => {
            let _ = child.lock().kill();
            return Err(error.to_string());
        }
    };
    let writer = match pair.master.take_writer() {
        Ok(writer) => writer,
        Err(error) => {
            let _ = child.lock().kill();
            return Err(error.to_string());
        }
    };
    let cwd = cwd_path.display().to_string();
    let shell_name = if active_shell == ActiveShell::Cmd {
        "cmd".to_string()
    } else {
        "powershell".to_string()
    };
    let output_flow = Arc::new(PtyOutputFlowV1::new());

    let previous = {
        if *APP_STATE.session_generation.lock() != session_id {
            let _ = child.lock().kill();
            return Err("shell start was superseded".to_string());
        }
        let mut guard = APP_STATE.session.lock();
        guard.replace(PtySession {
            id: session_id,
            writer,
            master: pair.master,
            child: child.clone(),
            shell: active_shell,
            cwd: cwd.clone(),
            compat_enabled: false,
            terminal,
            readiness,
            broker_pipe_name,
            broker,
            output_flow: output_flow.clone(),
            _runtime_files: Some(runtime_files),
        })
    };
    if let Some(previous) = previous {
        let _ = previous.child.lock().kill();
    }

    if let Some(window) = app.get_webview_window("main") {
        let title = if active_shell == ActiveShell::WindowsPowerShell {
            "Wingman - Starting"
        } else {
            "Wingman"
        };
        if let Err(error) = window.set_title(title) {
            let failed = {
                let mut guard = APP_STATE.session.lock();
                if guard
                    .as_ref()
                    .is_some_and(|session| session.id == session_id)
                {
                    guard.take()
                } else {
                    None
                }
            };
            if let Some(failed) = failed {
                let _ = failed.child.lock().kill();
            }
            return Err(error.to_string());
        }
    }

    let app_handle = app.clone();
    thread::spawn(move || {
        let mut buf = [0u8; 8192];
        let mut decoder = Utf8StreamDecoder::default();
        let mut output_sequence = 0u64;
        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    let trailing = decoder.finish();
                    let visible = filter_session_output(session_id, &trailing).unwrap_or_default();
                    let _ = deliver_pty_output(
                        &app_handle,
                        &output_flow,
                        session_id,
                        &mut output_sequence,
                        visible,
                    );
                    break;
                }
                Ok(n) => {
                    let chunk = decoder.push(&buf[..n]);
                    let visible = filter_session_output(session_id, &chunk).unwrap_or_default();
                    if !deliver_pty_output(
                        &app_handle,
                        &output_flow,
                        session_id,
                        &mut output_sequence,
                        visible,
                    ) {
                        break;
                    }
                }
                Err(_) => {
                    let trailing = decoder.finish();
                    let visible = filter_session_output(session_id, &trailing).unwrap_or_default();
                    let _ = deliver_pty_output(
                        &app_handle,
                        &output_flow,
                        session_id,
                        &mut output_sequence,
                        visible,
                    );
                    break;
                }
            }
        }
        output_flow.close();
    });

    let exit_app_handle = app.clone();
    monitor_session_exit(child, session_id, move || {
        let closed = exit_app_handle
            .get_webview_window("main")
            .is_some_and(|window| window.close().is_ok());
        if !closed {
            exit_app_handle.exit(0);
        }
    });

    let _ = app.emit("cwd-changed", cwd.clone());
    Ok(SessionInfo {
        shell: shell_name,
        cwd,
        elevated,
        performance_probe_enabled: any_performance_probe_enabled(),
    })
}

#[tauri::command]
fn acknowledge_pty_output(client_session_id: u64, sequence: u64) -> Result<bool, String> {
    validate_client_session_id(client_session_id)?;
    if sequence == 0 {
        return Ok(false);
    }
    let output_flow = {
        let guard = APP_STATE.session.lock();
        let Some(session) = guard
            .as_ref()
            .filter(|session| session.id == client_session_id)
        else {
            return Ok(false);
        };
        session.output_flow.clone()
    };
    Ok(output_flow.acknowledge(sequence))
}

#[tauri::command]
fn write_native_paste(client_session_id: u64, data: String) -> Result<(), String> {
    validate_client_session_id(client_session_id)?;
    validate_bridge_data(&data, MAX_NATIVE_PASTE_BYTES, "native paste")?;
    if !data.contains(['\r', '\n']) {
        return Err("native paste requires a line break".to_string());
    }

    let mut guard = APP_STATE.session.lock();
    let session = guard
        .as_mut()
        .ok_or_else(|| "shell not started".to_string())?;
    if session.id != client_session_id {
        return Ok(());
    }

    session.terminal.suspend_for_native_paste(&data);
    write_session_input(
        session.id,
        client_session_id,
        session.writer.as_mut(),
        &data,
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn refresh_editor_readiness(session: &mut PtySession) {
    match session.readiness.drain() {
        Ok(frames) => {
            for frame in frames {
                if !session.terminal.apply_editor_readiness(&frame) {
                    session.terminal.suspend_after_transport_failure();
                    break;
                }
            }
        }
        Err(_) => session.terminal.suspend_after_transport_failure(),
    }
}

#[tauri::command]
fn poll_shell_readiness(
    app: AppHandle,
    client_session_id: u64,
) -> Result<ShellReadinessResult, String> {
    validate_client_session_id(client_session_id)?;
    let mut guard = APP_STATE.session.lock();
    let session = guard
        .as_mut()
        .ok_or_else(|| "shell not started".to_string())?;
    if session.id != client_session_id {
        return Ok(ShellReadinessResult {
            accepted: false,
            editor_ready: false,
        });
    }

    refresh_editor_readiness(session);
    let editor_ready = session.terminal.editor_ready();
    if editor_ready {
        if let Some(window) = app.get_webview_window("main") {
            window
                .set_title("Wingman - Ready")
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(ShellReadinessResult {
        accepted: true,
        editor_ready,
    })
}

#[tauri::command]
fn performance_input_echo_probe(client_session_id: u64) -> Result<PerformanceProbeResult, String> {
    validate_client_session_id(client_session_id)?;
    let guard = APP_STATE.session.lock();
    let accepted = guard
        .as_ref()
        .is_some_and(|session| session.id == client_session_id);
    Ok(PerformanceProbeResult {
        accepted,
        enabled: accepted && APP_STATE.performance_input_echo_probe,
    })
}

#[tauri::command]
fn performance_bulk_output_probe(client_session_id: u64) -> Result<PerformanceProbeResult, String> {
    validate_client_session_id(client_session_id)?;
    let guard = APP_STATE.session.lock();
    let accepted = guard
        .as_ref()
        .is_some_and(|session| session.id == client_session_id);
    Ok(PerformanceProbeResult {
        accepted,
        enabled: accepted && APP_STATE.performance_bulk_output_probe,
    })
}

#[tauri::command]
fn performance_bulk_latency_probe(
    client_session_id: u64,
) -> Result<PerformanceProbeResult, String> {
    validate_client_session_id(client_session_id)?;
    let guard = APP_STATE.session.lock();
    let accepted = guard
        .as_ref()
        .is_some_and(|session| session.id == client_session_id);
    Ok(PerformanceProbeResult {
        accepted,
        enabled: accepted && APP_STATE.performance_bulk_latency_probe,
    })
}

#[tauri::command]
fn performance_bulk_retention_probe(
    client_session_id: u64,
) -> Result<PerformanceProbeResult, String> {
    validate_client_session_id(client_session_id)?;
    let guard = APP_STATE.session.lock();
    let accepted = guard
        .as_ref()
        .is_some_and(|session| session.id == client_session_id);
    Ok(PerformanceProbeResult {
        accepted,
        enabled: accepted && APP_STATE.performance_bulk_retention_probe,
    })
}

#[tauri::command]
fn performance_scrollback_probe(client_session_id: u64) -> Result<PerformanceProbeResult, String> {
    validate_client_session_id(client_session_id)?;
    let guard = APP_STATE.session.lock();
    let accepted = guard
        .as_ref()
        .is_some_and(|session| session.id == client_session_id);
    Ok(PerformanceProbeResult {
        accepted,
        enabled: accepted && APP_STATE.performance_scrollback_probe,
    })
}

#[tauri::command]
fn performance_endurance_probe(client_session_id: u64) -> Result<PerformanceProbeResult, String> {
    validate_client_session_id(client_session_id)?;
    let guard = APP_STATE.session.lock();
    let accepted = guard
        .as_ref()
        .is_some_and(|session| session.id == client_session_id);
    Ok(PerformanceProbeResult {
        accepted,
        enabled: accepted && APP_STATE.performance_endurance_probe,
    })
}

#[tauri::command]
fn mark_performance_endurance(
    app: AppHandle,
    client_session_id: u64,
    phase: EndurancePhase,
    cycle: u32,
) -> Result<bool, String> {
    validate_client_session_id(client_session_id)?;
    let guard = APP_STATE.session.lock();
    let accepted = APP_STATE.performance_endurance_probe
        && guard
            .as_ref()
            .is_some_and(|session| session.id == client_session_id);
    if !accepted {
        return Ok(false);
    }
    let title = match phase {
        EndurancePhase::Baseline if cycle == 0 => "Wingman - Endurance Baseline".to_string(),
        EndurancePhase::Cycle if (1..=10_000).contains(&cycle) => {
            format!("Wingman - Endurance Cycle {cycle}")
        }
        EndurancePhase::Complete if cycle > 0 => format!("Wingman - Endurance Complete {cycle}"),
        EndurancePhase::Failed => format!("Wingman - Endurance Failed {cycle}"),
        _ => return Err("invalid endurance measurement phase".to_string()),
    };
    if let Some(window) = app.get_webview_window("main") {
        window
            .set_title(&title)
            .map_err(|error| error.to_string())?;
    }
    Ok(true)
}

#[tauri::command]
fn mark_performance_input_echo(app: AppHandle, client_session_id: u64) -> Result<bool, String> {
    validate_client_session_id(client_session_id)?;
    let guard = APP_STATE.session.lock();
    let accepted = APP_STATE.performance_input_echo_probe
        && guard
            .as_ref()
            .is_some_and(|session| session.id == client_session_id);
    if accepted {
        if let Some(window) = app.get_webview_window("main") {
            window
                .set_title("Wingman - Echoed")
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(accepted)
}

#[tauri::command]
fn mark_performance_bulk_output(app: AppHandle, client_session_id: u64) -> Result<bool, String> {
    validate_client_session_id(client_session_id)?;
    let guard = APP_STATE.session.lock();
    let accepted = APP_STATE.performance_bulk_output_probe
        && guard
            .as_ref()
            .is_some_and(|session| session.id == client_session_id);
    if accepted {
        if let Some(window) = app.get_webview_window("main") {
            window
                .set_title("Wingman - Bulk Rendered")
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(accepted)
}

fn valid_scrollback_measurement(
    configured_scrollback_rows: u32,
    viewport_rows: u32,
    buffer_rows: u32,
) -> bool {
    configured_scrollback_rows == PERFORMANCE_SCROLLBACK_ROWS
        && viewport_rows > 0
        && buffer_rows.checked_sub(viewport_rows) == Some(configured_scrollback_rows)
}

#[tauri::command]
fn mark_performance_scrollback(
    app: AppHandle,
    client_session_id: u64,
    configured_scrollback_rows: u32,
    viewport_rows: u32,
    buffer_rows: u32,
) -> Result<bool, String> {
    validate_client_session_id(client_session_id)?;
    let guard = APP_STATE.session.lock();
    let accepted = APP_STATE.performance_scrollback_probe
        && guard
            .as_ref()
            .is_some_and(|session| session.id == client_session_id);
    if !accepted {
        return Ok(false);
    }
    if !valid_scrollback_measurement(configured_scrollback_rows, viewport_rows, buffer_rows) {
        return Err("invalid scrollback ceiling measurement".to_string());
    }
    let title =
        format!("Wingman - Scrollback {configured_scrollback_rows} {viewport_rows} {buffer_rows}");
    if let Some(window) = app.get_webview_window("main") {
        window
            .set_title(&title)
            .map_err(|error| error.to_string())?;
    }
    Ok(true)
}

#[tauri::command]
fn mark_performance_bulk_latency(
    app: AppHandle,
    client_session_id: u64,
    samples_ms: LatencySamplesV1,
) -> Result<bool, String> {
    validate_client_session_id(client_session_id)?;
    let guard = APP_STATE.session.lock();
    let accepted = APP_STATE.performance_bulk_latency_probe
        && guard
            .as_ref()
            .is_some_and(|session| session.id == client_session_id);
    if !accepted {
        return Ok(false);
    }
    let samples_ms = samples_ms.0;
    if samples_ms
        .iter()
        .any(|sample| !sample.is_finite() || !(0.0..=60_000.0).contains(sample))
    {
        return Err("invalid bulk input-latency distribution".to_string());
    }

    let mut sorted = samples_ms;
    sorted.sort_by(f64::total_cmp);
    let median = (sorted[49] + sorted[50]) / 2.0;
    let p95 = sorted[94];
    let maximum = sorted[99];
    let raw = samples_ms
        .iter()
        .map(|sample| format!("{sample:.1}"))
        .collect::<Vec<_>>()
        .join(",");
    let title = format!("Wingman - Bulk Latency {median:.1} {p95:.1} {maximum:.1}|{raw}");
    if let Some(window) = app.get_webview_window("main") {
        window
            .set_title(&title)
            .map_err(|error| error.to_string())?;
    }
    Ok(true)
}

fn mark_performance_retention_phase(
    app: AppHandle,
    client_session_id: u64,
    title: &str,
) -> Result<bool, String> {
    validate_client_session_id(client_session_id)?;
    let guard = APP_STATE.session.lock();
    let accepted = APP_STATE.performance_bulk_retention_probe
        && guard
            .as_ref()
            .is_some_and(|session| session.id == client_session_id);
    if accepted {
        if let Some(window) = app.get_webview_window("main") {
            window.set_title(title).map_err(|error| error.to_string())?;
        }
    }
    Ok(accepted)
}

#[tauri::command]
fn mark_performance_retention_baseline(
    app: AppHandle,
    client_session_id: u64,
) -> Result<bool, String> {
    mark_performance_retention_phase(app, client_session_id, "Wingman - Retention Baseline")
}

#[tauri::command]
fn mark_performance_retention_cleared(
    app: AppHandle,
    client_session_id: u64,
) -> Result<bool, String> {
    mark_performance_retention_phase(app, client_session_id, "Wingman - Retention Cleared")
}

#[tauri::command]
fn handle_terminal_input(
    client_session_id: u64,
    data: String,
) -> Result<TerminalInputResult, String> {
    validate_client_session_id(client_session_id)?;
    validate_bridge_data(&data, MAX_TERMINAL_INPUT_BYTES, "terminal input")?;
    let mut guard = APP_STATE.session.lock();
    let session = guard
        .as_mut()
        .ok_or_else(|| "shell not started".to_string())?;
    if session.id != client_session_id {
        return Ok(TerminalInputResult {
            accepted: false,
            familiar_enabled: session.compat_enabled,
        });
    }
    let active_shell = session.shell;
    refresh_editor_readiness(session);

    let outcome = execute_terminal_input(
        &mut session.terminal,
        active_shell,
        &session.broker,
        session.writer.as_mut(),
        &data,
        session.compat_enabled,
    )
    .map_err(|error| error.to_string())?;
    if let session_runtime::TerminalExecutionOutcomeV1::Prepared {
        familiar_effect: Some(effect),
        ..
    } = outcome
    {
        apply_familiar_effect(effect, &mut session.compat_enabled, |_| Ok(()))
            .map_err(|error| error.to_string())?;
    }
    Ok(TerminalInputResult {
        accepted: true,
        familiar_enabled: session.compat_enabled,
    })
}

#[tauri::command]
fn resize_shell(client_session_id: u64, cols: u16, rows: u16) -> Result<(), String> {
    validate_client_session_id(client_session_id)?;
    validate_terminal_dimensions(cols, rows)?;
    let guard = APP_STATE.session.lock();
    let session = guard
        .as_ref()
        .ok_or_else(|| "shell not started".to_string())?;
    if session.id != client_session_id {
        return Ok(());
    }
    session
        .master
        .resize(terminal_pty_size(cols, rows))
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    run_gui(app_launch::RequestedShellV1::WindowsPowerShell, None);
}

pub(crate) fn run_gui(
    initial_shell: app_launch::RequestedShellV1,
    handoff: Option<Arc<GuiChildHandoffV1>>,
) {
    let _ = INITIAL_SHELL.set(initial_shell);
    if let Some(handoff) = handoff {
        handoff.start_deadline_watchdog();
        let _ = GUI_HANDOFF.set(handoff);
    }
    let result = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_cwd,
            get_initial_shell,
            start_shell,
            poll_shell_readiness,
            performance_input_echo_probe,
            performance_bulk_output_probe,
            performance_bulk_latency_probe,
            performance_bulk_retention_probe,
            performance_scrollback_probe,
            performance_endurance_probe,
            mark_performance_input_echo,
            mark_performance_bulk_output,
            mark_performance_bulk_latency,
            mark_performance_scrollback,
            mark_performance_retention_baseline,
            mark_performance_retention_cleared,
            mark_performance_endurance,
            acknowledge_pty_output,
            write_native_paste,
            handle_terminal_input,
            resize_shell
        ])
        .run(tauri::generate_context!());

    result.expect("error while running tauri application");
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn performance_input_probe_requires_exact_opt_in() {
        assert!(performance_input_echo_probe_enabled(Some("1")));
        assert!(!performance_input_echo_probe_enabled(None));
        assert!(!performance_input_echo_probe_enabled(Some("0")));
        assert!(!performance_input_echo_probe_enabled(Some("true")));
    }

    #[test]
    fn performance_input_probe_is_removed_from_shell_environment() {
        let mut command = CommandBuilder::new("cmd.exe");
        command.env(PERFORMANCE_INPUT_ECHO_PROBE_ENV, "1");
        command.env(PERFORMANCE_BULK_OUTPUT_PROBE_ENV, "1");
        command.env(PERFORMANCE_BULK_LATENCY_PROBE_ENV, "1");
        command.env(PERFORMANCE_BULK_RETENTION_PROBE_ENV, "1");
        command.env(PERFORMANCE_SCROLLBACK_PROBE_ENV, "1");
        command.env(PERFORMANCE_ENDURANCE_PROBE_ENV, "1");
        remove_performance_probe_environment(&mut command);
        assert_eq!(command.get_env(PERFORMANCE_INPUT_ECHO_PROBE_ENV), None);
        assert_eq!(command.get_env(PERFORMANCE_BULK_OUTPUT_PROBE_ENV), None);
        assert_eq!(command.get_env(PERFORMANCE_BULK_LATENCY_PROBE_ENV), None);
        assert_eq!(command.get_env(PERFORMANCE_BULK_RETENTION_PROBE_ENV), None);
        assert_eq!(command.get_env(PERFORMANCE_SCROLLBACK_PROBE_ENV), None);
        assert_eq!(command.get_env(PERFORMANCE_ENDURANCE_PROBE_ENV), None);
    }

    #[test]
    fn scrollback_measurement_requires_a_full_exact_ceiling() {
        assert!(valid_scrollback_measurement(4_000, 27, 4_027));
        assert!(!valid_scrollback_measurement(1_000, 27, 1_027));
        assert!(!valid_scrollback_measurement(4_000, 27, 4_026));
        assert!(!valid_scrollback_measurement(4_000, 0, 4_000));
    }

    #[test]
    fn current_session_exit_is_detected() {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("open test PTY");

        let mut command = CommandBuilder::new("cmd.exe");
        command.args(["/C", "exit", "0"]);
        let child = Arc::new(Mutex::new(
            pair.slave.spawn_command(command).expect("spawn test shell"),
        ));
        let writer = pair.master.take_writer().expect("take test writer");
        let session_id = u64::MAX;

        APP_STATE.session.lock().replace(PtySession {
            id: session_id,
            writer,
            master: pair.master,
            child: child.clone(),
            shell: ActiveShell::Cmd,
            cwd: "C:\\".into(),
            compat_enabled: true,
            terminal: TerminalSessionV1::new(session_id, ActiveShell::Cmd),
            readiness: EditorReadinessBrokerV1::start(
                &format!(
                    r"\\.\pipe\wingman-test-readiness-{}-{}",
                    session_id,
                    Uuid::new_v4().as_simple()
                ),
                "abcdef0123456789abcdef0123456789".to_string(),
            )
            .expect("start test readiness broker"),
            broker_pipe_name: format!(
                r"\\.\pipe\wingman-test-{}-{}",
                session_id,
                Uuid::new_v4().as_simple()
            ),
            broker: SessionBrokerV1::start(&format!(
                r"\\.\pipe\wingman-test-broker-{}-{}",
                session_id,
                Uuid::new_v4().as_simple()
            ))
            .expect("start test session broker"),
            output_flow: Arc::new(PtyOutputFlowV1::new()),
            _runtime_files: None,
        });

        let (sender, receiver) = mpsc::channel();
        monitor_session_exit(child, session_id, move || {
            let _ = sender.send(());
        });

        receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("current shell exit should be detected");
        APP_STATE.session.lock().take();
    }

    #[test]
    fn pty_size_matches_small_terminal_dimensions() {
        let size = terminal_pty_size(28, 6);
        assert_eq!(size.cols, 28);
        assert_eq!(size.rows, 6);

        let minimum = terminal_pty_size(0, 0);
        assert_eq!(minimum.cols, 1);
        assert_eq!(minimum.rows, 1);
    }

    #[test]
    fn powershell_bootstrap_does_not_source_the_legacy_compat_profile() {
        let (_, arguments) = resolve_shell(ActiveShell::WindowsPowerShell);
        let command = arguments.last().expect("PowerShell command argument");

        assert!(!command.contains("WINGMAN_COMPAT_PROFILE"));
        assert!(!command.contains("powershell_compat"));
        assert!(!arguments.iter().any(|argument| argument == "Bypass"));
        assert!(!command.contains("WINGMAN_INTEGRATION_SCRIPT"));
    }

    #[test]
    fn powershell_bootstrap_loads_the_runner_transport_without_prompt_markers() {
        let nonce = "abcdef0123456789abcdef0123456789";

        let (program, mut arguments) = resolve_shell(ActiveShell::WindowsPowerShell);
        arguments.retain(|argument| argument != "-NoExit");
        let command = arguments.last_mut().expect("PowerShell command argument");
        command.push_str(
            "; [Console]::Out.Write([bool](Get-Command Invoke-WingmanPrepared -ErrorAction SilentlyContinue)); [Console]::Out.Write((prompt))",
        );

        let output = std::process::Command::new(program)
            .args(arguments)
            .env("WINGMAN_SESSION_NONCE", nonce)
            .output()
            .expect("run PowerShell bootstrap");

        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).expect("UTF-8 bootstrap output");
        assert!(stdout.starts_with("True"));
        assert!(!stdout.contains("wingman-prompt"));
    }

    #[test]
    fn utf8_decoder_preserves_multibyte_characters_across_reads() {
        let text = "Wingman 한글 🚀";
        let bytes = text.as_bytes();
        let mut decoder = Utf8StreamDecoder::default();
        let mut output = String::new();

        for byte in bytes {
            output.push_str(&decoder.push(std::slice::from_ref(byte)));
        }
        output.push_str(&decoder.finish());

        assert_eq!(output, text);
    }

    #[test]
    fn utf8_decoder_replaces_invalid_sequences_without_losing_valid_text() {
        let mut decoder = Utf8StreamDecoder::default();
        assert_eq!(decoder.push(b"ok\xffdone"), "ok\u{fffd}done");
        assert_eq!(decoder.finish(), "");
    }

    #[test]
    fn ipc_shell_names_fail_closed() {
        assert_eq!(
            serde_json::from_str::<ShellRequest>(r#""powershell""#).unwrap(),
            ShellRequest::Powershell
        );
        assert_eq!(
            serde_json::from_str::<ShellRequest>(r#""cmd""#).unwrap(),
            ShellRequest::Cmd
        );
        assert!(serde_json::from_str::<ShellRequest>(r#""pwsh""#).is_err());
        assert!(serde_json::from_str::<ShellRequest>(r#""""#).is_err());
        assert!(serde_json::from_str::<EndurancePhase>(r#""unknown""#).is_err());
    }

    #[test]
    fn ipc_latency_distribution_deserializes_into_a_fixed_buffer() {
        let exact = serde_json::to_string(&vec![1.0; 100]).unwrap();
        let samples = serde_json::from_str::<LatencySamplesV1>(&exact).unwrap();
        assert_eq!(samples.0, [1.0; 100]);

        let short = serde_json::to_string(&vec![1.0; 99]).unwrap();
        assert!(serde_json::from_str::<LatencySamplesV1>(&short).is_err());
        let long = serde_json::to_string(&vec![1.0; 101]).unwrap();
        assert!(serde_json::from_str::<LatencySamplesV1>(&long).is_err());
    }

    #[test]
    fn ipc_terminal_dimensions_are_bounded() {
        assert!(validate_terminal_dimensions(1, 1).is_ok());
        assert!(validate_terminal_dimensions(MAX_PTY_COLS, MAX_PTY_ROWS).is_ok());
        assert!(validate_terminal_dimensions(0, 24).is_err());
        assert!(validate_terminal_dimensions(80, 0).is_err());
        assert!(validate_terminal_dimensions(MAX_PTY_COLS + 1, 24).is_err());
        assert!(validate_terminal_dimensions(80, MAX_PTY_ROWS + 1).is_err());
    }

    #[test]
    fn ipc_session_generations_are_positive_monotonic_js_safe_integers() {
        assert!(is_newer_session_generation(0, 1));
        assert!(is_newer_session_generation(41, 42));
        assert!(is_newer_session_generation(
            MAX_CLIENT_SESSION_ID - 1,
            MAX_CLIENT_SESSION_ID
        ));
        assert!(!is_newer_session_generation(0, 0));
        assert!(!is_newer_session_generation(42, 42));
        assert!(!is_newer_session_generation(42, 41));
        assert!(!is_newer_session_generation(
            MAX_CLIENT_SESSION_ID,
            MAX_CLIENT_SESSION_ID + 1
        ));
    }

    #[test]
    fn ipc_terminal_and_paste_payloads_have_separate_byte_limits() {
        let terminal_limit = "x".repeat(MAX_TERMINAL_INPUT_BYTES);
        assert!(validate_bridge_data(&terminal_limit, MAX_TERMINAL_INPUT_BYTES, "input").is_ok());
        assert!(
            validate_bridge_data(&(terminal_limit + "x"), MAX_TERMINAL_INPUT_BYTES, "input")
                .is_err()
        );

        let paste_limit = "한".repeat(MAX_NATIVE_PASTE_BYTES / "한".len());
        assert!(validate_bridge_data(&paste_limit, MAX_NATIVE_PASTE_BYTES, "paste").is_ok());
        assert!(
            validate_bridge_data(&(paste_limit + "한"), MAX_NATIVE_PASTE_BYTES, "paste").is_err()
        );
    }

    #[test]
    fn current_process_elevation_is_observable() {
        security_context::current_process_is_elevated().expect("query current process elevation");
    }
}
