use crate::{Album, Result};
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::ItemKey;
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use tracing::{info, warn};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct TrackItem {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub composer: String,
    pub genre: String,
    pub track_number: i32,
    pub media_type: String,
}

pub struct LocalLibrary {
    db_path: PathBuf,
}

impl LocalLibrary {
    pub fn new(db_path: &Path) -> Self {
        let lib = Self {
            db_path: db_path.to_path_buf(),
        };
        if let Err(e) = lib.init_db() {
            warn!("Failed to initialize library DB: {}", e);
        }
        lib
    }

    fn init_db(&self) -> Result<()> {
        let conn = Connection::open(&self.db_path)
            .map_err(|e| crate::PlayerError::Internal(e.to_string()))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS tracks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path BLOB UNIQUE,
                title TEXT,
                artist TEXT,
                album TEXT,
                composer TEXT,
                genre TEXT,
                track_number INTEGER,
                media_type TEXT
            )",
            [],
        )
        .map_err(|e| crate::PlayerError::Internal(e.to_string()))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS playlists (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path BLOB UNIQUE,
                name TEXT
            )",
            [],
        )
        .map_err(|e| crate::PlayerError::Internal(e.to_string()))?;

        Ok(())
    }

    /// Scan a directory tree into the library DB.
    ///
    /// # Errors
    /// Returns an error if the `SQLite` DB at `self.db_path` cannot be opened or
    /// the `DELETE FROM …` / scan inserts fail. Individual bad files are
    /// skipped silently so a single unreadable tag does not abort the scan.
    //
    // Synchronous work in an async signature: kept `async` because the CLI
    // already `.await`s it; switching to sync + `spawn_blocking` is a larger
    // refactor that belongs in its own pass.
    #[allow(clippy::unused_async)]
    pub async fn scan_directory(&self, root_path: &Path, clear_db: bool) -> Result<()> {
        info!("Scanning library at: {}", root_path.display());
        let conn = Connection::open(&self.db_path)
            .map_err(|e| crate::PlayerError::Internal(e.to_string()))?;

        if clear_db {
            conn.execute("DELETE FROM tracks", [])
                .map_err(|e| crate::PlayerError::Internal(e.to_string()))?;
            conn.execute("DELETE FROM playlists", [])
                .map_err(|e| crate::PlayerError::Internal(e.to_string()))?;
        }

        let audio_exts = ["mp3", "flac", "ogg", "m4a", "wav"];
        let video_exts = ["mp4", "mkv", "avi", "mov", "webm", "mpg", "mpeg"];
        let playlist_exts = ["m3u", "m3u8", "pls"];

        let mut count = 0;
        let mut p_count = 0;

        for entry in WalkDir::new(root_path)
            .into_iter()
            .filter_map(std::result::Result::ok)
            .filter(|e| e.file_type().is_file())
        {
            let path = entry.path();
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase();
            let path_bytes = path.as_os_str().as_bytes();

            if playlist_exts.contains(&ext.as_str()) {
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Unknown")
                    .to_string();
                conn.execute(
                    "INSERT OR REPLACE INTO playlists (path, name) VALUES (?, ?)",
                    params![path_bytes, name],
                )
                .ok();
                p_count += 1;
            } else if audio_exts.contains(&ext.as_str()) || video_exts.contains(&ext.as_str()) {
                let media_type = if video_exts.contains(&ext.as_str()) {
                    "video"
                } else {
                    "audio"
                };
                let mut title = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Unknown")
                    .to_string();
                let mut artist = "Unknown Artist".to_string();
                let mut album = "Unknown Album".to_string();
                let mut composer = String::new();
                let mut genre = String::new();
                let mut track_number = 0;

                // Extract metadata
                if let Ok(tagged_file) = Probe::open(path).and_then(lofty::probe::Probe::read) {
                    if let Some(tag) = tagged_file
                        .primary_tag()
                        .or_else(|| tagged_file.first_tag())
                    {
                        if let Some(t) = tag.title() {
                            title = t.to_string();
                        }
                        if let Some(a) = tag.artist() {
                            artist = a.to_string();
                        }
                        if let Some(al) = tag.album() {
                            album = al.to_string();
                        }
                        if let Some(c) = tag.get_string(&ItemKey::Composer) {
                            composer = c.to_string();
                        }
                        if let Some(g) = tag.genre() {
                            genre = g.to_string();
                        }
                        track_number = i32::try_from(tag.track().unwrap_or(0)).unwrap_or(0);
                    }
                }

                conn.execute(
                    "INSERT OR REPLACE INTO tracks (path, title, artist, album, composer, genre, track_number, media_type)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                    params![path_bytes, title, artist, album, composer, genre, track_number, media_type],
                ).ok();
                count += 1;
            }
        }

        info!(
            "Library scan complete. Indexed {} tracks and {} playlists.",
            count, p_count
        );
        Ok(())
    }

    /// Token-search tracks indexed in the DB.
    ///
    /// # Errors
    /// Returns an error if the `SQLite` DB cannot be opened or the prepared
    /// query fails.
    pub fn search_tracks(&self, query: &str) -> Result<Vec<TrackItem>> {
        let tokens: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        if tokens.is_empty() {
            return Ok(vec![]);
        }

        let conn = Connection::open(&self.db_path)
            .map_err(|e| crate::PlayerError::Internal(e.to_string()))?;

        let mut conds = Vec::new();
        let mut params: Vec<String> = Vec::new();

        for token in &tokens {
            conds.push("(title LIKE ? OR artist LIKE ? OR album LIKE ? OR composer LIKE ? OR CAST(path AS TEXT) LIKE ?)".to_string());
            let p = format!("%{token}%");
            for _ in 0..5 {
                params.push(p.clone());
            }
        }

        let where_clause = conds.join(" AND ");
        let sql = format!(
            "SELECT path, title, artist, album, composer, genre, track_number, media_type FROM tracks WHERE {where_clause}"
        );

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| crate::PlayerError::Internal(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(params.into_iter()), |row| {
                let path_bytes: Vec<u8> = row.get(0)?;
                Ok(TrackItem {
                    path: PathBuf::from(std::ffi::OsString::from_vec(path_bytes)),
                    title: row.get(1)?,
                    artist: row.get(2)?,
                    album: row.get(3)?,
                    composer: row.get(4)?,
                    genre: row.get(5)?,
                    track_number: row.get(6)?,
                    media_type: row.get(7)?,
                })
            })
            .map_err(|e| crate::PlayerError::Internal(e.to_string()))?;

        let mut results = Vec::new();
        let re_tokens: Vec<regex::Regex> = tokens
            .iter()
            .filter_map(|t| {
                regex::RegexBuilder::new(&format!(r"\b{}\b", regex::escape(t)))
                    .case_insensitive(true)
                    .build()
                    .ok()
            })
            .collect();

        for track in rows.flatten() {
            let full_text = format!(
                "{} {} {} {} {}",
                track.title,
                track.artist,
                track.album,
                track.composer,
                track.path.display()
            );

            let all_match = tokens.iter().all(|t| {
                let is_whole = re_tokens.iter().any(|re| re.is_match(&full_text));
                is_whole || track.path.to_string_lossy().to_lowercase().contains(t)
            });

            if all_match {
                results.push(track);
            }
        }

        Ok(results)
    }

    /// Return albums matching the given artist query, sorted by match tier.
    ///
    /// # Errors
    /// Returns an error if the underlying [`Self::search_tracks`] call fails.
    pub fn get_artist_albums(&self, query: &str) -> Result<Vec<Album>> {
        let tracks = self.search_tracks(query)?;
        let mut album_scores: HashMap<String, i32> = HashMap::new();
        let query_tokens: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();

        for t in tracks {
            if t.album.is_empty() || t.album == "Unknown Album" {
                continue;
            }

            let mut score = 3;
            if query_tokens
                .iter()
                .all(|tok| t.artist.to_lowercase().contains(tok))
            {
                score = 0;
            } else if query_tokens
                .iter()
                .all(|tok| t.album.to_lowercase().contains(tok))
            {
                score = 1;
            } else if query_tokens
                .iter()
                .all(|tok| t.composer.to_lowercase().contains(tok))
            {
                score = 2;
            }

            let entry = album_scores.entry(t.album.clone()).or_insert(score);
            if score < *entry {
                *entry = score;
            }
        }

        let mut sorted_albums: Vec<String> = album_scores.keys().cloned().collect();
        sorted_albums.sort_by(|a, b| {
            let score_a = album_scores[a];
            let score_b = album_scores[b];

            if score_a == score_b {
                // Secondary: check if album name contains any query token
                let sub_a = i32::from(
                    !query_tokens
                        .iter()
                        .any(|tok| a.to_lowercase().contains(tok)),
                );
                let sub_b = i32::from(
                    !query_tokens
                        .iter()
                        .any(|tok| b.to_lowercase().contains(tok)),
                );

                if sub_a == sub_b {
                    a.cmp(b)
                } else {
                    sub_a.cmp(&sub_b)
                }
            } else {
                score_a.cmp(&score_b)
            }
        });

        Ok(sorted_albums.into_iter().map(Album).collect())
    }

    /// Return file paths of all tracks on the given album, ordered by track
    /// number then title.
    ///
    /// # Errors
    /// Returns an error if the `SQLite` DB cannot be opened or the query fails.
    pub fn get_album_tracks(&self, album: &str) -> Result<Vec<PathBuf>> {
        let conn = Connection::open(&self.db_path)
            .map_err(|e| crate::PlayerError::Internal(e.to_string()))?;
        let mut stmt = conn
            .prepare("SELECT path FROM tracks WHERE album = ? ORDER BY track_number, title")
            .map_err(|e| crate::PlayerError::Internal(e.to_string()))?;

        let rows = stmt
            .query_map([album], |row| {
                let path_bytes: Vec<u8> = row.get(0)?;
                Ok(PathBuf::from(std::ffi::OsString::from_vec(path_bytes)))
            })
            .map_err(|e| crate::PlayerError::Internal(e.to_string()))?;

        let mut results = Vec::new();
        for p in rows.flatten() {
            results.push(p);
        }
        Ok(results)
    }

    /// Search playlists by name (SQL `LIKE %query%`).
    ///
    /// # Errors
    /// Returns an error if the `SQLite` DB cannot be opened or the query fails.
    pub fn search_playlists(&self, query: &str) -> Result<Vec<(String, PathBuf)>> {
        let conn = Connection::open(&self.db_path)
            .map_err(|e| crate::PlayerError::Internal(e.to_string()))?;
        let mut stmt = conn
            .prepare("SELECT name, path FROM playlists WHERE name LIKE ?")
            .map_err(|e| crate::PlayerError::Internal(e.to_string()))?;

        let rows = stmt
            .query_map([format!("%{query}%")], |row| {
                let name: String = row.get(0)?;
                let path_bytes: Vec<u8> = row.get(1)?;
                Ok((
                    name,
                    PathBuf::from(std::ffi::OsString::from_vec(path_bytes)),
                ))
            })
            .map_err(|e| crate::PlayerError::Internal(e.to_string()))?;

        let mut results = Vec::new();
        for p in rows.flatten() {
            results.push(p);
        }
        Ok(results)
    }

    /// Read a playlist file and return the list of track URIs.
    ///
    /// # Errors
    /// Returns an error for unsupported playlist formats or when the file
    /// cannot be read.
    pub fn get_playlist_tracks(&self, playlist_path: &Path) -> Result<Vec<String>> {
        let ext = playlist_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        if ext == "m3u" || ext == "m3u8" {
            Self::parse_m3u(playlist_path)
        } else if ext == "pls" {
            Self::parse_pls(playlist_path)
        } else {
            Err(crate::PlayerError::Internal(format!(
                "Unsupported playlist format: {ext}"
            )))
        }
    }

    fn parse_m3u(path: &Path) -> Result<Vec<String>> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| crate::PlayerError::Internal(e.to_string()))?;
        let mut tracks = Vec::new();
        let parent = path.parent().unwrap_or(Path::new("."));

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let track_path = if Path::new(line).is_absolute() {
                PathBuf::from(line)
            } else {
                parent.join(line)
            };
            if let Some(uri) = track_path.to_str() {
                tracks.push(uri.to_string());
            }
        }
        Ok(tracks)
    }

    fn parse_pls(path: &Path) -> Result<Vec<String>> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| crate::PlayerError::Internal(e.to_string()))?;
        let mut tracks = Vec::new();

        for line in content.lines() {
            let line = line.trim();
            if line.to_lowercase().starts_with("file") && line.contains('=') {
                if let Some(val) = line.split('=').nth(1) {
                    tracks.push(val.trim().to_string());
                }
            }
        }
        Ok(tracks)
    }

    /// Return up to `limit` random tracks from the library.
    ///
    /// # Errors
    /// Returns an error if the `SQLite` DB cannot be opened or the query fails.
    pub fn get_random_tracks(&self, limit: i32) -> Result<Vec<TrackItem>> {
        let conn = Connection::open(&self.db_path)
            .map_err(|e| crate::PlayerError::Internal(e.to_string()))?;
        let mut stmt = conn.prepare("SELECT path, title, artist, album, composer, genre, track_number, media_type FROM tracks ORDER BY RANDOM() LIMIT ?")
            .map_err(|e| crate::PlayerError::Internal(e.to_string()))?;

        let rows = stmt
            .query_map([limit], |row| {
                let path_bytes: Vec<u8> = row.get(0)?;
                Ok(TrackItem {
                    path: PathBuf::from(std::ffi::OsString::from_vec(path_bytes)),
                    title: row.get(1)?,
                    artist: row.get(2)?,
                    album: row.get(3)?,
                    composer: row.get(4)?,
                    genre: row.get(5)?,
                    track_number: row.get(6)?,
                    media_type: row.get(7)?,
                })
            })
            .map_err(|e| crate::PlayerError::Internal(e.to_string()))?;

        let results: Vec<TrackItem> = rows.filter_map(std::result::Result::ok).collect();
        Ok(results)
    }

    /// Build a short human-readable summary of the library (top artists,
    /// albums, and playlists).
    ///
    /// # Errors
    /// Returns an error if any of the underlying queries fail.
    pub fn get_summary(&self) -> Result<String> {
        let conn = Connection::open(&self.db_path)
            .map_err(|e| crate::PlayerError::Internal(e.to_string()))?;

        // Get Top 10 Artists
        let mut stmt = conn
            .prepare("SELECT DISTINCT artist FROM tracks WHERE artist != 'Unknown Artist' LIMIT 10")
            .map_err(|e| crate::PlayerError::Internal(e.to_string()))?;
        let artists: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| crate::PlayerError::Internal(e.to_string()))?
            .filter_map(std::result::Result::ok)
            .collect();

        // Get Top 10 Albums
        let mut stmt = conn
            .prepare("SELECT DISTINCT album FROM tracks WHERE album != 'Unknown Album' LIMIT 10")
            .map_err(|e| crate::PlayerError::Internal(e.to_string()))?;
        let albums: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| crate::PlayerError::Internal(e.to_string()))?
            .filter_map(std::result::Result::ok)
            .collect();

        // Get Playlists
        let mut stmt = conn
            .prepare("SELECT name FROM playlists LIMIT 10")
            .map_err(|e| crate::PlayerError::Internal(e.to_string()))?;
        let playlists: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| crate::PlayerError::Internal(e.to_string()))?
            .filter_map(std::result::Result::ok)
            .collect();

        let mut summary = String::new();
        if !artists.is_empty() {
            summary.push_str("Artists: ");
            summary.push_str(&artists.join(", "));
            summary.push('\n');
        }
        if !albums.is_empty() {
            summary.push_str("Albums: ");
            summary.push_str(&albums.join(", "));
            summary.push('\n');
        }
        if !playlists.is_empty() {
            summary.push_str("Playlists: ");
            summary.push_str(&playlists.join(", "));
            summary.push('\n');
        }

        Ok(summary)
    }
}
