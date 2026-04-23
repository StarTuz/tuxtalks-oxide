use crate::config::PlayerContext;
use crate::{
    Album, Artist, Genre, MediaPlayer, NowPlaying, PlayerError, Result, SearchResult,
    SelectionItem, Track,
};
use async_trait::async_trait;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use zbus::zvariant::OwnedValue;
use zbus::{proxy, Connection};

#[proxy(
    interface = "org.mpris.MediaPlayer2.Player",
    default_service = "org.mpris.MediaPlayer2.vlc",
    default_path = "/org/mpris/MediaPlayer2",
    gen_blocking = false
)]
pub trait MprisInterface {
    fn play_pause(&self) -> zbus::Result<()>;
    fn stop(&self) -> zbus::Result<()>;
    fn next(&self) -> zbus::Result<()>;
    fn previous(&self) -> zbus::Result<()>;
    fn open_uri(&self, uri: &str) -> zbus::Result<()>;

    #[zbus(property)]
    fn metadata(&self) -> zbus::Result<std::collections::HashMap<String, OwnedValue>>;

    #[zbus(property)]
    fn playback_status(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn identity(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn volume(&self) -> zbus::Result<f64>;
    #[zbus(property, name = "Volume")]
    fn set_volume(&self, value: f64) -> zbus::Result<()>;
}

/// Trait for mocking the MPRIS proxy.
#[async_trait]
pub trait MprisProxyTrait: Send + Sync {
    async fn play_pause(&self) -> Result<()>;
    async fn stop(&self) -> Result<()>;
    async fn next(&self) -> Result<()>;
    async fn previous(&self) -> Result<()>;
    async fn open_uri(&self, uri: &str) -> Result<()>;
    async fn metadata(&self) -> Result<std::collections::HashMap<String, OwnedValue>>;
    async fn playback_status(&self) -> Result<String>;
    async fn identity(&self) -> Result<String>;
    async fn volume(&self) -> Result<f64>;
    async fn set_volume(&self, value: f64) -> Result<()>;
}

#[async_trait]
impl MprisProxyTrait for MprisInterfaceProxy<'_> {
    async fn play_pause(&self) -> Result<()> {
        self.play_pause()
            .await
            .map_err(|e| PlayerError::Communication(e.to_string()))
    }
    async fn stop(&self) -> Result<()> {
        self.stop()
            .await
            .map_err(|e| PlayerError::Communication(e.to_string()))
    }
    async fn next(&self) -> Result<()> {
        self.next()
            .await
            .map_err(|e| PlayerError::Communication(e.to_string()))
    }
    async fn previous(&self) -> Result<()> {
        self.previous()
            .await
            .map_err(|e| PlayerError::Communication(e.to_string()))
    }
    async fn open_uri(&self, uri: &str) -> Result<()> {
        self.open_uri(uri)
            .await
            .map_err(|e| PlayerError::Communication(e.to_string()))
    }
    async fn metadata(&self) -> Result<std::collections::HashMap<String, OwnedValue>> {
        self.metadata()
            .await
            .map_err(|e| PlayerError::Communication(e.to_string()))
    }
    async fn playback_status(&self) -> Result<String> {
        self.playback_status()
            .await
            .map_err(|e| PlayerError::Communication(e.to_string()))
    }
    async fn identity(&self) -> Result<String> {
        self.identity()
            .await
            .map_err(|e| PlayerError::Communication(e.to_string()))
    }
    async fn volume(&self) -> Result<f64> {
        self.volume()
            .await
            .map_err(|e| PlayerError::Communication(e.to_string()))
    }
    async fn set_volume(&self, value: f64) -> Result<()> {
        self.set_volume(value)
            .await
            .map_err(|e| PlayerError::Communication(e.to_string()))
    }
}

pub struct MprisPlayer {
    _ctx: Arc<PlayerContext>,
    service_name: String,
}

impl MprisPlayer {
    #[must_use]
    pub fn new(ctx: Arc<PlayerContext>, service_name: String) -> Self {
        Self {
            _ctx: ctx,
            service_name,
        }
    }

    async fn get_proxy(&self) -> Result<Box<dyn MprisProxyTrait>> {
        let conn = Connection::session()
            .await
            .map_err(|e| PlayerError::Communication(e.to_string()))?;

        // 1. Try the configured service name first if it's active
        if let Ok(proxy) = MprisInterfaceProxy::builder(&conn)
            .destination(self.service_name.clone())?
            .build()
            .await
        {
            if let Ok(status) = proxy.playback_status().await {
                if status == "Playing" {
                    return Ok(Box::new(proxy));
                }
            }
        }

        // 2. Discover all players and pick the best candidate
        let players = self.discover_players(&conn).await?;
        let mut candidates = Vec::new();

        for player in players {
            if let Ok(proxy) = MprisInterfaceProxy::builder(&conn)
                .destination(player.clone())?
                .build()
                .await
            {
                if let Ok(status) = proxy.playback_status().await {
                    if status == "Playing" {
                        return Ok(Box::new(proxy)); // Instant win
                    }
                    candidates.push(proxy);
                }
            }
        }

        // 3. Fallback to any detected player (likely Paused or Stopped)
        if let Some(first) = candidates.into_iter().next() {
            return Ok(Box::new(first));
        }

        Err(PlayerError::Communication(
            "No active MPRIS player found".to_string(),
        ))
    }

    async fn discover_players(&self, conn: &Connection) -> Result<Vec<String>> {
        let reply: Vec<String> = conn
            .call_method(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus"),
                "ListNames",
                &(),
            )
            .await
            .map_err(|e| PlayerError::Communication(e.to_string()))?
            .body()
            .deserialize()
            .map_err(|e| PlayerError::Communication(e.to_string()))?;

        Ok(reply
            .into_iter()
            .filter(|name| name.starts_with("org.mpris.MediaPlayer2."))
            .collect())
    }

    async fn play_files(&self, files: Vec<PathBuf>) -> Result<()> {
        if files.is_empty() {
            return Ok(());
        }

        let proxy = self.get_proxy().await?;

        if files.len() == 1 {
            let uri = format!("file://{}", files[0].to_string_lossy());
            return proxy.open_uri(&uri).await;
        }

        // Multiple files: Generate M3U8
        let m3u_path = std::env::temp_dir().join(format!("tuxtalks_{}.m3u8", self.service_name));
        let mut f = std::fs::File::create(&m3u_path)
            .map_err(|e| PlayerError::Internal(format!("Failed to create playlist: {e}")))?;

        writeln!(f, "#EXTM3U").ok();
        for file in files {
            writeln!(f, "{}", file.to_string_lossy()).ok();
        }

        let uri = format!("file://{}", m3u_path.to_string_lossy());
        proxy.open_uri(&uri).await
    }

    /// Fetch the raw MPRIS `Metadata` dictionary from the active player.
    ///
    /// # Errors
    /// Returns an error if the D-Bus proxy cannot be built or the property
    /// fetch fails.
    pub async fn get_metadata(
        &self,
    ) -> Result<Option<std::collections::HashMap<String, OwnedValue>>> {
        let proxy = self.get_proxy().await?;
        Ok(Some(proxy.metadata().await?))
    }

    async fn fetch_now_playing(&self) -> Result<NowPlaying> {
        let proxy = self.get_proxy().await?;
        let mut metadata = proxy.metadata().await?;

        let title = metadata
            .remove("xesam:title")
            .and_then(|v| String::try_from(v).ok())
            .unwrap_or_else(|| "Unknown Title".to_string());

        let artist = metadata
            .remove("xesam:artist")
            .and_then(|v| {
                let val: zbus::zvariant::Value = v.into();
                match val {
                    zbus::zvariant::Value::Str(s) => Some(s.to_string()),
                    zbus::zvariant::Value::Array(arr) => {
                        for item in arr.iter() {
                            if let Ok(s) = String::try_from(item) {
                                return Some(s);
                            }
                        }
                        None
                    }
                    _ => None,
                }
            })
            .unwrap_or_else(|| "Unknown Artist".to_string());

        let album = metadata
            .remove("xesam:album")
            .and_then(|v| String::try_from(v).ok())
            .unwrap_or_else(|| "Unknown Album".to_string());

        let player_name = proxy
            .identity()
            .await
            .unwrap_or_else(|_| "Unknown Player".to_string());

        let summary = if title == "Unknown Title" && artist == "Unknown Artist" {
            format!("Nothing playing on {player_name}")
        } else {
            format!("{title} by {artist} ({album}) via {player_name}")
        };

        Ok(NowPlaying {
            title,
            artist,
            album,
            player: self.id().to_string(),
            summary,
        })
    }
}

#[async_trait]
impl MediaPlayer for MprisPlayer {
    fn id(&self) -> &'static str {
        "mpris"
    }

    async fn health_check(&self) -> bool {
        match Connection::session().await {
            Ok(conn) => {
                // Check if the name has an owner
                match conn
                    .call_method(
                        Some("org.freedesktop.DBus"),
                        "/org/freedesktop/DBus",
                        Some("org.freedesktop.DBus"),
                        "NameHasOwner",
                        &(&self.service_name,),
                    )
                    .await
                {
                    Ok(reply) => reply.body().deserialize::<bool>().unwrap_or(false),
                    Err(_) => false,
                }
            }
            Err(_) => false,
        }
    }

    async fn play(&self) -> Result<()> {
        let proxy = self.get_proxy().await?;
        // MPRIS doesn't have a simple 'play' that isn't 'play_pause' sometimes,
        // but we can use the 'Play' method if it exists.
        // For now, PlayPause is fine as a fallback or if we use the specific 'Play' member.
        proxy.play_pause().await
    }

    async fn pause(&self) -> Result<()> {
        self.get_proxy().await?.play_pause().await
    }

    async fn play_pause(&self) -> Result<()> {
        self.get_proxy().await?.play_pause().await
    }

    async fn stop(&self) -> Result<()> {
        self.get_proxy().await?.stop().await
    }

    async fn next_track(&self) -> Result<()> {
        self.get_proxy().await?.next().await?;
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let _ = self.what_is_playing().await;
        Ok(())
    }

    async fn previous_track(&self) -> Result<()> {
        self.get_proxy().await?.previous().await?;
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let _ = self.what_is_playing().await;
        Ok(())
    }

    async fn volume_up(&self) -> Result<()> {
        let proxy = self.get_proxy().await?;
        let current = proxy.volume().await.unwrap_or(0.5f64);
        proxy.set_volume((current + 0.1).min(1.0)).await
    }

    async fn volume_down(&self) -> Result<()> {
        let proxy = self.get_proxy().await?;
        let current = proxy.volume().await.unwrap_or(0.5f64);
        proxy.set_volume((current - 0.1).max(0.0)).await
    }

    async fn now_playing(&self) -> Result<NowPlaying> {
        self.fetch_now_playing().await
    }

    async fn what_is_playing(&self) -> Result<String> {
        Ok(self.fetch_now_playing().await?.summary)
    }

    async fn list_tracks(&self) -> Result<Vec<(Track, String)>> {
        if let Some(lib) = &self._ctx.library {
            if let Ok(proxy) = self.get_proxy().await {
                if let Ok(mut metadata) = proxy.metadata().await {
                    if let Some(album_str) = metadata
                        .remove("xesam:album")
                        .and_then(|v| String::try_from(v).ok())
                    {
                        if !album_str.is_empty() {
                            let paths = lib.get_album_tracks(&album_str)?;
                            let tracks: Vec<(Track, String)> = paths
                                .into_iter()
                                .map(|p| {
                                    let path_str = p.to_string_lossy().to_string();
                                    let filename = p.file_name().map_or_else(
                                        || path_str.clone(),
                                        |f| f.to_string_lossy().to_string(),
                                    );
                                    (Track(filename), path_str)
                                })
                                .collect();
                            return Ok(tracks);
                        }
                    }
                }
            }
        }
        Ok(vec![])
    }

    async fn play_playlist(&self, name: &str) -> Result<()> {
        if let Some(lib) = &self._ctx.library {
            let matches = lib.search_playlists(name)?;
            if let Some((_, path)) = matches.first() {
                let paths = lib.get_playlist_tracks(std::path::Path::new(path))?;
                let path_bufs = paths.into_iter().map(PathBuf::from).collect();
                return self.play_files(path_bufs).await;
            }
            return Err(PlayerError::NotFound(format!("Playlist {name} not found")));
        }
        Err(PlayerError::Internal(
            "Local library not initialized".to_string(),
        ))
    }

    async fn play_genre(&self, genre: &Genre) -> Result<()> {
        if let Some(lib) = &self._ctx.library {
            let tracks = lib.search_tracks(&genre.0)?;
            let paths = tracks.into_iter().map(|t| t.path).collect();
            return self.play_files(paths).await;
        }
        Err(PlayerError::Internal(
            "Local library not initialized".to_string(),
        ))
    }

    async fn play_artist(&self, artist: &Artist) -> Result<()> {
        if let Some(lib) = &self._ctx.library {
            let albums = lib.get_artist_albums(&artist.0)?;
            let mut all_paths: Vec<PathBuf> = Vec::new();
            for alb in albums {
                let paths = lib.get_album_tracks(&alb.0)?;
                all_paths.extend(paths);
            }
            return self.play_files(all_paths).await;
        }
        Err(PlayerError::Internal(
            "Local library not initialized".to_string(),
        ))
    }

    async fn play_album(&self, album: &Album) -> Result<()> {
        if let Some(lib) = &self._ctx.library {
            let paths = lib.get_album_tracks(&album.0)?;
            return self.play_files(paths).await;
        }
        Err(PlayerError::Internal(
            "Local library not initialized".to_string(),
        ))
    }

    async fn play_random(&self) -> Result<()> {
        if let Some(lib) = &self._ctx.library {
            let tracks = lib.get_random_tracks(50)?;
            let paths = tracks.into_iter().map(|t| t.path).collect();
            return self.play_files(paths).await;
        }
        Err(PlayerError::Internal(
            "Local library not initialized".to_string(),
        ))
    }

    async fn play_any(&self, query: &str) -> Result<SearchResult> {
        if let Some(lib) = &self._ctx.library {
            let query_norm = crate::utils::fuzzy::normalize_text(query);
            let mut candidates = Vec::new();

            // 1. Artist Matches
            let albums_by_artist = lib.get_artist_albums(&query_norm)?;
            for alb in albums_by_artist {
                // get_artist_albums already scores, but we'll treat them as high confidence
                candidates.push(SelectionItem {
                    label: format!("Artist search result -> Album: {}", alb.0),
                    value: alb.0,
                    item_type: "album".to_string(),
                });
            }

            // 2. Track Matches
            let tracks = lib.search_tracks(&query_norm)?;
            for t in tracks {
                candidates.push(SelectionItem {
                    label: format!("Track: {} by {}", t.title, t.artist),
                    value: t.path.to_string_lossy().to_string(),
                    item_type: "track".to_string(),
                });
            }

            if candidates.is_empty() {
                return Ok(SearchResult::Error(format!("Nothing found for {query}")));
            }

            if candidates.len() == 1 {
                let item = candidates.remove(0);
                match item.item_type.as_str() {
                    "album" => self.play_album(&Album(item.value)).await?,
                    "track" => self.play_files(vec![PathBuf::from(item.value)]).await?,
                    _ => {}
                }
                return Ok(SearchResult::Done(item.label));
            }

            return Ok(SearchResult::SelectionRequired(candidates));
        }
        Err(PlayerError::Internal(
            "Local library not initialized".to_string(),
        ))
    }

    async fn get_artist_albums(&self, artist: &Artist) -> Result<Vec<Album>> {
        if let Some(lib) = &self._ctx.library {
            return lib.get_artist_albums(&artist.0);
        }
        Ok(vec![])
    }
}
