use once_cell::sync::Lazy;
use parking_lot::Mutex;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

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

struct PtySession {
    id: u64,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    #[allow(dead_code)]
    shell: String,
    cwd: String,
}

struct AppState {
    session: Mutex<Option<PtySession>>,
}

static APP_STATE: Lazy<AppState> = Lazy::new(|| AppState {
    session: Mutex::new(None),
});

static COMPAT_FLAG_PATH: Lazy<PathBuf> =
    Lazy::new(|| std::env::temp_dir().join(format!("wingman-{}-compat.flag", std::process::id())));

static COMPAT_PROFILE_PATH: Lazy<PathBuf> =
    Lazy::new(|| std::env::temp_dir().join(format!("wingman-{}-compat.ps1", std::process::id())));

const POWERSHELL_COMPAT_PROFILE: &str = include_str!("powershell_compat.ps1");

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
        "chcp 65001 | Out-Null; [Console]::InputEncoding = New-Object System.Text.UTF8Encoding $false; [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false; . $env:WINGMAN_COMPAT_PROFILE".into(),
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
    std::fs::write(&*COMPAT_FLAG_PATH, if compat { "1" } else { "0" })
        .map_err(|e| e.to_string())?;
    if shell != "cmd" {
        std::fs::write(&*COMPAT_PROFILE_PATH, POWERSHELL_COMPAT_PROFILE)
            .map_err(|e| e.to_string())?;
    }
    cmd.env("WINGMAN_COMPAT_FLAG", COMPAT_FLAG_PATH.as_os_str());
    cmd.env("WINGMAN_COMPAT_PROFILE", COMPAT_PROFILE_PATH.as_os_str());
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
                    if !trailing.is_empty() {
                        let _ = app_handle.emit(
                            "pty-output",
                            PtyOutput {
                                session_id,
                                data: trailing,
                            },
                        );
                    }
                    break;
                }
                Ok(n) => {
                    let chunk = decoder.push(&buf[..n]);
                    if !chunk.is_empty() {
                        let _ = app_handle.emit(
                            "pty-output",
                            PtyOutput {
                                session_id,
                                data: chunk,
                            },
                        );
                    }
                }
                Err(_) => {
                    let trailing = decoder.finish();
                    if !trailing.is_empty() {
                        let _ = app_handle.emit(
                            "pty-output",
                            PtyOutput {
                                session_id,
                                data: trailing,
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
fn set_compat(enabled: bool) -> Result<(), String> {
    std::fs::write(&*COMPAT_FLAG_PATH, if enabled { "1" } else { "0" }).map_err(|e| e.to_string())
}

#[tauri::command]
fn write_shell(data: String) -> Result<(), String> {
    let mut guard = APP_STATE.session.lock();
    let session = guard
        .as_mut()
        .ok_or_else(|| "shell not started".to_string())?;
    session
        .writer
        .write_all(data.as_bytes())
        .map_err(|e| e.to_string())?;
    session.writer.flush().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn resize_shell(cols: u16, rows: u16) -> Result<(), String> {
    let guard = APP_STATE.session.lock();
    let session = guard
        .as_ref()
        .ok_or_else(|| "shell not started".to_string())?;
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
            set_compat,
            write_shell,
            resize_shell
        ])
        .run(tauri::generate_context!());

    let _ = std::fs::remove_file(&*COMPAT_FLAG_PATH);
    let _ = std::fs::remove_file(&*COMPAT_PROFILE_PATH);
    result.expect("error while running tauri application");
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
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
    fn powershell_bootstrap_loads_compat_profile() {
        std::fs::write(&*COMPAT_FLAG_PATH, "1").expect("write compat flag");
        std::fs::write(&*COMPAT_PROFILE_PATH, POWERSHELL_COMPAT_PROFILE)
            .expect("write compat profile");

        let (program, mut arguments) = resolve_shell("powershell");
        arguments.retain(|argument| argument != "-NoExit");
        let command = arguments.last_mut().expect("PowerShell command argument");
        command.push_str("; 'alpha beta' | cut -d ' ' -f 2");

        let output = std::process::Command::new(program)
            .args(arguments)
            .env("WINGMAN_COMPAT_FLAG", &*COMPAT_FLAG_PATH)
            .env("WINGMAN_COMPAT_PROFILE", &*COMPAT_PROFILE_PATH)
            .output()
            .expect("run PowerShell bootstrap");

        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "beta");
        let _ = std::fs::remove_file(&*COMPAT_FLAG_PATH);
        let _ = std::fs::remove_file(&*COMPAT_PROFILE_PATH);
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
