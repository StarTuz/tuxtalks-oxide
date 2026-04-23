use crate::config::PlayerContext;
use crate::players::elisa::ElisaPlayer;
use crate::players::jriver::JRiverPlayer;
use crate::players::mpris::MprisPlayer;
use crate::players::strawberry::StrawberryPlayer;
use crate::{MediaPlayer, PlayerError, Result};
use std::collections::HashMap;
use std::sync::Arc;

pub struct PlayerManager {
    players: HashMap<String, Box<dyn MediaPlayer>>,
    active_player: Option<String>,
    /// Used when `--player` is not passed; matches Python `cfg.get("PLAYER")`.
    default_player_id: String,
}

impl PlayerManager {
    #[must_use]
    pub fn new(ctx: &Arc<PlayerContext>) -> Self {
        let mut players: HashMap<String, Box<dyn MediaPlayer>> = HashMap::new();
        let pid = ctx.config.player.to_lowercase();
        let default_player_id = match pid.as_str() {
            "jriver" => {
                let p = JRiverPlayer::new(ctx.clone());
                players.insert("jriver".to_string(), Box::new(p));
                "jriver".to_string()
            }
            "strawberry" => {
                if ctx.config.strawberry_db_path.is_empty() {
                    tracing::warn!("PLAYER=strawberry but STRAWBERRY_DB_PATH is empty");
                } else {
                    let p = StrawberryPlayer::new(ctx.clone());
                    players.insert("strawberry".to_string(), Box::new(p));
                }
                "strawberry".to_string()
            }
            "elisa" => {
                let p = ElisaPlayer::new(ctx.clone());
                players.insert("elisa".to_string(), Box::new(p));
                "elisa".to_string()
            }
            "mpris" => {
                let svc = ctx
                    .config
                    .mpris_service
                    .clone()
                    .unwrap_or_else(|| "org.mpris.MediaPlayer2.vlc".to_string());
                let p = MprisPlayer::new(ctx.clone(), svc);
                players.insert("mpris".to_string(), Box::new(p));
                "mpris".to_string()
            }
            other => {
                tracing::warn!("Unknown PLAYER {other}, defaulting to jriver (Python parity)");
                let p = JRiverPlayer::new(ctx.clone());
                players.insert("jriver".to_string(), Box::new(p));
                "jriver".to_string()
            }
        };

        Self {
            players,
            active_player: None,
            default_player_id,
        }
    }

    /// Select the active player by id.
    ///
    /// # Errors
    /// Returns [`PlayerError::NotFound`] if no backend is registered for `id`.
    pub fn set_active(&mut self, id: &str) -> Result<()> {
        if self.players.contains_key(id) {
            self.active_player = Some(id.to_string());
            Ok(())
        } else {
            Err(PlayerError::NotFound(format!(
                "Player {id} not configured (check PLAYER in ~/.config/tuxtalks-oxide/config.json)"
            )))
        }
    }

    /// Returns the currently selected player (or the default from config).
    ///
    /// # Errors
    /// Returns [`PlayerError::NotFound`] if no backend is registered for the
    /// active / default player id.
    pub fn get_active(&self) -> Result<&dyn MediaPlayer> {
        let id = self
            .active_player
            .as_deref()
            .unwrap_or(self.default_player_id.as_str());
        self.players
            .get(id)
            .map(std::convert::AsRef::as_ref)
            .ok_or_else(|| {
                PlayerError::NotFound(format!(
                    "No backend registered for active player '{id}' (empty PLAYER setup?)"
                ))
            })
    }

    pub async fn health_check_all(&self) -> HashMap<String, bool> {
        let mut results = HashMap::new();
        for (id, player) in &self.players {
            results.insert(id.clone(), player.health_check().await);
        }
        results
    }
}
