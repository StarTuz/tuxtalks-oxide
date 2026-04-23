use crate::config::PlayerContext;
use crate::utils::circuit_breaker::CircuitBreaker;
use crate::utils::fuzzy::{find_matches, normalize_text};
use crate::{
    Album, Artist, Genre, MediaPlayer, NowPlaying, PlayerError, Result, SearchResult,
    SelectionItem, Track,
};
use async_trait::async_trait;
use quick_xml::de::from_str;
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Write as _;
use std::sync::Arc;
use strsim;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

/// Shape used by `Library/Values`, `Files/Search`, `Playlists/List`, and
/// `Playback/Playlist`: items are containers of named `<Field>` children.
#[derive(Debug, Deserialize)]
struct McwsResponse {
    #[serde(rename = "Item", default)]
    items: Vec<McwsItem>,
}

#[derive(Debug, Deserialize)]
struct McwsItem {
    #[serde(rename = "$value")]
    text: Option<String>,
    #[serde(rename = "Field", default)]
    fields: Vec<McwsField>,
}

#[derive(Debug, Deserialize)]
struct McwsField {
    #[serde(rename = "@Name")]
    name: String,
    #[serde(rename = "$value")]
    value: Option<String>,
}

/// Shape used by `Playback/Info`: flat `<Item Name="Key">value</Item>` entries.
#[derive(Debug, Deserialize)]
struct PlaybackInfoResponse {
    #[serde(rename = "Item", default)]
    items: Vec<PlaybackInfoEntry>,
}

#[derive(Debug, Deserialize)]
struct PlaybackInfoEntry {
    #[serde(rename = "@Name")]
    name: String,
    #[serde(rename = "$value")]
    value: Option<String>,
}

pub struct JRiverPlayer {
    ctx: Arc<PlayerContext>,
    client: Client,
    base_url: String,
    circuit_breaker: CircuitBreaker,
    cache: Mutex<HashMap<String, Vec<String>>>,
}

/// Whether `send_command` / `health_check` are allowed to spawn the `JRiver`
/// binary when a connection fails. Disable with `TUXTALKS_NO_AUTOSTART=1`
/// for CI, headless tests, or scripts that must not spawn GUIs.
fn autostart_enabled() -> bool {
    !matches!(
        std::env::var("TUXTALKS_NO_AUTOSTART")
            .ok()
            .as_deref()
            .map(str::trim),
        Some("1" | "true" | "yes" | "on")
    )
}

/// Builds a human-readable, causally-complete error message for a failing
/// MCWS call. `reqwest::Error`'s `Display` impl only shows the top frame and
/// swallows the real cause ("connection refused", "timed out", …) into
/// `source()`, so we walk the chain and prepend the URL that was targeted.
fn describe_reqwest_error(base_url: &str, e: &reqwest::Error) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut source: Option<&dyn std::error::Error> = Some(e);
    while let Some(err) = source {
        let s = err.to_string();
        if parts.last() != Some(&s) {
            parts.push(s);
        }
        source = err.source();
    }
    let chain = parts.join(": ");
    let hint = if e.is_connect() || chain.contains("Connection refused") {
        " — is JRiver Media Center running with Media Network enabled?"
    } else if e.is_timeout() {
        " — JRiver took too long to respond"
    } else {
        ""
    };
    format!("Could not reach JRiver at {base_url}: {chain}{hint}")
}

impl JRiverPlayer {
    #[must_use]
    pub fn new(ctx: Arc<PlayerContext>) -> Self {
        let base_url = format!(
            "http://{}:{}/MCWS/v1/",
            ctx.config.jriver_ip, ctx.config.jriver_port
        );
        Self::with_base_url(ctx, base_url)
    }

    #[must_use]
    pub fn with_base_url(ctx: Arc<PlayerContext>, base_url: String) -> Self {
        Self {
            ctx,
            client: Client::new(),
            base_url,
            // Allow 3 failures before tripping, 30s reset
            circuit_breaker: CircuitBreaker::new(3, 30),
            cache: Mutex::new(HashMap::new()),
        }
    }

    async fn send_command(&self, path: &str, params: &[(&str, &str)]) -> Result<String> {
        let mut url = format!("{}{}", self.base_url, path);
        if !params.is_empty() {
            let query = serde_urlencoded::to_string(params)
                .map_err(|e| PlayerError::Internal(e.to_string()))?;
            url.push('?');
            url.push_str(&query);
        }

        if url.contains('?') {
            url.push('&');
        } else {
            url.push('?');
        }
        let _ = write!(
            url,
            "Zone=-1&ZoneType=ID&Key={}",
            self.ctx.config.jriver_access_key
        );

        // Retry logic for connection robustness (Parity with Python)
        let mut last_err = None;
        let mut autostart_tried = false;
        for attempt in 1..=3 {
            if self.circuit_breaker.is_open() {
                return Err(PlayerError::Communication(
                    "JRiver circuit breaker is OPEN".to_string(),
                ));
            }

            match self
                .client
                .get(&url)
                .timeout(Duration::from_secs(5))
                .send()
                .await
            {
                Ok(res) => {
                    if res.status().is_success() {
                        let text = res.text().await.map_err(|e| {
                            PlayerError::Communication(describe_reqwest_error(&self.base_url, &e))
                        })?;
                        self.circuit_breaker.record_success();
                        return Ok(text);
                    }
                    self.circuit_breaker.record_failure();
                    last_err = Some(PlayerError::Communication(format!(
                        "JRiver at {} returned HTTP {}",
                        self.base_url,
                        res.status()
                    )));
                }
                Err(e) => {
                    self.circuit_breaker.record_failure();
                    let msg = describe_reqwest_error(&self.base_url, &e);

                    // Python parity: when JRiver is unreachable, try to launch it
                    // and keep retrying once it comes up. See `health_check` in
                    // `players/jriver.py`.
                    if e.is_connect() && !autostart_tried && autostart_enabled() {
                        autostart_tried = true;
                        tracing::info!(
                            "JRiver unreachable at {}; attempting autostart via `{}`…",
                            self.base_url,
                            self.ctx.config.jriver_binary
                        );
                        if self.ensure_running().await {
                            // Skip the 1s cooldown — JRiver just reported Alive.
                            continue;
                        }
                        last_err = Some(PlayerError::Communication(format!(
                            "{msg} [autostart of `{}` failed or timed out]",
                            self.ctx.config.jriver_binary
                        )));
                        break;
                    }
                    last_err = Some(PlayerError::Communication(msg));
                }
            }
            if attempt < 3 {
                sleep(Duration::from_secs(1)).await;
            }
        }

        Err(last_err.unwrap_or_else(|| PlayerError::Communication("Retries exhausted".to_string())))
    }

    /// Quick `GET /Alive` probe. Returns `true` on HTTP 2xx within 2s.
    async fn alive(&self) -> bool {
        match self
            .client
            .get(format!("{}Alive", self.base_url))
            .timeout(Duration::from_secs(2))
            .send()
            .await
        {
            Ok(res) => res.status().is_success(),
            Err(_) => false,
        }
    }

    /// If `JRiver` isn't answering, try to launch the configured binary and
    /// poll `Alive` for up to 20s (500ms interval). Mirrors Python
    /// `players/jriver.py::health_check` launch-and-wait loop.
    ///
    /// Returns `true` if `JRiver` is ready by the end. `false` if the binary
    /// couldn't be spawned (missing from `PATH`) or didn't become ready in time.
    /// Respects `TUXTALKS_NO_AUTOSTART=1` at the caller.
    async fn ensure_running(&self) -> bool {
        if self.alive().await {
            return true;
        }

        let binary = &self.ctx.config.jriver_binary;
        // User-visible progress on stderr (stdout stays clean for --json callers).
        // Mirrors Python `players/jriver.py::health_check` print output.
        eprintln!("⚠️  JRiver not responding. Launching `{binary}`...");

        // Fire-and-forget: drop the Child so init reaps it when JRiver outlives us.
        // JRiver is a long-running GUI app; we intentionally do not wait().
        match std::process::Command::new(binary)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(_child) => tracing::info!("launched `{binary}`; polling Alive…"),
            Err(e) => {
                eprintln!(
                    "❌ Could not launch `{binary}`: {e}. Install JRiver or set \
                     JRIVER_BINARY to the correct executable name."
                );
                return false;
            }
        }

        eprint!("⏳ Waiting for JRiver to start");
        let _ = std::io::stderr().flush();
        // Python parity: 20s deadline, 500ms polling interval.
        for i in 0u32..40 {
            sleep(Duration::from_millis(500)).await;
            if self.alive().await {
                eprintln!(
                    "\n✅ JRiver is ready (took {:.1}s).",
                    f64::from(i + 1) * 0.5
                );
                self.circuit_breaker.record_success();
                return true;
            }
            if i.is_multiple_of(2) {
                eprint!(".");
                let _ = std::io::stderr().flush();
            }
        }
        eprintln!("\n⚠️  JRiver launched but didn't become ready in 20s. Commands may fail.");
        false
    }

    async fn get_library_values(&self, field: &str) -> Result<Vec<String>> {
        {
            let cache = self.cache.lock().await;
            if let Some(vals) = cache.get(field) {
                return Ok(vals.clone());
            }
        }

        let xml = self
            .send_command("Library/Values", &[("Field", field), ("Limit", "10000")])
            .await?;
        let resp: McwsResponse =
            from_str(&xml).map_err(|e| PlayerError::Internal(e.to_string()))?;
        let values: Vec<String> = resp.items.into_iter().filter_map(|i| i.text).collect();

        let mut cache = self.cache.lock().await;
        cache.insert(field.to_string(), values.clone());
        Ok(values)
    }

    /// Fetches `Playback/Info` and returns a `Name` → `value` map. Matches
    /// Python `players/jriver.py::what_is_playing_silent` XML shape exactly.
    async fn playback_info(&self) -> Result<HashMap<String, String>> {
        let xml = self.send_command("Playback/Info", &[]).await?;
        let resp: PlaybackInfoResponse =
            from_str(&xml).map_err(|e| PlayerError::Internal(e.to_string()))?;
        Ok(resp
            .items
            .into_iter()
            .filter_map(|e| e.value.map(|v| (e.name, v)))
            .collect())
    }

    /// Returns `(title, artist, album, summary)` without speaking.
    async fn what_is_playing_silent(&self) -> Result<(String, String, String, String)> {
        let info = self.playback_info().await?;

        let title = info
            .get("Name")
            .cloned()
            .unwrap_or_else(|| "Unknown Title".to_string());
        let artist = info
            .get("Artist")
            .cloned()
            .unwrap_or_else(|| "Unknown Artist".to_string());
        let mut album = info.get("Album").cloned().unwrap_or_default();
        let file_key = info.get("FileKey").cloned();

        let mut track_num = None;
        let mut disc_num = None;

        if let Some(key) = file_key {
            if key != "-1" {
                if let Ok(info_xml) = self.send_command("File/GetInfo", &[("File", &key)]).await {
                    if let Ok(info_resp) = from_str::<McwsResponse>(&info_xml) {
                        for item in info_resp.items {
                            for field in item.fields {
                                match field.name.as_str() {
                                    "Track #" => track_num = field.value,
                                    "Disc #" => disc_num = field.value,
                                    "Album" if album.is_empty() => {
                                        album = field.value.clone().unwrap_or_default();
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        }

        let mut metadata = format!("{title} by {artist}");
        if let Some(t) = track_num {
            let _ = write!(metadata, ", track {t}");
        }
        if let Some(d) = disc_num {
            if d != "1" {
                let _ = write!(metadata, " of disc {d}");
            }
        }

        tracing::info!("Now Playing (Silent): {}", metadata);
        Ok((title, artist, album, metadata))
    }
}

// `play_any` mirrors the multi-tier fuzzy-search flow in Python
// `players/jriver.py::play_any`; breaking it up would fragment the
// tier-by-tier control flow across unrelated helpers.
#[allow(clippy::too_many_lines)]
#[async_trait]
impl MediaPlayer for JRiverPlayer {
    fn id(&self) -> &'static str {
        "jriver"
    }

    async fn health_check(&self) -> bool {
        if self.circuit_breaker.is_open() {
            return false;
        }
        if self.alive().await {
            self.circuit_breaker.record_success();
            return true;
        }
        if !autostart_enabled() {
            self.circuit_breaker.record_failure();
            return false;
        }
        tracing::warn!("JRiver not responding. Attempting to launch...");
        let ok = self.ensure_running().await;
        if !ok {
            self.circuit_breaker.record_failure();
        }
        ok
    }

    async fn play(&self) -> Result<()> {
        self.send_command("Playback/Play", &[]).await.map(|_| ())
    }

    async fn pause(&self) -> Result<()> {
        self.send_command("Playback/Pause", &[]).await.map(|_| ())
    }

    async fn play_pause(&self) -> Result<()> {
        // JRiver MCWS has a PlayPause command
        self.send_command("Playback/PlayPause", &[])
            .await
            .map(|_| ())
    }

    async fn stop(&self) -> Result<()> {
        self.send_command("Playback/Stop", &[]).await?;
        Ok(())
    }

    async fn next_track(&self) -> Result<()> {
        self.send_command("Playback/Next", &[]).await?;
        sleep(Duration::from_millis(500)).await;
        // Silent update to avoid talking over the intro (Python Parity)
        let _ = self.what_is_playing_silent().await;
        Ok(())
    }

    async fn previous_track(&self) -> Result<()> {
        self.send_command("Playback/Previous", &[]).await?;
        sleep(Duration::from_millis(500)).await;
        // Silent update to avoid talking over the intro (Python Parity)
        let _ = self.what_is_playing_silent().await;
        Ok(())
    }

    async fn volume_up(&self) -> Result<()> {
        self.send_command("Playback/Volume", &[("Level", "600")])
            .await?;
        Ok(())
    }

    async fn volume_down(&self) -> Result<()> {
        self.send_command("Playback/Volume", &[("Level", "400")])
            .await?;
        Ok(())
    }

    async fn now_playing(&self) -> Result<NowPlaying> {
        let (title, artist, album, summary) = self.what_is_playing_silent().await?;
        Ok(NowPlaying {
            title,
            artist,
            album,
            player: self.id().to_string(),
            summary,
        })
    }

    async fn what_is_playing(&self) -> Result<String> {
        let np = self.now_playing().await?;
        self.ctx.speaker.announce_now_playing(&np).await;
        Ok(np.summary)
    }

    async fn list_tracks(&self) -> Result<Vec<(Track, String)>> {
        // Implementation for current playing playlist
        Ok(vec![])
    }

    async fn play_genre(&self, genre: &Genre) -> Result<()> {
        let genres = self.get_library_values("Genre").await?;
        let cand: Vec<&str> = genres.iter().map(std::string::String::as_str).collect();
        let matches = find_matches(&genre.0, &cand, 1, 0.6);

        if let Some(m) = matches.first() {
            self.ctx
                .speaker
                .speak(format!("Playing random {} music", m.text))
                .await;
            self.send_command(
                "Playback/PlayDoctor",
                &[
                    ("Seed", &format!("[Genre]=[{}]", m.text)),
                    ("Action", "Play"),
                ],
            )
            .await?;
            Ok(())
        } else {
            self.ctx
                .speaker
                .speak(format!("I couldn't find a genre named {}", genre.0))
                .await;
            Err(PlayerError::NotFound(genre.0.clone()))
        }
    }

    async fn play_artist(&self, artist: &Artist) -> Result<()> {
        self.ctx
            .speaker
            .speak(format!("Playing music by {}", artist.0))
            .await;
        self.send_command(
            "Playback/PlayDoctor",
            &[
                ("Seed", &format!("[Artist]=[{}]", artist.0)),
                ("Action", "Play"),
            ],
        )
        .await?;
        Ok(())
    }

    async fn play_album(&self, album: &Album) -> Result<()> {
        self.ctx
            .speaker
            .speak(format!("Playing album {}", album.0))
            .await;
        self.send_command(
            "Files/Search",
            &[
                ("Query", &format!("[Album]=[{}]", album.0)),
                ("Action", "Play"),
            ],
        )
        .await?;
        Ok(())
    }

    async fn play_playlist(&self, name_query: &str) -> Result<()> {
        self.ctx
            .speaker
            .speak(format!("Searching for playlist {name_query}"))
            .await;

        // 1. Fetch all playlists to find ID (Parity with Python)
        let xml = self.send_command("Playlists/List", &[]).await?;
        let resp: McwsResponse =
            from_str(&xml).map_err(|e| PlayerError::Internal(e.to_string()))?;

        let mut best_match: Option<(String, String)> = None; // (Name, ID)
        let mut best_score = 0.0;

        for item in resp.items {
            let mut p_name = String::new();
            let mut p_id = String::new();

            for field in item.fields {
                if field.name == "Name" {
                    p_name = field.value.unwrap_or_default();
                } else if field.name == "ID" {
                    p_id = field.value.unwrap_or_default();
                }
            }

            if !p_name.is_empty() && !p_id.is_empty() {
                // Exact match check
                if p_name.eq_ignore_ascii_case(name_query) {
                    best_match = Some((p_name, p_id));
                    // best_score = 1.0; // Unused
                    break;
                }

                // Fuzzy match
                let score =
                    strsim::jaro_winkler(&p_name.to_lowercase(), &name_query.to_lowercase());
                if score > best_score && score > 0.6 {
                    best_score = score;
                    best_match = Some((p_name, p_id));
                }
            }
        }

        if let Some((name, id)) = best_match {
            self.ctx
                .speaker
                .speak(format!("Playing playlist {name}"))
                .await;
            self.send_command(
                "Playback/PlayPlaylist",
                &[("Playlist", &id), ("PlaylistType", "ID")],
            )
            .await?;
            Ok(())
        } else {
            self.ctx
                .speaker
                .speak(format!("I couldn't find a playlist named {name_query}"))
                .await;
            Err(PlayerError::NotFound(name_query.to_string()))
        }
    }

    async fn play_random(&self) -> Result<()> {
        // Search for "Random" smartlists (Parity with Python)
        if let Ok(xml) = self.send_command("Playlists/List", &[]).await {
            if let Ok(resp) = from_str::<McwsResponse>(&xml) {
                let playlist_names: Vec<String> = resp
                    .items
                    .into_iter()
                    .flat_map(|i| {
                        i.fields
                            .into_iter()
                            .filter(|f| f.name == "Name")
                            .filter_map(|f| f.value)
                    })
                    .collect();

                let candidates = [
                    "Audio - 100 random songs",
                    "Audio - Random",
                    "Random 100",
                    "Random Songs",
                ];
                for cand in candidates {
                    if playlist_names.contains(&cand.to_string()) {
                        tracing::info!("JRiver: Found random smartlist: {}", cand);
                        self.ctx.speaker.speak("Playing random music").await;
                        self.send_command(
                            "Playback/PlayPlaylist",
                            &[("Playlist", cand), ("PlaylistType", "Name")],
                        )
                        .await?;
                        // Shuffle immediately for smartlists
                        sleep(Duration::from_millis(500)).await;
                        self.send_command("Playback/Shuffle", &[("Mode", "reshuffle")])
                            .await?;
                        return Ok(());
                    }
                }
            }
        }

        tracing::warn!("JRiver: No random smartlist found, falling back to PlayDoctor");
        self.ctx.speaker.speak("Playing random music").await;
        // Python Parity: Radio=0 ensures sequential playback of the doctor list
        self.send_command(
            "Playback/PlayDoctor",
            &[("Seed", ""), ("Action", "Play"), ("Radio", "0")],
        )
        .await?;
        Ok(())
    }

    async fn play_any(&self, query: &str) -> Result<SearchResult> {
        let query_norm = normalize_text(query);

        // 1. Collection Phase
        let mut candidates = Vec::new();

        // -- Artist matches
        let artists = self.get_library_values("Artist").await?;
        let artist_cands: Vec<&str> = artists.iter().map(std::string::String::as_str).collect();
        for m in find_matches(&query_norm, &artist_cands, 5, 0.6) {
            candidates.push((
                format!("Artist: {}", m.text),
                m.text.to_string(),
                "artist",
                m.score,
            ));

            // Contextual Album Search (Parity)
            if m.score > 0.8 {
                let albums_by_artist = self.get_artist_albums(&Artist(m.text.to_string())).await?;
                for alb in albums_by_artist {
                    candidates.push((
                        format!("Album: {} (by {})", alb.0, m.text),
                        alb.0,
                        "album",
                        m.score * 0.98,
                    ));
                }
            }
        }

        // -- Composer matches
        let composers = self.get_library_values("Composer").await?;
        let composer_cands: Vec<&str> = composers.iter().map(std::string::String::as_str).collect();
        for m in find_matches(&query_norm, &composer_cands, 5, 0.6) {
            candidates.push((
                format!("Composer: {}", m.text),
                m.text.to_string(),
                "artist",
                m.score,
            ));

            if m.score > 0.8 {
                let albums_by_composer =
                    self.get_artist_albums(&Artist(m.text.to_string())).await?;
                for alb in albums_by_composer {
                    candidates.push((
                        format!("Album: {} (by {})", alb.0, m.text),
                        alb.0,
                        "album",
                        m.score * 0.98,
                    ));
                }
            }
        }

        // -- Album matches
        let albums = self.get_library_values("Album").await?;
        let album_cands: Vec<&str> = albums.iter().map(std::string::String::as_str).collect();
        for m in find_matches(&query_norm, &album_cands, 5, 0.6) {
            candidates.push((
                format!("Album: {}", m.text),
                m.text.to_string(),
                "album",
                m.score,
            ));
        }

        // -- Playlist matches (Best effort)
        if let Ok(xml) = self.send_command("Playlists/List", &[]).await {
            if let Ok(resp) = from_str::<McwsResponse>(&xml) {
                let playlist_names: Vec<String> = resp
                    .items
                    .into_iter()
                    .flat_map(|i| {
                        i.fields
                            .into_iter()
                            .filter(|f| f.name == "Name")
                            .filter_map(|f| f.value)
                    })
                    .collect();
                let pl_cands: Vec<&str> = playlist_names
                    .iter()
                    .map(std::string::String::as_str)
                    .collect();
                for m in find_matches(&query_norm, &pl_cands, 5, 0.6) {
                    candidates.push((
                        format!("Playlist: {}", m.text),
                        m.text.to_string(),
                        "playlist",
                        m.score,
                    ));
                }
            }
        }

        // 3. Sorting and Decision Logic
        candidates.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));
        // Remove duplicates
        candidates.dedup_by(|a, b| a.1 == b.1 && a.2 == b.2);

        if candidates.is_empty() {
            return match self
                .send_command(
                    "Playback/PlayDoctor",
                    &[("Seed", &query_norm), ("Action", "Play")],
                )
                .await
            {
                Ok(_) => Ok(SearchResult::Done(format!(
                    "Playing generic search: {query_norm}"
                ))),
                Err(_) => Ok(SearchResult::Error(format!(
                    "Nothing found for {query_norm}"
                ))),
            };
        }

        let top = &candidates[0];

        // High Confidence Auto-Play (Top > 0.9 and lead > 0.15)
        let is_clear_winner =
            candidates.len() == 1 || (top.3 > 0.9 && (top.3 - candidates[1].3 > 0.15));

        if is_clear_winner {
            match top.2 {
                "artist" => self.play_artist(&Artist(top.1.clone())).await?,
                "album" => self.play_album(&Album(top.1.clone())).await?,
                "playlist" => {
                    self.send_command(
                        "Playback/PlayPlaylist",
                        &[("Playlist", &top.1), ("PlaylistType", "Name")],
                    )
                    .await?;
                }
                _ => {}
            }
            return Ok(SearchResult::Done(top.0.clone()));
        }

        // Ambiguity -> Return Selection List
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
        let artist_norm = normalize_text(&artist.0);
        let tokens: Vec<&str> = artist_norm.split_whitespace().collect();

        // Search MCWS broadly
        let xml = self
            .send_command(
                "Files/Search",
                &[
                    ("Query", &artist_norm),
                    ("Fields", "Album,Artist,Composer,Name"),
                ],
            )
            .await?;
        let resp: McwsResponse =
            from_str(&xml).map_err(|e| PlayerError::Internal(e.to_string()))?;

        let mut albums = Vec::new();
        for item in resp.items {
            let mut item_album = None;
            let mut full_text = String::new();

            for field in item.fields {
                if field.name == "Album" {
                    item_album.clone_from(&field.value);
                }
                if let Some(val) = field.value {
                    full_text.push_str(&val.to_lowercase());
                    full_text.push(' ');
                }
            }

            if let Some(alb) = item_album {
                // Token-based matching matches Python players/jriver.py::get_artist_albums.
                if tokens.iter().all(|&t| full_text.contains(t)) {
                    let album = Album(alb);
                    if !albums.contains(&album) {
                        albums.push(album);
                    }
                }
            }
        }
        Ok(albums)
    }

    /// Lists the current "Playing Now" queue with 1-based positions.
    /// Mirrors Python `players/jriver.py::list_tracks`: `Playback/Playlist`
    /// returns items with `<Field Name="Name">title</Field>` entries.
    async fn now_playing_queue(&self) -> Result<Vec<(Track, usize)>> {
        let xml = self.send_command("Playback/Playlist", &[]).await?;
        let resp: McwsResponse =
            from_str(&xml).map_err(|e| PlayerError::Internal(e.to_string()))?;

        Ok(resp
            .items
            .into_iter()
            .enumerate()
            .map(|(i, item)| {
                let title = item
                    .fields
                    .into_iter()
                    .find(|f| f.name == "Name")
                    .and_then(|f| f.value)
                    .unwrap_or_else(|| "Unknown Track".to_string());
                (Track(title), i + 1)
            })
            .collect())
    }

    /// Jumps to a 1-based queue position by spamming `Playback/Next` or
    /// `Playback/Previous`. Matches Python `players/jriver.py::go_to_track`;
    /// MCWS has no atomic "seek to index" for the Playing Now queue.
    async fn go_to_track(&self, position: usize) -> Result<()> {
        if position == 0 {
            return Err(PlayerError::NotFound(
                "Track positions are 1-based".to_string(),
            ));
        }

        let info = self.playback_info().await?;
        let current_pos: usize = info
            .get("PlayingNowPosition")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let total_tracks: usize = info
            .get("PlayingNowTracks")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        if total_tracks == 0 {
            return Err(PlayerError::NotFound(
                "Playing Now queue is empty".to_string(),
            ));
        }
        if position > total_tracks {
            return Err(PlayerError::NotFound(format!(
                "Track {position} out of range (queue has {total_tracks} tracks)"
            )));
        }

        let target = position - 1;
        if target == current_pos {
            return Ok(());
        }

        if target > current_pos {
            for _ in 0..(target - current_pos) {
                self.send_command("Playback/Next", &[]).await?;
            }
        } else {
            for _ in 0..(current_pos - target) {
                self.send_command("Playback/Previous", &[]).await?;
            }
        }

        sleep(Duration::from_millis(500)).await;
        let _ = self.what_is_playing_silent().await;
        Ok(())
    }

    /// Lists albums. If `artist` is given, uses `Files/Search?Query=[Artist]=[X]`
    /// and de-duplicates. Otherwise returns cached `Library/Values?Field=Album`.
    /// Mirrors Python `players/jriver.py::list_albums`.
    async fn list_albums(&self, artist: Option<&Artist>) -> Result<Vec<Album>> {
        if let Some(artist) = artist {
            let query = format!("[Artist]=[{}]", artist.0);
            let xml = self
                .send_command("Files/Search", &[("Query", &query), ("Fields", "Album")])
                .await?;
            let resp: McwsResponse =
                from_str(&xml).map_err(|e| PlayerError::Internal(e.to_string()))?;

            let mut seen = std::collections::BTreeSet::new();
            for item in resp.items {
                for field in item.fields {
                    if field.name == "Album" {
                        if let Some(v) = field.value {
                            if !v.is_empty() {
                                seen.insert(v);
                            }
                        }
                    }
                }
            }
            Ok(seen.into_iter().map(Album).collect())
        } else {
            let albums = self.get_library_values("Album").await?;
            Ok(albums.into_iter().map(Album).collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quick_xml::de::from_str;

    #[test]
    fn test_parse_playlists_xml() {
        let xml = r#"
        <Response Status="OK">
            <Item>
                <Field Name="ID">1001</Field>
                <Field Name="Name">Audio - 100 random songs</Field>
            </Item>
            <Item>
                <Field Name="ID">1002</Field>
                <Field Name="Name">My Favorites</Field>
            </Item>
        </Response>
        "#;

        let resp: McwsResponse = from_str(xml).expect("Failed to parse XML");
        assert_eq!(resp.items.len(), 2);

        let random_pl = resp.items.iter().find(|i| {
            i.fields
                .iter()
                .any(|f| f.name == "Name" && f.value.as_deref() == Some("Audio - 100 random songs"))
        });
        assert!(random_pl.is_some());

        let id_field = random_pl.unwrap().fields.iter().find(|f| f.name == "ID");
        assert_eq!(id_field.unwrap().value.as_deref(), Some("1001"));
    }

    #[test]
    fn test_parse_track_metadata_xml() {
        let xml = r#"
        <Response Status="OK">
            <Item>
                <Field Name="Track #">5</Field>
                <Field Name="Disc #">2</Field>
                <Field Name="Name">Bohemian Rhapsody</Field>
            </Item>
        </Response>
        "#;

        let resp: McwsResponse = from_str(xml).expect("Failed to parse XML");
        let item = &resp.items[0];

        let track = item
            .fields
            .iter()
            .find(|f| f.name == "Track #")
            .and_then(|f| f.value.as_deref());
        let disc = item
            .fields
            .iter()
            .find(|f| f.name == "Disc #")
            .and_then(|f| f.value.as_deref());

        assert_eq!(track, Some("5"));
        assert_eq!(disc, Some("2"));
    }
}
