use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;
use wingman_lib::transport::EditorReadinessBrokerV1;

#[test]
fn powershell_non_filesystem_location_never_starts_the_runner() {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("powershell_runner_transport.ps1");
    let quoted_script = script.display().to_string().replace('\'', "''");
    let command = format!(
        "& {{ . '{quoted_script}'; Set-Location 'HKLM:\\'; Invoke-WingmanPrepared -RequestId '0123456789abcdef0123456789abcdef'; exit $LASTEXITCODE }}"
    );

    let output = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &command,
        ])
        .env("WINGMAN_RUNNER_PATH", env!("CARGO_BIN_EXE_wingman-runner"))
        .env("WINGMAN_BROKER_PIPE", r"\\.\pipe\must-not-connect")
        .output()
        .expect("run Windows PowerShell transport shim");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"wingman: Familiar commands require a FileSystem location\r\n"
    );
}

#[test]
fn powershell_rejects_a_malformed_request_id_before_starting_the_runner() {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("powershell_runner_transport.ps1");
    let quoted_script = script.display().to_string().replace('\'', "''");
    let command = format!(
        "& {{ . '{quoted_script}'; Invoke-WingmanPrepared -RequestId 'not-an-id'; exit $LASTEXITCODE }}"
    );

    let output = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &command,
        ])
        .env(
            "WINGMAN_RUNNER_PATH",
            r"C:\definitely-missing\wingman-runner.exe",
        )
        .output()
        .expect("run Windows PowerShell transport shim");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, b"wingman: invalid prepared request ID\r\n");
}

#[test]
fn powershell_direct_prompt_call_does_not_emit_a_private_marker() {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("powershell_runner_transport.ps1");
    let quoted_script = script.display().to_string().replace('\'', "''");
    let command = format!(
        "& {{ . '{quoted_script}'; [Console]::Out.Write((prompt)); [Console]::Out.Write((prompt)) }}"
    );
    let nonce = "abcdef0123456789abcdef0123456789";

    let output = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &command,
        ])
        .env("WINGMAN_SESSION_NONCE", nonce)
        .output()
        .expect("run Windows PowerShell prompt integration");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 prompt output");
    assert!(
        !stdout.contains("wingman-prompt"),
        "a direct prompt call forged an editor marker: {stdout:?}"
    );
    assert!(
        stdout.matches("PS ").count() >= 2,
        "prompt hidden: {stdout:?}"
    );
}

#[test]
fn powershell_prompt_nonce_is_not_inherited_by_child_processes() {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("powershell_runner_transport.ps1");
    let quoted_script = script.display().to_string().replace('\'', "''");
    let command = format!(
        "& {{ . '{quoted_script}'; [Console]::Out.Write(\"PARENT=$env:WINGMAN_SESSION_NONCE,$env:WINGMAN_READINESS_PIPE;\"); & powershell.exe -NoLogo -NoProfile -Command '\"$env:WINGMAN_SESSION_NONCE,$env:WINGMAN_READINESS_PIPE\"' }}"
    );
    let nonce = "abcdef0123456789abcdef0123456789";

    let output = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &command,
        ])
        .env("WINGMAN_SESSION_NONCE", nonce)
        .env("WINGMAN_READINESS_PIPE", "must-not-exist")
        .output()
        .expect("run Windows PowerShell child inheritance probe");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 prompt output");
    assert_eq!(stdout, "PARENT=,;");
}

#[test]
fn performance_history_is_isolated_without_leaking_the_control_flag() {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("powershell_runner_transport.ps1");
    let quoted_script = script.display().to_string().replace('\'', "''");
    let command = format!(
        "& {{ . '{quoted_script}'; [Console]::Out.Write(\"$((Get-PSReadLineOption).HistorySaveStyle)|$env:WINGMAN_PERF_ISOLATED_HISTORY\") }}"
    );

    let output = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &command,
        ])
        .env("WINGMAN_SESSION_NONCE", "abcdef0123456789abcdef0123456789")
        .env("WINGMAN_READINESS_PIPE", "wingman-history-test")
        .env("WINGMAN_PERF_ISOLATED_HISTORY", "1")
        .output()
        .expect("inspect isolated PSReadLine history mode");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, b"SaveNothing|");
}

#[test]
fn ordinary_transport_preserves_the_default_psreadline_history_style() {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("powershell_runner_transport.ps1");
    let quoted_script = script.display().to_string().replace('\'', "''");
    let command = format!(
        "& {{ . '{quoted_script}'; [Console]::Out.Write((Get-PSReadLineOption).HistorySaveStyle) }}"
    );

    let output = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &command,
        ])
        .env("WINGMAN_SESSION_NONCE", "abcdef0123456789abcdef0123456789")
        .env("WINGMAN_READINESS_PIPE", "wingman-history-test")
        .output()
        .expect("inspect ordinary PSReadLine history mode");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, b"SaveIncrementally");
}

#[test]
fn powershell_registers_a_dedicated_psreadline_replacement_chord() {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("powershell_runner_transport.ps1");
    let quoted_script = script.display().to_string().replace('\'', "''");
    let command = format!(
        "& {{ . '{quoted_script}'; Get-PSReadLineKeyHandler | Where-Object {{ $_.Key -eq 'Ctrl+x,Ctrl+w' }} | ForEach-Object {{ \"$($_.Key)|$($_.Function)\" }} }}"
    );
    let nonce = "abcdef0123456789abcdef0123456789";
    let pipe_id = format!(
        "wingman-readiness-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let pipe_name = format!(r"\\.\pipe\{pipe_id}");
    let broker = EditorReadinessBrokerV1::start(&pipe_name, nonce.to_string())
        .expect("start readiness broker");

    let output = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &command,
        ])
        .env("WINGMAN_SESSION_NONCE", nonce)
        .env("WINGMAN_READINESS_PIPE", &pipe_id)
        .output()
        .expect("inspect Windows PowerShell replacement chord");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        "Ctrl+x,Ctrl+w|WingmanReplaceLineV1"
    );
    broker.stop().expect("stop readiness broker");
}

#[test]
fn psreadline_chord_replaces_a_unicode_buffer_from_a_middle_cursor_position() {
    let integration_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("powershell_runner_transport.ps1");
    let nonce = "abcdef0123456789abcdef0123456789";
    let pipe_id = format!(
        "wingman-readiness-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let pipe_name = format!(r"\\.\pipe\{pipe_id}");
    let broker = EditorReadinessBrokerV1::start(&pipe_name, nonce.to_string())
        .expect("start readiness broker");
    let forbidden_path = std::env::temp_dir().join(format!(
        "wingman-replacement-must-not-run-{}.txt",
        Uuid::new_v4().as_simple()
    ));
    let _ = std::fs::remove_file(&forbidden_path);

    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("open PowerShell test PTY");
    let mut command = CommandBuilder::new("powershell.exe");
    command.args([
        "-NoLogo",
        "-NoProfile",
        "-NoExit",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        ". $env:WINGMAN_INTEGRATION_SCRIPT",
    ]);
    command.env("WINGMAN_INTEGRATION_SCRIPT", integration_path);
    command.env("WINGMAN_SESSION_NONCE", nonce);
    command.env("WINGMAN_READINESS_PIPE", &pipe_id);
    let mut child = pair
        .slave
        .spawn_command(command)
        .expect("spawn integrated PowerShell");
    let mut reader = pair
        .master
        .try_clone_reader()
        .expect("clone PowerShell reader");
    let mut writer = pair.master.take_writer().expect("take PowerShell writer");
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = [0u8; 4096];
        while let Ok(count) = reader.read(&mut bytes) {
            if count == 0 || sender.send(bytes[..count].to_vec()).is_err() {
                break;
            }
        }
    });

    let mut output = receive_until(&receiver, "PS ", Duration::from_secs(8));
    let first_readiness = receive_readiness(&broker, Duration::from_secs(8));
    assert_eq!(first_readiness.sequence, 1);

    let forbidden_literal = forbidden_path.display().to_string().replace('\'', "''");
    let original_line =
        format!("Set-Content -LiteralPath '{forbidden_literal}' -Value '한글 한 e\u{301} 🚀 漢字'");
    writer
        .write_all(original_line.as_bytes())
        .expect("type original PowerShell line");
    writer
        .write_all(b"\x1b[H\x1b[C\x1b[C\x1b[C\x1b[C")
        .expect("move to a middle cursor position");
    writer
        .write_all(
            b"\x18\x17[Console]::Out.Write((-join (87,73,78,71,77,65,78,95,82,69,80,76,65,67,69,68 | ForEach-Object { [char] $_ })))\r",
        )
        .expect("replace and submit PowerShell line");
    writer.flush().expect("flush PowerShell input");

    output.push_str(&receive_until(
        &receiver,
        "WINGMAN_REPLACED",
        Duration::from_secs(8),
    ));
    let _ = child.kill();
    broker.stop().expect("stop readiness broker");

    assert!(
        output.contains("WINGMAN_REPLACED"),
        "replacement did not run: {output:?}"
    );
    assert!(
        !output.contains("wingman-prompt"),
        "production transport unexpectedly emitted a prompt marker: {output:?}"
    );
    assert!(
        !forbidden_path.exists(),
        "the original editor buffer executed instead of being replaced"
    );
}

#[test]
fn psreadline_known_edits_match_the_unicode_mirror_contract() {
    let integration_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("powershell_runner_transport.ps1");
    let nonce = "abcdef0123456789abcdef0123456789";
    let pipe_id = format!(
        "wingman-readiness-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let pipe_name = format!(r"\\.\pipe\{pipe_id}");
    let broker = EditorReadinessBrokerV1::start(&pipe_name, nonce.to_string())
        .expect("start readiness broker");
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("open PowerShell edit probe PTY");
    let probe_setup = r#"
      . $env:WINGMAN_INTEGRATION_SCRIPT
      $script:WingmanBufferProbe = 0
      Set-PSReadLineKeyHandler -Chord 'Ctrl+x,Ctrl+q' -BriefDescription 'WingmanBufferProbeV1' -ScriptBlock {
        $line = $null
        $cursor = 0
        [Microsoft.PowerShell.PSConsoleReadLine]::GetBufferState([ref] $line, [ref] $cursor)
        $script:WingmanBufferProbe++
        $hex = [BitConverter]::ToString([Text.Encoding]::UTF8.GetBytes($line)).Replace('-', '')
        [Console]::Out.Write(('__WINGMAN_BUFFER_{0}__{1}:{2}__WINGMAN_BUFFER_{0}_END__' -f $script:WingmanBufferProbe, $hex, $cursor))
        [Microsoft.PowerShell.PSConsoleReadLine]::RevertLine()
      }
    "#;
    let mut command = CommandBuilder::new("powershell.exe");
    command.args([
        "-NoLogo",
        "-NoProfile",
        "-NoExit",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        probe_setup,
    ]);
    command.env("WINGMAN_INTEGRATION_SCRIPT", integration_path);
    command.env("WINGMAN_SESSION_NONCE", nonce);
    command.env("WINGMAN_READINESS_PIPE", &pipe_id);
    let mut child = pair
        .slave
        .spawn_command(command)
        .expect("spawn integrated PowerShell edit probe");
    let mut reader = pair
        .master
        .try_clone_reader()
        .expect("clone PowerShell edit probe reader");
    let mut writer = pair
        .master
        .take_writer()
        .expect("take PowerShell edit probe writer");
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = [0u8; 4096];
        while let Ok(count) = reader.read(&mut bytes) {
            if count == 0 || sender.send(bytes[..count].to_vec()).is_err() {
                break;
            }
        }
    });

    assert_eq!(
        receive_readiness(&broker, Duration::from_secs(8)).sequence,
        1
    );
    writer
        .write_all("pxwd 한글\u{1b}[H\u{1b}[C\u{1b}[3~\u{1b}[F\u{1b}[D!".as_bytes())
        .expect("type known Unicode edit sequence");
    writer
        .write_all(b"\x18\x11")
        .expect("request first PSReadLine buffer probe");
    writer.flush().expect("flush first edit probe");
    let first = receive_until(
        &receiver,
        "__WINGMAN_BUFFER_1_END__",
        Duration::from_secs(8),
    );
    assert_buffer_probe(&first, 1, "pwd 한!글", 6);

    writer
        .write_all("pwd e\u{301}\u{7f}".as_bytes())
        .expect("type combining-character edit sequence");
    writer
        .write_all(b"\x18\x11")
        .expect("request second PSReadLine buffer probe");
    writer.flush().expect("flush second edit probe");
    let second = receive_until(
        &receiver,
        "__WINGMAN_BUFFER_2_END__",
        Duration::from_secs(8),
    );
    assert_buffer_probe(&second, 2, "pwd e", 5);

    let _ = child.kill();
    broker.stop().expect("stop readiness broker");
}

#[test]
fn direct_prompt_call_cannot_signal_readiness_during_a_foreground_pipeline() {
    let integration_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("powershell_runner_transport.ps1");
    let nonce = "abcdef0123456789abcdef0123456789";
    let pipe_id = format!(
        "wingman-readiness-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let pipe_name = format!(r"\\.\pipe\{pipe_id}");
    let broker = EditorReadinessBrokerV1::start(&pipe_name, nonce.to_string())
        .expect("start readiness broker");
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("open PowerShell PTY");
    let mut command = CommandBuilder::new("powershell.exe");
    command.args([
        "-NoLogo",
        "-NoProfile",
        "-NoExit",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        ". $env:WINGMAN_INTEGRATION_SCRIPT",
    ]);
    command.env("WINGMAN_INTEGRATION_SCRIPT", integration_path);
    command.env("WINGMAN_SESSION_NONCE", nonce);
    command.env("WINGMAN_READINESS_PIPE", &pipe_id);
    let mut child = pair
        .slave
        .spawn_command(command)
        .expect("spawn integrated PowerShell");
    let mut reader = pair
        .master
        .try_clone_reader()
        .expect("clone PowerShell reader");
    let mut writer = pair.master.take_writer().expect("take PowerShell writer");
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = [0u8; 4096];
        while let Ok(count) = reader.read(&mut bytes) {
            if count == 0 || sender.send(bytes[..count].to_vec()).is_err() {
                break;
            }
        }
    });

    assert_eq!(
        receive_readiness(&broker, Duration::from_secs(8)).sequence,
        1
    );
    writer
        .write_all(
            b"prompt; Start-Sleep -Milliseconds 800; [Console]::Out.Write((-join (87,73,78,71,77,65,78,95,68,79,78,69 | ForEach-Object { [char] $_ })))\r",
        )
        .expect("submit direct prompt foreground pipeline");
    writer.flush().expect("flush direct prompt pipeline");
    thread::sleep(Duration::from_millis(250));
    assert!(
        broker.drain().expect("drain early readiness").is_empty(),
        "direct prompt call signaled readiness before the pipeline completed"
    );

    let output = receive_until(&receiver, "WINGMAN_DONE", Duration::from_secs(8));
    assert!(!output.contains("wingman-prompt"));
    assert_eq!(
        receive_readiness(&broker, Duration::from_secs(8)).sequence,
        2
    );
    let _ = child.kill();
    broker.stop().expect("stop readiness broker");
}

fn receive_readiness(
    broker: &EditorReadinessBrokerV1,
    timeout: Duration,
) -> wingman_lib::transport::EditorReadinessFrameV1 {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let frames = broker.drain().expect("drain editor readiness");
        if let Some(frame) = frames.into_iter().next() {
            return frame;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for editor readiness");
}

fn receive_until(receiver: &mpsc::Receiver<Vec<u8>>, needle: &str, timeout: Duration) -> String {
    let deadline = Instant::now() + timeout;
    let mut output = String::new();
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(remaining) {
            Ok(bytes) => {
                output.push_str(&String::from_utf8_lossy(&bytes));
                if output.contains(needle) {
                    return output;
                }
            }
            Err(_) => break,
        }
    }
    panic!("timed out waiting for {needle:?}; output: {output:?}");
}

fn assert_buffer_probe(output: &str, index: usize, expected_line: &str, expected_cursor: usize) {
    let prefix = format!("WINGMAN_BUFFER_{index}__");
    let suffix = format!("__WINGMAN_BUFFER_{index}_END__");
    let start = output
        .find(&prefix)
        .unwrap_or_else(|| panic!("missing buffer probe prefix in {output:?}"))
        + prefix.len();
    let end = output[start..]
        .find(&suffix)
        .map(|offset| start + offset)
        .unwrap_or_else(|| panic!("missing buffer probe suffix in {output:?}"));
    let (actual_hex, actual_cursor) = output[start..end]
        .split_once(':')
        .unwrap_or_else(|| panic!("malformed buffer probe in {output:?}"));
    let expected_hex = expected_line
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<String>();

    assert_eq!(actual_hex, expected_hex, "PSReadLine buffer mismatch");
    assert_eq!(
        actual_cursor
            .parse::<usize>()
            .expect("numeric probe cursor"),
        expected_cursor,
        "PSReadLine cursor mismatch"
    );
}
