use once_cell::sync::Lazy;
use parking_lot::Mutex;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use std::io::{Read, Write};
use std::thread;
use tauri::{AppHandle, Emitter};

#[derive(Clone, Serialize)]
struct SessionInfo {
  shell: String,
  cwd: String,
}

struct PtySession {
  writer: Box<dyn Write + Send>,
  master: Box<dyn MasterPty + Send>,
  _child: Box<dyn Child + Send + Sync>,
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
        "-Command".into(),
        "chcp 65001 | Out-Null; [Console]::InputEncoding = New-Object System.Text.UTF8Encoding $false; [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false".into(),
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
fn start_shell(app: AppHandle, shell: String, cols: u16, rows: u16) -> Result<SessionInfo, String> {
  {
    let mut guard = APP_STATE.session.lock();
    *guard = None;
  }

  let pty_system = native_pty_system();
  let pair = pty_system
    .openpty(PtySize {
      rows: rows.max(10),
      cols: cols.max(40),
      pixel_width: 0,
      pixel_height: 0,
    })
    .map_err(|e| e.to_string())?;

  let (program, args) = resolve_shell(&shell);
  let mut cmd = CommandBuilder::new(program);
  for arg in args {
    cmd.arg(arg);
  }
  if let Ok(cwd) = std::env::current_dir() {
    cmd.cwd(cwd);
  }

  let child = pair
    .slave
    .spawn_command(cmd)
    .map_err(|e| e.to_string())?;

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
      writer,
      master: pair.master,
      _child: child,
      shell: shell_name.clone(),
      cwd: cwd.clone(),
    });
  }

  let app_handle = app.clone();
  thread::spawn(move || {
    let mut buf = [0u8; 8192];
    loop {
      match reader.read(&mut buf) {
        Ok(0) => break,
        Ok(n) => {
          let chunk = String::from_utf8_lossy(&buf[..n]).to_string();
          let _ = app_handle.emit("pty-output", chunk);
        }
        Err(_) => break,
      }
    }
  });

  let _ = app.emit("cwd-changed", cwd.clone());
  Ok(SessionInfo {
    shell: shell_name,
    cwd,
  })
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
    .resize(PtySize {
      rows: rows.max(10),
      cols: cols.max(40),
      pixel_width: 0,
      pixel_height: 0,
    })
    .map_err(|e| e.to_string())?;
  Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .plugin(tauri_plugin_shell::init())
    .invoke_handler(tauri::generate_handler![
      get_cwd,
      start_shell,
      write_shell,
      resize_shell
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
