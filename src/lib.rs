//! `TuxTalks` Oxide — Rust port of the media and voice-control layer.
//!
//! # Behavioral source of truth
//!
//! The **Python application** in the repository root (`tuxtalks.py`, `core/`, `players/`, …)
//! defines what the product **does**. This crate should match that behavior. Rust may use
//! different libraries or IPC (e.g. SpeechD-NG on D-Bus) as long as observable behavior
//! aligns with Python, unless a gap is explicitly documented.
//!
//! Configuration uses the same keys as Python’s `config.json` but **Oxide-only paths**
//! (`~/.config/tuxtalks-oxide/`, etc.); see `config` module docs.
//!
//! See `CONTRIBUTING.md` in this directory and `CLAUDE.md` at the repository root.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Domain-specific errors for the media player system.
#[derive(Debug, Error)]
pub enum PlayerError {
    #[error("Communication error: {0}")]
    Communication(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Search matching below threshold: {0}")]
    LowConfidence(String),

    #[error("Resource not found: {0}")]
    NotFound(String),

    #[error("Unexpected error: {0}")]
    Internal(String),

    #[error("Operation timed out")]
    Timeout,

    #[error("DBus error: {0}")]
    Zbus(#[from] zbus::Error),
}

pub type Result<T> = std::result::Result<T, PlayerError>;

/// Newtype wrappers for type safety as defined in the implementation plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Artist(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Album(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Track(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Genre(pub String);

/// Possible states for a query response.
#[derive(Debug, Serialize)]
pub enum SearchResult {
    /// Action completed immediately.
    Done(String),
    /// Multiple matches found, requires user selection.
    SelectionRequired(Vec<SelectionItem>),
    /// Operation failed.
    Error(String),
}

#[derive(Debug, Serialize)]
pub struct SelectionItem {
    pub label: String,
    pub value: String,
    pub item_type: String,
}

/// Structured playback info for CLI / API consumers (no TTS).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// Backend id (`jriver`, `mpris`, `strawberry`, `elisa`).
    pub player: String,
    /// Human-readable one-line summary (legacy `what_is_playing` string shape per backend).
    pub summary: String,
}

#[async_trait]
pub trait MediaPlayer: Send + Sync {
    /// Unique identifier for the player (e.g., "mpris", "jriver").
    fn id(&self) -> &str;

    /// Checks if the player is healthy and reachable.
    async fn health_check(&self) -> bool;

    // --- Basic Controls ---
    async fn play(&self) -> Result<()>;
    async fn pause(&self) -> Result<()>;
    async fn play_pause(&self) -> Result<()>;
    async fn stop(&self) -> Result<()>;
    async fn next_track(&self) -> Result<()>;
    async fn previous_track(&self) -> Result<()>;
    async fn volume_up(&self) -> Result<()>;
    async fn volume_down(&self) -> Result<()>;

    // --- Information ---
    /// Structured now-playing metadata. Must not trigger TTS.
    async fn now_playing(&self) -> Result<NowPlaying>;

    /// Spoken / legacy string status. Backends that announce via TTS do so here (e.g. `JRiver`).
    async fn what_is_playing(&self) -> Result<String>;

    /// Tracks in the current "Playing Now" queue, keyed by (label, backend-specific value).
    /// `JRiver` / MPRIS-backed players may return file paths, MCWS file keys, etc.
    async fn list_tracks(&self) -> Result<Vec<(Track, String)>>;

    /// Tracks in the current "Playing Now" queue with **1-based** position.
    /// Backends without a queue concept return `Vec::new()`.
    async fn now_playing_queue(&self) -> Result<Vec<(Track, usize)>> {
        Ok(Vec::new())
    }

    /// Jump to `position` (1-based) in the current queue. Default: unsupported.
    async fn go_to_track(&self, position: usize) -> Result<()> {
        let _ = position;
        Err(PlayerError::NotFound(
            "go_to_track not supported for this backend".to_string(),
        ))
    }

    /// List albums, optionally filtered to a single artist.
    /// Default: empty list (backends without bulk metadata indexes).
    async fn list_albums(&self, artist: Option<&Artist>) -> Result<Vec<Album>> {
        let _ = artist;
        Ok(Vec::new())
    }

    // --- Library Operations ---
    async fn play_genre(&self, genre: &Genre) -> Result<()>;
    async fn play_artist(&self, artist: &Artist) -> Result<()>;
    async fn play_album(&self, album: &Album) -> Result<()>;
    async fn play_playlist(&self, name: &str) -> Result<()>;
    async fn play_random(&self) -> Result<()>;

    /// Flexible search and play. Returns action status or selection options.
    async fn play_any(&self, query: &str) -> Result<SearchResult>;

    /// Metadata helpers
    async fn get_artist_albums(&self, artist: &Artist) -> Result<Vec<Album>>;
}

pub mod config;
pub mod players;
pub mod utils;

#[cfg(feature = "voice")]
pub mod active_loop;
#[cfg(feature = "voice")]
pub mod integration;
#[cfg(feature = "voice")]
pub mod intelligence;
