//! Runtime configuration: **same JSON keys and shape** as Python `config.py` / `config.json`, but
//! **never** the same files as the Python app.
//!
//! Resolution order (Oxide only):
//! 1. `TUXTALKS_OXIDE_CONFIG` or `TUXTALKS_CONFIG` — explicit path (tests, packaging).
//! 2. `./tuxtalks-oxide.config.json` in the current working directory.
//! 3. `tuxtalks-oxide.config.json` next to this crate (`CARGO_MANIFEST_DIR`).
//! 4. `~/.config/tuxtalks-oxide/config.json`.
//!
//! Defaults match Python `DEFAULTS` where applicable; Oxide keeps its own library index at
//! `~/.local/share/tuxtalks-oxide/library.db` (not `tuxtalks/`). Environment overrides follow
//! Python: `JRIVER_<KEY>` or `<KEY>` for mapped fields, then `TUXTALKS_MPRIS_SERVICE` /
//! `TUXTALKS_WAKE_WORD`.

use crate::utils::speaker::Speaker;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct PlayerConfig {
    /// Same semantics as Python `PLAYER` (`jriver`, `strawberry`, `elisa`, `mpris`).
    pub player: String,
    pub mpris_service: Option<String>,
    pub jriver_ip: String,
    pub jriver_port: u16,
    pub jriver_access_key: String,
    /// `JRiver` executable name used by autostart. Default `mediacenter35` matches
    /// Python `players/jriver.py::health_check`. Override with `JRIVER_BINARY`
    /// when you bump `JRiver` versions (e.g. `mediacenter36`).
    pub jriver_binary: String,
    pub strawberry_db_path: String,
    pub elisa_db_path: String,
    pub library_path: String,
    pub library_db_path: String,
    pub wake_word: String,
}

impl PlayerConfig {
    /// Load merged configuration (file + env). See module docs for paths and overrides.
    pub fn load() -> Self {
        let home = std::env::var("HOME").unwrap_or_default();
        let mut c = Self {
            player: "jriver".to_string(),
            mpris_service: Some("org.mpris.MediaPlayer2.vlc".to_string()),
            jriver_ip: "localhost".to_string(),
            jriver_port: 52199,
            jriver_access_key: String::new(),
            jriver_binary: "mediacenter35".to_string(),
            strawberry_db_path: format!("{home}/.local/share/strawberry/strawberry/strawberry.db"),
            elisa_db_path: format!("{home}/.local/share/elisa/elisaDatabase.db"),
            library_path: format!("{home}/Music"),
            library_db_path: format!("{home}/.local/share/tuxtalks-oxide/library.db"),
            wake_word: "Alice".to_string(),
        };

        if let Some(path) = resolve_config_path() {
            match std::fs::read_to_string(&path) {
                Ok(text) => match serde_json::from_str::<Value>(&text) {
                    Ok(v) => merge_from_json(&mut c, &v),
                    Err(e) => tracing::warn!(
                        "Could not parse {}: {e} — keeping defaults for unmapped keys",
                        path.display()
                    ),
                },
                Err(e) => tracing::warn!("Could not read {}: {e}", path.display()),
            }
        }

        apply_python_style_env(&mut c);
        apply_tuxtalks_rust_env(&mut c);

        c.player = c.player.trim().to_lowercase();
        if c.player.is_empty() {
            c.player = "jriver".to_string();
        }

        c
    }
}

/// Cwd / crate-local filename — avoids reading Python's `./config.json`.
const OXIDE_CONFIG_FILENAME: &str = "tuxtalks-oxide.config.json";

fn resolve_config_path() -> Option<PathBuf> {
    for (var_name, path_str) in [
        (
            "TUXTALKS_OXIDE_CONFIG",
            std::env::var("TUXTALKS_OXIDE_CONFIG").ok(),
        ),
        ("TUXTALKS_CONFIG", std::env::var("TUXTALKS_CONFIG").ok()),
    ] {
        if let Some(p) = path_str {
            let pb = PathBuf::from(&p);
            if pb.is_file() {
                return Some(pb);
            }
            tracing::warn!("{var_name} set but file missing: {}", pb.display());
            return None;
        }
    }

    let cwd_cfg = std::env::current_dir().ok()?.join(OXIDE_CONFIG_FILENAME);
    if cwd_cfg.is_file() {
        return Some(cwd_cfg);
    }

    let manifest_cfg = Path::new(env!("CARGO_MANIFEST_DIR")).join(OXIDE_CONFIG_FILENAME);
    if manifest_cfg.is_file() {
        return Some(manifest_cfg);
    }

    let home = std::env::var("HOME").ok()?;
    let system = PathBuf::from(&home).join(".config/tuxtalks-oxide/config.json");
    if system.is_file() {
        return Some(system);
    }

    None
}

fn merge_from_json(c: &mut PlayerConfig, v: &Value) {
    if let Some(s) = v.get("PLAYER").and_then(Value::as_str) {
        c.player = s.to_string();
    }
    if let Some(s) = v.get("MPRIS_SERVICE").and_then(Value::as_str) {
        c.mpris_service = Some(s.to_string());
    }
    if let Some(s) = v.get("JRIVER_IP").and_then(Value::as_str) {
        c.jriver_ip = s.to_string();
    }
    if let Some(n) = v.get("JRIVER_PORT").and_then(json_u16) {
        c.jriver_port = n;
    }
    if let Some(s) = v.get("ACCESS_KEY").and_then(Value::as_str) {
        c.jriver_access_key = s.to_string();
    }
    if let Some(s) = v.get("JRIVER_BINARY").and_then(Value::as_str) {
        c.jriver_binary = s.to_string();
    }
    if let Some(s) = v.get("STRAWBERRY_DB_PATH").and_then(Value::as_str) {
        c.strawberry_db_path = s.to_string();
    }
    if let Some(s) = v.get("LIBRARY_PATH").and_then(Value::as_str) {
        c.library_path = s.to_string();
    }
    if let Some(s) = v.get("WAKE_WORD").and_then(Value::as_str) {
        c.wake_word = s.to_string();
    }
}

fn json_u16(v: &Value) -> Option<u16> {
    v.as_u64()
        .and_then(|n| u16::try_from(n).ok())
        .or_else(|| v.as_str()?.parse().ok())
}

fn env_for_python_key(key: &str) -> Option<String> {
    let prefixed = format!("JRIVER_{key}");
    std::env::var(prefixed)
        .or_else(|_| std::env::var(key))
        .ok()
        .filter(|s| !s.is_empty())
}

fn apply_python_style_env(c: &mut PlayerConfig) {
    if let Some(v) = env_for_python_key("PLAYER") {
        c.player = v;
    }
    if let Some(v) = env_for_python_key("MPRIS_SERVICE") {
        c.mpris_service = Some(v);
    }
    if let Some(v) = env_for_python_key("JRIVER_IP") {
        c.jriver_ip = v;
    }
    if let Some(v) = env_for_python_key("JRIVER_PORT") {
        if let Ok(p) = v.parse::<u16>() {
            c.jriver_port = p;
        }
    }
    if let Some(v) = env_for_python_key("ACCESS_KEY") {
        c.jriver_access_key = v;
    }
    if let Some(v) = env_for_python_key("JRIVER_BINARY") {
        c.jriver_binary = v;
    }
    if let Some(v) = env_for_python_key("STRAWBERRY_DB_PATH") {
        c.strawberry_db_path = v;
    }
    if let Some(v) = env_for_python_key("LIBRARY_PATH") {
        c.library_path = v;
    }
    if let Some(v) = env_for_python_key("WAKE_WORD") {
        c.wake_word = v;
    }
}

fn apply_tuxtalks_rust_env(c: &mut PlayerConfig) {
    if let Ok(v) = std::env::var("TUXTALKS_MPRIS_SERVICE") {
        if !v.is_empty() {
            c.mpris_service = Some(v);
        }
    }
    if let Ok(v) = std::env::var("TUXTALKS_WAKE_WORD") {
        if !v.is_empty() {
            c.wake_word = v;
        }
    }
}

pub struct PlayerContext {
    pub config: PlayerConfig,
    pub speaker: Arc<Speaker>,
    pub library: Option<Arc<crate::utils::library::LocalLibrary>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::{Mutex, OnceLock};

    static CONFIG_LOAD_TEST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

    fn config_load_test_lock() -> std::sync::MutexGuard<'static, ()> {
        CONFIG_LOAD_TEST_MUTEX
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("config test lock")
    }

    /// Keys that override `PlayerConfig::load` (must not leak from the developer's shell into tests).
    const ISOLATION_KEYS: &[&str] = &[
        "PLAYER",
        "JRIVER_PLAYER",
        "MPRIS_SERVICE",
        "JRIVER_MPRIS_SERVICE",
        "WAKE_WORD",
        "JRIVER_WAKE_WORD",
        "TUXTALKS_WAKE_WORD",
        "TUXTALKS_MPRIS_SERVICE",
        "JRIVER_IP",
        "JRIVER_JRIVER_IP",
        "JRIVER_PORT",
        "JRIVER_JRIVER_PORT",
        "ACCESS_KEY",
        "JRIVER_ACCESS_KEY",
        "JRIVER_BINARY",
        "JRIVER_JRIVER_BINARY",
        "STRAWBERRY_DB_PATH",
        "JRIVER_STRAWBERRY_DB_PATH",
        "LIBRARY_PATH",
        "JRIVER_LIBRARY_PATH",
        "TUXTALKS_CONFIG",
        "TUXTALKS_OXIDE_CONFIG",
    ];

    fn clear_config_env_for_test() -> Vec<(String, Option<String>)> {
        let saved: Vec<(String, Option<String>)> = ISOLATION_KEYS
            .iter()
            .map(|k| ((*k).to_string(), std::env::var(k).ok()))
            .collect();
        for k in ISOLATION_KEYS {
            std::env::remove_var(k);
        }
        saved
    }

    fn restore_env(saved: &[(String, Option<String>)]) {
        for (k, v) in saved {
            if let Some(val) = v {
                std::env::set_var(k, val);
            } else {
                std::env::remove_var(k);
            }
        }
    }

    #[test]
    fn load_merges_tuxtalks_config_file() {
        let _lock = config_load_test_lock();
        let saved = clear_config_env_for_test();
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            f,
            r#"{{"PLAYER": "mpris", "WAKE_WORD": "Hey", "MPRIS_SERVICE": "org.mpris.MediaPlayer2.test"}}"#
        )
        .unwrap();
        std::env::set_var("TUXTALKS_CONFIG", f.path().to_str().unwrap());
        let c = PlayerConfig::load();
        std::env::remove_var("TUXTALKS_CONFIG");
        restore_env(&saved);
        assert_eq!(c.player, "mpris");
        assert_eq!(c.wake_word, "Hey");
        assert_eq!(
            c.mpris_service.as_deref(),
            Some("org.mpris.MediaPlayer2.test")
        );
    }

    #[test]
    fn defaults_when_json_empty_object() {
        let _lock = config_load_test_lock();
        let saved = clear_config_env_for_test();
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(f, "{{}}").unwrap();
        std::env::set_var("TUXTALKS_CONFIG", f.path().to_str().unwrap());
        let c = PlayerConfig::load();
        std::env::remove_var("TUXTALKS_CONFIG");
        restore_env(&saved);
        assert_eq!(c.player, "jriver");
        assert_eq!(c.jriver_ip, "localhost");
        assert_eq!(c.jriver_port, 52199);
        assert_eq!(c.jriver_binary, "mediacenter35");
        assert_eq!(c.wake_word, "Alice");
        assert_eq!(
            c.mpris_service.as_deref(),
            Some("org.mpris.MediaPlayer2.vlc")
        );
        assert!(
            c.library_db_path.contains("tuxtalks-oxide"),
            "library DB must not default under tuxtalks/ (Python)"
        );
    }
}
