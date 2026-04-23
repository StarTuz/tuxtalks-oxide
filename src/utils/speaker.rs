//! Text-to-speech speaker.
//!
//! The [`Speaker`] is a thin producer: it pushes lines into an `mpsc` channel.
//! The worker spawned by [`spawn_tts_worker`] drains that channel and routes
//! each line through the first available backend:
//!
//! 1. **speechd-ng** via D-Bus (`org.speech.Service.Speak`) — primary.
//!    See `/home/startux/Code/speechd-ng`.
//! 2. **`spd-say`** subprocess — fallback for systems without speechd-ng.
//! 3. **`tracing` log** — last resort, so nothing is silently dropped.
//!
//! Backend selection can be forced with the `TUXTALKS_TTS` env var:
//! `speechd` / `spd-say` / `off`. The default is `auto` (1 → 2 → 3).

use tokio::process::Command;
use tokio::sync::mpsc;
use zbus::{proxy, Connection};

#[proxy(
    interface = "org.speech.Service",
    default_service = "org.speech.Service",
    default_path = "/org/speech/Service",
    gen_blocking = false
)]
trait TtsSpeak {
    /// Synchronous speech request. Returns once the daemon has accepted
    /// the message (playback itself is queued on the daemon side).
    fn speak(&self, text: &str) -> zbus::Result<()>;
}

/// Producer side of the TTS channel.
pub struct Speaker {
    sender: mpsc::Sender<String>,
}

impl Speaker {
    #[must_use]
    pub fn new() -> (Self, mpsc::Receiver<String>) {
        let (tx, rx) = mpsc::channel(100);
        (Self { sender: tx }, rx)
    }

    /// Queue an utterance. Never blocks the caller beyond channel capacity.
    pub async fn speak(&self, text: impl Into<String>) {
        let msg = text.into();
        tracing::debug!(target: "tts", "queue: {msg}");
        if let Err(e) = self.sender.send(msg).await {
            tracing::error!("speaker channel closed: {e}");
        }
    }

    /// Announce the currently-playing track. The message uses the backend's
    /// `summary` field, which already embeds track / disc info when available
    /// (mirrors Python `players/jriver.py::what_is_playing`).
    pub async fn announce_now_playing(&self, np: &crate::NowPlaying) {
        self.speak(format!("Playing {}.", np.summary)).await;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TtsMode {
    /// speechd-ng → spd-say → log.
    Auto,
    /// speechd-ng only; drop on failure.
    SpeechdNg,
    /// spd-say only; drop on failure.
    SpdSay,
    /// Silent. Messages are still logged at `info` so they show up in traces.
    Off,
}

impl TtsMode {
    fn from_env() -> Self {
        let raw = std::env::var("TUXTALKS_TTS").ok();
        match raw.as_deref().map(str::trim).map(str::to_ascii_lowercase) {
            Some(s) => match s.as_str() {
                "speechd" | "speechd-ng" | "speechd_ng" => Self::SpeechdNg,
                "spd-say" | "spd_say" | "speech-dispatcher" => Self::SpdSay,
                "off" | "none" | "mute" => Self::Off,
                _ => Self::Auto,
            },
            None => Self::Auto,
        }
    }
}

/// Spawn the worker that drains a [`Speaker`]'s receiver and speaks each
/// message through the best available backend.
///
/// Reconnects to speechd-ng on demand if the D-Bus session drops.
pub fn spawn_tts_worker(mut rx: mpsc::Receiver<String>) {
    tokio::spawn(async move {
        let mode = TtsMode::from_env();
        tracing::info!("tts: worker starting in {:?} mode", mode);
        let mut speechd = if matches!(mode, TtsMode::Auto | TtsMode::SpeechdNg) {
            connect_speechd().await
        } else {
            None
        };
        if speechd.is_some() {
            tracing::info!("tts: connected to speechd-ng");
        }

        while let Some(msg) = rx.recv().await {
            tracing::info!("tts: {msg}");
            match mode {
                TtsMode::Off => {}
                TtsMode::SpeechdNg => {
                    if speak_via_speechd(&mut speechd, &msg).await.is_err() {
                        tracing::warn!("tts: speechd-ng unavailable, message dropped");
                    }
                }
                TtsMode::SpdSay => {
                    if speak_via_spd_say(&msg).await.is_err() {
                        tracing::warn!("tts: spd-say unavailable, message dropped");
                    }
                }
                TtsMode::Auto => {
                    if speak_via_speechd(&mut speechd, &msg).await.is_err()
                        && speak_via_spd_say(&msg).await.is_err()
                    {
                        tracing::warn!(
                            "tts: no backend available (speechd-ng / spd-say); logged only"
                        );
                    }
                }
            }
        }
        tracing::debug!("tts: worker channel closed, exiting");
    });
}

async fn connect_speechd() -> Option<Connection> {
    match Connection::session().await {
        Ok(conn) => Some(conn),
        Err(e) => {
            tracing::debug!("tts: no session bus ({e})");
            None
        }
    }
}

async fn speak_via_speechd(slot: &mut Option<Connection>, msg: &str) -> Result<(), ()> {
    if slot.is_none() {
        *slot = connect_speechd().await;
    }
    let Some(conn) = slot.as_ref() else {
        return Err(());
    };

    // Proxy is cheap (wraps the connection + service/path/interface strings),
    // so we build per-call and let zbus cache on its side.
    let proxy = match TtsSpeakProxy::new(conn).await {
        Ok(p) => p,
        Err(e) => {
            tracing::debug!("tts: speechd-ng proxy build failed: {e}");
            *slot = None;
            return Err(());
        }
    };
    match proxy.speak(msg).await {
        Ok(()) => Ok(()),
        Err(e) => {
            tracing::debug!("tts: speechd-ng speak failed: {e}");
            // Drop the connection so the next message forces a reconnect.
            *slot = None;
            Err(())
        }
    }
}

async fn speak_via_spd_say(msg: &str) -> Result<(), ()> {
    // `--wait` so the subprocess doesn't return before synth, keeping ordering
    // intact when multiple announcements stack up.
    match Command::new("spd-say")
        .arg("--wait")
        .arg(msg)
        .status()
        .await
    {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => {
            tracing::debug!("tts: spd-say exited with {status}");
            Err(())
        }
        Err(e) => {
            tracing::debug!("tts: spd-say not available: {e}");
            Err(())
        }
    }
}
