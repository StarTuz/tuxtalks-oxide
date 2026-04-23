//! Voice assistant loop — **feature-gated and pending rework**.
//!
//! Status: **drift from reference Python behavior.** Python routes commands through
//! `text_normalizer` → `command_processor` → `voice_fingerprint`, with Vosk/Wyoming
//! as the ASR. This module instead delegates to SpeechD-NG on D-Bus and asks an LLM
//! (`think`) to emit a JSON intent (see `intelligence/intent.rs`). That is not how
//! the Python app decides what to do, and it pulls Oxide toward a home-assistant
//! shape which is out of scope.
//!
//! This module is compiled only with `--features voice` and must be reworked to
//! mirror the Python pipeline before being re-enabled by default.

use crate::config::PlayerContext;
use crate::integration::speech_service::SpeechServiceProxy;
use crate::intelligence::intent::{Intent, IntentEngine};
use crate::players::manager::PlayerManager;
use crate::Result;
use std::sync::Arc;
use zbus::Connection;

pub struct VoiceLoop {
    ctx: Arc<PlayerContext>,
}

impl VoiceLoop {
    #[must_use]
    pub fn new(ctx: Arc<PlayerContext>) -> Self {
        Self { ctx }
    }

    /// Run the voice loop until the process is signalled to exit.
    ///
    /// # Errors
    /// Returns an error if the D-Bus session cannot be established or any
    /// of the downstream speech-service / player calls fail fatally.
    pub async fn run(&self) -> Result<()> {
        let conn = Connection::session().await?;
        // Use the connection helper to respect SPEECH_SERVICE_NAME if set (e.g. for tests)
        let speech = crate::integration::speech_service::connect(&conn).await?;

        // Initialize player manager
        let mut manager = PlayerManager::new(&self.ctx);
        // Auto-select active player if possible (e.g. first one)
        if let Ok(p) = manager.get_active() {
            tracing::info!("Active player: {}", p.id());
        } else {
            tracing::warn!("No active player found at startup");
        }

        let wake_word = self.ctx.config.wake_word.to_lowercase();
        println!("Tuxtalks listening... Wake word: '{wake_word}'");
        self.ctx
            .speaker
            .speak(format!("Listening for {wake_word}"))
            .await;

        loop {
            // 1. Listen (VAD)
            // We use standard listen_vad which blocks until speech is detected
            let text = match speech.listen_vad().await {
                Ok(t) => t,
                Err(e) => {
                    tracing::error!("Speech recognition failed: {}", e);
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    continue;
                }
            };

            let trimmed = text.trim();
            if trimmed.is_empty() {
                continue;
            }

            tracing::debug!("Heard: {}", trimmed);

            // 2. Check Wake Word
            let lower_text = trimmed.to_lowercase();
            if lower_text.starts_with(&wake_word) {
                // Determine command part
                let command_part = trimmed[wake_word.len()..].trim();

                if command_part.is_empty() {
                    self.ctx.speaker.speak("Yes?").await;
                    continue;
                }

                tracing::info!("Wake word detected! Processing: '{}'", command_part);

                // 3. Process Command
                if let Err(e) = self
                    .process_command(&speech, &mut manager, command_part)
                    .await
                {
                    tracing::error!("Failed to process command: {}", e);
                    self.ctx.speaker.speak("Sorry, something went wrong.").await;
                }
            }
        }
    }

    async fn process_command(
        &self,
        speech: &SpeechServiceProxy<'_>,
        manager: &mut PlayerManager,
        text: &str,
    ) -> Result<()> {
        // 1. Get Library Context
        let library_summary = if let Some(lib) = &self.ctx.library {
            lib.get_summary().ok()
        } else {
            None
        };

        // 2. Think (AI)
        let prompt = IntentEngine::construct_prompt(text, library_summary.as_deref());
        let json_response = speech.think(&prompt).await?;

        // 3. Parse Intent
        let intent = match IntentEngine::parse_response(&json_response) {
            Ok(i) => i,
            Err(e) => {
                tracing::error!("Intent parsing failed: {}", e);
                return Ok(());
            }
        };

        tracing::info!("Intent: {:?}", intent);

        // 4. Execute
        // We get the active player freshly each time in case it changed
        let Ok(p) = manager.get_active() else {
            self.ctx
                .speaker
                .speak("No media player is connected.")
                .await;
            return Ok(());
        };

        let action_result = match intent {
            Intent::PlayArtist { artist } => {
                p.play_artist(&crate::Artist(artist.clone())).await?;
                format!("Playing artist {artist}")
            }
            Intent::PlayAlbum { album } => {
                p.play_album(&crate::Album(album.clone())).await?;
                format!("Playing album {album}")
            }
            Intent::PlayTrack { track } => {
                p.play_any(&track).await?;
                format!("Playing {track}")
            }
            Intent::PlayGenre { genre } => {
                p.play_genre(&crate::Genre(genre.clone())).await?;
                format!("Playing genre {genre}")
            }
            Intent::PlayPlaylist { name } => {
                p.play_playlist(&name).await?;
                format!("Playing playlist {name}")
            }
            Intent::VolumeUp {} => {
                p.volume_up().await?;
                "Volume up".to_string()
            }
            Intent::VolumeDown {} => {
                p.volume_down().await?;
                "Volume down".to_string()
            }
            Intent::NextTrack {} => {
                p.next_track().await?;
                "Next track".to_string()
            }
            Intent::PreviousTrack {} => {
                p.previous_track().await?;
                "Previous track".to_string()
            }
            Intent::Pause {} => {
                p.pause().await?;
                "Paused".to_string()
            }
            Intent::Resume {} => {
                p.play().await?;
                "Resumed".to_string()
            }
            Intent::Stop {} => {
                p.stop().await?;
                "Stopped".to_string()
            }
            Intent::WhatIsPlaying {} => {
                let status = p.what_is_playing().await?;
                speech.speak(&status).await?;
                status
            }
            Intent::GameCommand { command } => {
                format!("Game command: {command}")
            }
            Intent::Unknown {} => "I didn't understand that.".to_string(),
        };

        // Feedback
        // If the action result is short/meaningful, we might speak it?
        // For now, Play* commands usually have visual/audio feedback from the player itself.
        // But "Paused", "Resumed" etc might benefit from confirmation.
        // The original Python code spoke the confirmation.
        // Let's speak it if it's not a playback start (which would talk over the music).
        if !action_result.starts_with("Playing") {
            crate::utils::speaker::Speaker::new()
                .0
                .speak(&action_result)
                .await;
        }

        println!("Action: {action_result}");
        Ok(())
    }
}
