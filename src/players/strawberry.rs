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

pub struct StrawberryPlayer {
    mpris: MprisPlayer,
    db: DbConnection,
    _ctx: Arc<PlayerContext>,
}

impl StrawberryPlayer {
    #[must_use]
    pub fn new(ctx: Arc<PlayerContext>) -> Self {
        let db_path = ctx.config.strawberry_db_path.clone();
        Self {
            mpris: MprisPlayer::new(ctx.clone(), "org.mpris.MediaPlayer2.strawberry".to_string()),
            db: DbConnection::new(&db_path),
            _ctx: ctx,
        }
    }

    async fn play_files(&self, files: Vec<String>) -> Result<()> {
        if files.is_empty() {
            return Err(PlayerError::NotFound("No files found to play".to_string()));
        }

        tracing::info!("Strawberry: Loading {} files", files.len());

        // Strawberry supports loading files via CLI
        let mut cmd = Command::new("strawberry");
        cmd.arg("--load");
        for f in files {
            cmd.arg(f);
        }

        cmd.spawn()
            .map_err(|e| PlayerError::Internal(format!("Failed to launch strawberry: {e}")))?;

        // Give it a moment to load then ensure it's playing via MPRIS
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        self.mpris.play_pause().await?;
        self.mpris.what_is_playing().await?;
        Ok(())
    }
}

#[async_trait]
impl MediaPlayer for StrawberryPlayer {
    fn id(&self) -> &'static str {
        "strawberry"
    }

    async fn health_check(&self) -> bool {
        if self.mpris.health_check().await {
            return true;
        }

        tracing::info!("Strawberry: Not running, attempting to launch...");
        if let Ok(_child) = Command::new("strawberry").spawn() {
            for _ in 0..10 {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                if self.mpris.health_check().await {
                    tracing::info!("Strawberry: Successfully launched.");
                    return true;
                }
            }
        }
        tracing::error!("Strawberry: Failed to launch.");
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
        // Query the DB for the current album's tracks
        if let Some(metadata) = self.mpris.get_metadata().await? {
            if let Some(album) = metadata.get("xesam:album") {
                let album_str = album.to_string();
                let sql = "SELECT title, url FROM songs WHERE album = ? ORDER BY track, disc";
                let tracks = self.db.query_list(sql, [album_str], |row| {
                    let title: String = row.get(0)?;
                    let url: String = row.get(1)?;
                    Ok((Track(title), url))
                })?;
                return Ok(tracks);
            }
        }
        Ok(vec![])
    }

    async fn play_genre(&self, genre: &Genre) -> Result<()> {
        let sql = "SELECT url FROM songs WHERE genre LIKE ? ORDER BY random() LIMIT 100";
        let tracks = self.db.query_list(sql, [format!("%{}%", genre.0)], |row| {
            row.get::<_, String>(0)
        })?;
        self.play_files(tracks).await
    }

    async fn play_artist(&self, artist: &Artist) -> Result<()> {
        let sql = "SELECT url FROM songs WHERE artist LIKE ? ORDER BY album, track, disc";
        let tracks = self
            .db
            .query_list(sql, [format!("%{}%", artist.0)], |row| {
                row.get::<_, String>(0)
            })?;
        self.play_files(tracks).await
    }

    async fn play_album(&self, album: &Album) -> Result<()> {
        let sql = "SELECT url FROM songs WHERE album LIKE ? ORDER BY track, disc";
        let tracks = self.db.query_list(sql, [format!("%{}%", album.0)], |row| {
            row.get::<_, String>(0)
        })?;
        self.play_files(tracks).await
    }

    async fn play_playlist(&self, name: &str) -> Result<()> {
        let sql = "SELECT rowid, name FROM playlists WHERE name LIKE ?";
        let results = self.db.query_list(sql, [format!("%{name}%")], |row| {
            let id: i64 = row.get(0)?;
            let name: String = row.get(1)?;
            Ok((id, name))
        })?;

        if let Some((id, pname)) = results.first() {
            tracing::info!("Strawberry: Playing playlist {}", pname);
            let track_sql = "SELECT url FROM playlist_items WHERE playlist_id = ? ORDER BY rowid";
            let tracks = self
                .db
                .query_list(track_sql, [id], |row| row.get::<_, String>(0))?;
            self.play_files(tracks).await
        } else {
            Err(PlayerError::NotFound(format!("Playlist {name} not found")))
        }
    }

    async fn play_random(&self) -> Result<()> {
        let sql = "SELECT url FROM songs ORDER BY random() LIMIT 100";
        let tracks = self.db.query_list(sql, [], |row| row.get::<_, String>(0))?;
        self.play_files(tracks).await
    }

    async fn play_any(&self, query: &str) -> Result<SearchResult> {
        let query_norm = crate::utils::fuzzy::normalize_text(query);
        let mut candidates = Vec::new();

        // 1. Artist Matches
        let artist_sql = "SELECT DISTINCT artist FROM songs WHERE artist LIKE ? ORDER BY artist";
        let artists = self
            .db
            .query_list(artist_sql, [format!("%{query_norm}%")], |row| {
                row.get::<_, String>(0)
            })?;
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
        let album_sql = "SELECT DISTINCT album FROM songs WHERE album LIKE ? ORDER BY album";
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
        let track_sql = "SELECT title, url FROM songs WHERE title LIKE ? LIMIT 50";
        let tracks = self
            .db
            .query_list(track_sql, [format!("%{query_norm}%")], |row| {
                let title: String = row.get(0)?;
                let url: String = row.get(1)?;
                Ok((title, url))
            })?;
        for (title, url) in tracks {
            let score = crate::utils::fuzzy::find_matches(&query_norm, &[&title], 1, 0.6)
                .first()
                .map_or(0.6, |m| m.score);
            candidates.push((title, url, "track", score * 0.95));
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
        let sql =
            "SELECT DISTINCT album FROM songs WHERE artist LIKE ? OR album LIKE ? ORDER BY album";
        let albums = self.db.query_list(
            sql,
            [format!("%{}%", artist.0), format!("%{}%", artist.0)],
            |row| row.get::<_, String>(0),
        )?;
        Ok(albums.into_iter().map(Album).collect())
    }
}
