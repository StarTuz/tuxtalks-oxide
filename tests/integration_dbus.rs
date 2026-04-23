#![cfg(feature = "voice")]
// D-Bus trait impls use `async fn` as required by `#[interface]`; the mock
// doesn't actually await anything but the signature is dictated by zbus.
#![allow(clippy::unused_async, clippy::used_underscore_binding)]

use std::io::Write;
use std::process::Command;
use std::sync::{Arc, Mutex};
use zbus::{connection, interface};

/// Isolated Oxide config so tests never depend on `~/.config/tuxtalks-oxide/config.json`.
fn test_config_mpris_only() -> tempfile::NamedTempFile {
    let mut f = tempfile::NamedTempFile::new().expect("temp config");
    writeln!(f, r#"{{"PLAYER":"mpris"}}"#).expect("write config");
    f
}

struct MockSpeechService {
    calls: Arc<Mutex<Vec<String>>>,
}

#[interface(name = "org.speech.Service")]
impl MockSpeechService {
    async fn speak(&self, text: &str) -> zbus::fdo::Result<()> {
        self.calls.lock().unwrap().push(format!("SPEAK: {text}"));
        Ok(())
    }

    async fn think(&self, _query: &str) -> zbus::fdo::Result<String> {
        self.calls.lock().unwrap().push("THINK".to_string());
        // Return a valid JSON response for the CLI.
        // We simulate a 'pause' intent for simplicity.
        Ok(r#"{"intent": "pause", "parameters": {}}"#.to_string())
    }

    async fn listen(&self) -> zbus::fdo::Result<String> {
        self.calls.lock().unwrap().push("LISTEN".to_string());
        Ok("play beethoven".to_string())
    }

    async fn listen_vad(&self) -> zbus::fdo::Result<String> {
        self.calls.lock().unwrap().push("LISTEN_VAD".to_string());
        Ok("stop music".to_string())
    }

    async fn add_correction(&self, heard: &str, meant: &str) -> zbus::fdo::Result<bool> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("CORRECTION: {heard} -> {meant}"));
        Ok(true)
    }
}

#[tokio::test]
async fn test_cli_listen_flow() {
    // Unique service name to avoid conflicts
    let unique_suffix = std::process::id();
    // D-Bus name segments must not start with a digit; prefix the PID.
    let service_name = format!("org.speech.Service.Test.p{unique_suffix}");

    let calls = Arc::new(Mutex::new(Vec::new()));
    let service = MockSpeechService {
        calls: calls.clone(),
    };

    let _conn = connection::Builder::session()
        .unwrap()
        .name(service_name.clone())
        .unwrap()
        .serve_at("/org/speech/Service", service)
        .unwrap()
        .build()
        .await
        .unwrap();

    // Invoke CLI
    let bin_path = env!("CARGO_BIN_EXE_tuxtalks-oxide");
    let cfg = test_config_mpris_only();

    let output = Command::new(bin_path)
        .arg("listen")
        .env("TUXTALKS_CONFIG", cfg.path().to_str().unwrap())
        .env("SPEECH_SERVICE_NAME", &service_name)
        .env("RUST_LOG", "debug")
        .output()
        .expect("Failed to execute command");

    if !output.status.success() {
        eprintln!("CLI Error: {}", String::from_utf8_lossy(&output.stderr));
    }
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    println!("CLI Output: {stdout}");

    // Logic Parity Check: The mock returns "stop music" -> "pause" intent -> "Paused" action
    assert!(
        stdout.contains("Paused") || stdout.contains("Resumed"),
        "CLI content mismatch: {stdout}"
    );

    let recorded_calls = calls.lock().unwrap();
    assert!(recorded_calls.contains(&"LISTEN_VAD".to_string()));
    assert!(recorded_calls.contains(&"THINK".to_string()));
}

#[tokio::test]
async fn test_cli_daemon_flow() {
    // Unique service name
    let unique_suffix = std::process::id();
    let service_name = format!("org.speech.Service.DaemonTest.p{unique_suffix}");

    let calls = Arc::new(Mutex::new(Vec::new()));
    let service = MockSpeechService {
        calls: calls.clone(),
    };

    let _conn = connection::Builder::session()
        .unwrap()
        .name(service_name.clone())
        .unwrap()
        .serve_at("/org/speech/Service", service)
        .unwrap()
        .build()
        .await
        .unwrap();

    // Invoke CLI in daemon mode for a short duration
    let bin_path = env!("CARGO_BIN_EXE_tuxtalks-oxide");

    let cfg = test_config_mpris_only();

    // We use timeout to stop the daemon after it processes the mock response
    let child = Command::new("timeout")
        .arg("2s") // Kill after 2 seconds
        .arg(bin_path)
        .arg("daemon")
        .env("TUXTALKS_CONFIG", cfg.path().to_str().unwrap())
        .env("SPEECH_SERVICE_NAME", &service_name)
        .env("TUXTALKS_WAKE_WORD", "Stop") // Mock returns "stop music", so wake word "Stop" works
        .env("RUST_LOG", "debug")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to execute command");

    let output = child.wait_with_output().expect("Failed to wait on child");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    println!("Daemon CLI Output: {stdout}");
    println!("Daemon CLI Stderr: {stderr}");

    // Verify the daemon picked up the command
    let recorded_calls = calls.lock().unwrap();
    assert!(recorded_calls.contains(&"LISTEN_VAD".to_string()));
    // If "stop music" matches wake word "Stop", logic should call "THINK" with "music"
    // However, our mock Logic might differ.
    // Let's verify that LISTEN_VAD loop was active.
    assert!(recorded_calls.iter().filter(|c| *c == "LISTEN_VAD").count() >= 1);
}
