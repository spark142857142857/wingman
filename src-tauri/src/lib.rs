use once_cell::sync::Lazy;
use parking_lot::Mutex;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use std::io::{Read, Write};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

use interpreter::ActiveShell;
use session_runtime::{apply_familiar_effect, execute_terminal_input, write_session_input};
use terminal_session::TerminalSessionV1;
use transport::{EditorReadinessBrokerV1, SessionBrokerV1};

pub mod app_launch;
pub mod catalog;
pub mod find_pattern;
pub mod grep_pattern;
pub mod interpreter;
pub mod lexer;
pub mod ordered_pipeline;
pub mod parser;
pub mod pipeline;
pub mod runner;
pub mod runner_cancel;
pub mod runner_find;
pub mod runner_grep;
pub mod runner_io;
pub mod runner_ls;
mod runner_mkdir;
mod runner_mutation;
mod runner_ordered_fault;
pub mod runner_readonly;
pub mod runner_which;
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
}

#[derive(Clone, Serialize)]
struct PtyOutput {
    session_id: u64,
    data: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminalInputResult {
    accepted: bool,
    familiar_enabled: bool,
}

struct PtySession {
    id: u64,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    #[allow(dead_code)]
    shell: String,
    cwd: String,
    compat_enabled: bool,
    terminal: TerminalSessionV1,
    readiness: EditorReadinessBrokerV1,
    #[allow(dead_code)]
    broker_pipe_name: String,
    #[allow(dead_code)]
    broker: SessionBrokerV1,
}

struct AppState {
    session: Mutex<Option<PtySession>>,
}

static APP_STATE: Lazy<AppState> = Lazy::new(|| AppState {
    session: Mutex::new(None),
});

fn terminal_pty_size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows: rows.max(1),
        cols: cols.max(1),
        pixel_width: 0,
        pixel_height: 0,
    }
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

fn resolve_shell(shell: &str) -> (String, Vec<String>) {
    match shell {
    "cmd" => (
      "cmd.exe".into(),
      vec!["/K".into(), "chcp 65001>nul".into()],
    ),
    _ => (
      "powershell.exe".into(),
      vec![
        "-NoLogo".into(),
        "-NoExit".into(),
        "-ExecutionPolicy".into(),
        "Bypass".into(),
        "-Command".into(),
        "chcp 65001 | Out-Null; [Console]::InputEncoding = New-Object System.Text.UTF8Encoding $false; [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false; if ($env:WINGMAN_INTEGRATION_SCRIPT) { . $env:WINGMAN_INTEGRATION_SCRIPT }".into(),
      ],
    ),
  }
}

fn detect_cwd(shell: &str) -> String {
    let output = if shell == "cmd" {
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

#[tauri::command]
fn get_cwd() -> Result<String, String> {
    let guard = APP_STATE.session.lock();
    if let Some(session) = guard.as_ref() {
        Ok(session.cwd.clone())
    } else {
        Ok(detect_cwd("powershell"))
    }
}

#[tauri::command]
fn start_shell(
    app: AppHandle,
    shell: String,
    cols: u16,
    rows: u16,
    compat: bool,
    client_session_id: u64,
) -> Result<SessionInfo, String> {
    let _ = compat;
    {
        let mut guard = APP_STATE.session.lock();
        if let Some(previous) = guard.take() {
            let _ = previous.child.lock().kill();
        }
    }

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(terminal_pty_size(cols, rows))
        .map_err(|e| e.to_string())?;

    let (program, args) = resolve_shell(&shell);
    let mut cmd = CommandBuilder::new(program);
    let active_shell = if shell == "cmd" {
        ActiveShell::Cmd
    } else {
        ActiveShell::WindowsPowerShell
    };
    let mut terminal = TerminalSessionV1::new(client_session_id, active_shell);
    terminal.disable_pty_readiness();
    let integration_nonce = terminal.integration_nonce().to_string();
    let runner_path = std::env::current_exe()
        .map_err(|error| error.to_string())?
        .with_file_name("wingman-runner.exe");
    let integration_path = app
        .path()
        .resource_dir()
        .map_err(|error| error.to_string())?
        .join("src")
        .join("powershell_runner_transport.ps1");
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
    cmd.env("WINGMAN_INTEGRATION_SCRIPT", integration_path.as_os_str());
    cmd.env("WINGMAN_SESSION_NONCE", integration_nonce);
    cmd.env("WINGMAN_READINESS_PIPE", readiness_pipe_id);
    cmd.env("WINGMAN_RUNNER_PATH", runner_path.as_os_str());
    cmd.env("WINGMAN_BROKER_PIPE", &broker_pipe_name);
    for arg in args {
        cmd.arg(arg);
    }
    if let Ok(cwd) = std::env::current_dir() {
        cmd.cwd(cwd);
    }

    let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    let child = Arc::new(Mutex::new(child));
    let session_id = client_session_id;

    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    let cwd = detect_cwd(&shell);
    let shell_name = if shell == "cmd" {
        "cmd".to_string()
    } else {
        "powershell".to_string()
    };

    {
        let mut guard = APP_STATE.session.lock();
        *guard = Some(PtySession {
            id: session_id,
            writer,
            master: pair.master,
            child: child.clone(),
            shell: shell_name.clone(),
            cwd: cwd.clone(),
            compat_enabled: false,
            terminal,
            readiness,
            broker_pipe_name,
            broker,
        });
    }

    let app_handle = app.clone();
    thread::spawn(move || {
        let mut buf = [0u8; 8192];
        let mut decoder = Utf8StreamDecoder::default();
        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    let trailing = decoder.finish();
                    let visible = filter_session_output(session_id, &trailing).unwrap_or_default();
                    if !visible.is_empty() {
                        let _ = app_handle.emit(
                            "pty-output",
                            PtyOutput {
                                session_id,
                                data: visible,
                            },
                        );
                    }
                    break;
                }
                Ok(n) => {
                    let chunk = decoder.push(&buf[..n]);
                    let visible = filter_session_output(session_id, &chunk).unwrap_or_default();
                    if !visible.is_empty() {
                        let _ = app_handle.emit(
                            "pty-output",
                            PtyOutput {
                                session_id,
                                data: visible,
                            },
                        );
                    }
                }
                Err(_) => {
                    let trailing = decoder.finish();
                    let visible = filter_session_output(session_id, &trailing).unwrap_or_default();
                    if !visible.is_empty() {
                        let _ = app_handle.emit(
                            "pty-output",
                            PtyOutput {
                                session_id,
                                data: visible,
                            },
                        );
                    }
                    break;
                }
            }
        }
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
    })
}

#[tauri::command]
fn write_native_paste(client_session_id: u64, data: String) -> Result<(), String> {
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

#[tauri::command]
fn handle_terminal_input(
    client_session_id: u64,
    data: String,
) -> Result<TerminalInputResult, String> {
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
    let active_shell = if session.shell == "cmd" {
        ActiveShell::Cmd
    } else {
        ActiveShell::WindowsPowerShell
    };
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
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            get_cwd,
            start_shell,
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
    use std::path::PathBuf;
    use std::sync::mpsc;

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
            shell: "cmd".into(),
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
        let (_, arguments) = resolve_shell("powershell");
        let command = arguments.last().expect("PowerShell command argument");

        assert!(!command.contains("WINGMAN_COMPAT_PROFILE"));
        assert!(!command.contains("powershell_compat"));
    }

    #[test]
    fn powershell_bootstrap_loads_the_runner_transport_without_prompt_markers() {
        let integration_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("powershell_runner_transport.ps1");
        let nonce = "abcdef0123456789abcdef0123456789";

        let (program, mut arguments) = resolve_shell("powershell");
        arguments.retain(|argument| argument != "-NoExit");
        let command = arguments.last_mut().expect("PowerShell command argument");
        command.push_str(
            "; [Console]::Out.Write([bool](Get-Command Invoke-WingmanPrepared -ErrorAction SilentlyContinue)); [Console]::Out.Write((prompt))",
        );

        let output = std::process::Command::new(program)
            .args(arguments)
            .env("WINGMAN_INTEGRATION_SCRIPT", integration_path)
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
}
