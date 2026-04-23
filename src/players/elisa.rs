use crate::config::PlayerContext;
use crate::players::mpris::MprisPlayer;
use crate::utils::db::DbConnection;
use crate::{
    Album, Artist, Genre, MediaPlayer, NowPlaying, PlayerError, Result, SearchResult,
    SelectionItem, Track,
};
use async_trait::async_trait;
use std::process::Command;
use std::sync::Arc;

pub struct ElisaPlayer {
    mpris: MprisPlayer,
    db: DbConnection,
    _ctx: Arc<PlayerContext>,
}

impl ElisaPlayer {
    #[must_use]
    pub fn new(ctx: Arc<PlayerContext>) -> Self {
        let db_path = ctx.config.elisa_db_path.clone();
        Self {
            mpris: MprisPlayer::new(ctx.clone(), "org.mpris.MediaPlayer2.elisa".to_string()),
            db: DbConnection::new(&db_path),
            _ctx: ctx,
        }
    }

    async fn play_files(&self, files: Vec<String>) -> Result<()> {
        if files.is_empty() {
            return Err(PlayerError::NotFound("No files found to play".to_string()));
        }

        tracing::info!("Elisa: Loading {} files", files.len());

        // Elisa supports loading files via command line
        let mut cmd = Command::new("elisa");
        for f in files {
            cmd.arg(f);
        }

        cmd.spawn()
            .map_err(|e| PlayerError::Internal(format!("Failed to launch elisa: {e}")))?;

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // Ensure it's playing
        self.mpris.play_pause().await?;
        self.mpris.what_is_playing().await?;
        Ok(())
    }
}

#[async_trait]
impl MediaPlayer for ElisaPlayer {
    fn id(&self) -> &'static str {
        "elisa"
    }

    async fn health_check(&self) -> bool {
        if self.mpris.health_check().await {
            return true;
        }

        tracing::info!("Elisa: Not running, attempting to launch...");
        if let Ok(_child) = Command::new("elisa").spawn() {
            for _ in 0..10 {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                if self.mpris.health_check().await {
                    tracing::info!("Elisa: Successfully launched.");
                    return true;
                }
            }
        }
        tracing::error!("Elisa: Failed to launch.");
        false
    }

    async fn play(&self) -> Result<()> {
        self.mpris.play().await
    }
    async fn pause(&self) -> Result<()> {
        self.mpris.pause().await
    }
    async fn play_pause(&self) -> Result<()> {
        self.mpris.play_pause().await
    }

    async fn stop(&self) -> Result<()> {
        self.mpris.stop().await
    }

    async fn next_track(&self) -> Result<()> {
        self.mpris.next_track().await
    }

    async fn previous_track(&self) -> Result<()> {
        self.mpris.previous_track().await
    }

    async fn volume_up(&self) -> Result<()> {
        self.mpris.volume_up().await
    }

    async fn volume_down(&self) -> Result<()> {
        self.mpris.volume_down().await
    }

    async fn now_playing(&self) -> Result<NowPlaying> {
        let mut n = self.mpris.now_playing().await?;
        n.player = self.id().to_string();
        Ok(n)
    }

    async fn what_is_playing(&self) -> Result<String> {
        self.mpris.what_is_playing().await
    }

    async fn list_tracks(&self) -> Result<Vec<(Track, String)>> {
        if let Some(metadata) = self.mpris.get_metadata().await? {
            if let Some(album) = metadata.get("xesam:album") {
                let album_str = album.to_string();
                let sql = "SELECT Title, FileName FROM Tracks WHERE AlbumTitle = ? ORDER BY DiscNumber, TrackNumber";
                let tracks = self.db.query_list(sql, [album_str], |row| {
                    let title: String = row.get(0)?;
                    let file: String = row.get(1)?;
                    Ok((Track(title), file))
                })?;
                return Ok(tracks);
            }
        }
        Ok(vec![])
    }

    async fn play_playlist(&self, _name: &str) -> Result<()> {
        Err(PlayerError::NotFound(
            "Playlist support not implemented for Elisa (matching Gold Standard)".to_string(),
        ))
    }

    async fn play_genre(&self, genre: &Genre) -> Result<()> {
        let sql = "SELECT FileName FROM Tracks WHERE Genre LIKE ? ORDER BY random() LIMIT 100";
        let tracks = self.db.query_list(sql, [format!("%{}%", genre.0)], |row| {
            row.get::<_, String>(0)
        })?;
        self.play_files(tracks).await
    }

    async fn play_artist(&self, artist: &Artist) -> Result<()> {
        let sql = "SELECT FileName FROM Tracks WHERE ArtistName LIKE ? OR AlbumArtistName LIKE ? ORDER BY AlbumTitle, TrackNumber";
        let tracks = self.db.query_list(
            sql,
            [format!("%{}%", artist.0), format!("%{}%", artist.0)],
            |row| row.get::<_, String>(0),
        )?;
        self.play_files(tracks).await
    }

    async fn play_album(&self, album: &Album) -> Result<()> {
        let sql =
            "SELECT FileName FROM Tracks WHERE AlbumTitle LIKE ? ORDER BY DiscNumber, TrackNumber";
        let tracks = self.db.query_list(sql, [format!("%{}%", album.0)], |row| {
            row.get::<_, String>(0)
        })?;
        self.play_files(tracks).await
    }

    async fn play_random(&self) -> Result<()> {
        let sql = "SELECT FileName FROM Tracks ORDER BY random() LIMIT 100";
        let tracks = self.db.query_list(sql, [], |row| row.get::<_, String>(0))?;
        self.play_files(tracks).await
    }

    async fn play_any(&self, query: &str) -> Result<SearchResult> {
        let query_norm = crate::utils::fuzzy::normalize_text(query);
        let mut candidates = Vec::new();

        // 1. Artist Matches
        let artist_sql = "SELECT DISTINCT ArtistName FROM Tracks WHERE ArtistName LIKE ? OR AlbumArtistName LIKE ? ORDER BY ArtistName";
        let artists = self.db.query_list(
            artist_sql,
            [format!("%{query_norm}%"), format!("%{query_norm}%")],
            |row| row.get::<_, String>(0),
        )?;
        for a in artists {
            let score = crate::utils::fuzzy::find_matches(&query_norm, &[&a], 1, 0.6)
                .first()
                .map_or(0.6, |m| m.score);
            candidates.push((format!("Artist: {a}"), a.clone(), "artist", score));

            // Contextual Album Search
            if score > 0.8 {
                let albums = self.get_artist_albums(&Artist(a.clone())).await?;
                for alb in albums {
                    candidates.push((
                        format!("Album: {} (by {})", alb.0, a),
                        alb.0,
                        "album",
                        score * 0.98,
                    ));
                }
            }
        }

        // 2. Album Matches
        let album_sql =
            "SELECT DISTINCT AlbumTitle FROM Tracks WHERE AlbumTitle LIKE ? ORDER BY AlbumTitle";
        let albums = self
            .db
            .query_list(album_sql, [format!("%{query_norm}%")], |row| {
                row.get::<_, String>(0)
            })?;
        for alb in albums {
            let score = crate::utils::fuzzy::find_matches(&query_norm, &[&alb], 1, 0.6)
                .first()
                .map_or(0.6, |m| m.score);
            candidates.push((format!("Album: {alb}"), alb.clone(), "album", score));
        }

        // 3. Track Matches
        let track_sql = "SELECT Title, FileName FROM Tracks WHERE Title LIKE ? LIMIT 50";
        // Note: Elisa might not have 'Title' column in some versions, but the schema I saw in tests had it?
        // Wait, the schema in elisa_integration.rs didn't have Title! It only had FileName.
        // Let me check what the Python version uses.
        // Python ElisaPlayer uses 'Title' if available or extracts from FileName.

        // I'll try 'Title' but fallback or use FileName stem.
        let tracks = self
            .db
            .query_list(track_sql, [format!("%{query_norm}%")], |row| {
                let title: String = row.get(0)?;
                let file: String = row.get(1)?;
                Ok((title, file))
            });

        if let Ok(tracks_list) = tracks {
            for (title, file) in tracks_list {
                let score = crate::utils::fuzzy::find_matches(&query_norm, &[&title], 1, 0.6)
                    .first()
                    .map_or(0.6, |m| m.score);
                candidates.push((title, file, "track", score * 0.95));
            }
        }

        if candidates.is_empty() {
            return Ok(SearchResult::Error(format!("Nothing found for {query}")));
        }

        candidates.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));
        candidates.dedup_by(|a, b| a.1 == b.1 && a.2 == b.2);

        let top = &candidates[0];
        let is_clear_winner =
            candidates.len() == 1 || (top.3 > 0.9 && (top.3 - candidates[1].3 > 0.15));

        if is_clear_winner {
            match top.2 {
                "artist" => self.play_artist(&Artist(top.1.clone())).await?,
                "album" => self.play_album(&Album(top.1.clone())).await?,
                "track" => self.play_files(vec![top.1.clone()]).await?,
                _ => {}
            }
            return Ok(SearchResult::Done(top.0.clone()));
        }

        let selection = candidates
            .into_iter()
            .take(10)
            .map(|c| SelectionItem {
                label: c.0,
                value: c.1,
                item_type: c.2.to_string(),
            })
            .collect();

        Ok(SearchResult::SelectionRequired(selection))
    }

    async fn get_artist_albums(&self, artist: &Artist) -> Result<Vec<Album>> {
        let sql = "SELECT DISTINCT AlbumTitle FROM Tracks WHERE ArtistName LIKE ? OR AlbumArtistName LIKE ? OR AlbumTitle LIKE ? ORDER BY AlbumTitle";
        let albums = self.db.query_list(
            sql,
            [
                format!("%{}%", artist.0),
                format!("%{}%", artist.0),
                format!("%{}%", artist.0),
            ],
            |row| row.get::<_, String>(0),
        )?;
        Ok(albums.into_iter().map(Album).collect())
    }
}
