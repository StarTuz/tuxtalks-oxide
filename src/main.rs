//! `TuxTalks` Oxide binary.
//!
//! # Exit codes
//!
//! | Code | Meaning |
//! |------|---------|
//! | `0` | Success |
//! | `1` | Runtime error (`PlayerError`, I/O, D-Bus) or application error (e.g. search found nothing) |
//! | `2` | No subcommand on a default (non-`voice`) build — media CLI only |

use clap::{Parser, Subcommand};
use serde::Serialize;
use std::sync::Arc;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use tuxtalks_oxide::config::{PlayerConfig, PlayerContext};
use tuxtalks_oxide::players::manager::PlayerManager;
use tuxtalks_oxide::utils::speaker::Speaker;
use tuxtalks_oxide::{NowPlaying, SelectionItem};

#[cfg(feature = "voice")]
use tuxtalks_oxide::integration::speech_service;
#[cfg(feature = "voice")]
use tuxtalks_oxide::intelligence::intent::{Intent, IntentEngine};
#[cfg(feature = "voice")]
use zbus::Connection;

#[derive(Parser)]
#[command(
    author,
    version,
    about,
    long_about = "TuxTalks Oxide — media-control CLI. Use a subcommand to play, pause, search, etc. Voice mode is a build-time feature (`cargo build --features voice`) and is still being reworked; the default binary is media CLI only."
)]
struct Cli {
    /// Media control subcommand.
    #[command(subcommand)]
    command: Option<Commands>,

    /// Specific player to use
    #[arg(short, long)]
    player: Option<String>,

    /// Output in JSON format
    #[arg(short, long)]
    json: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Play or resume playback
    Play,
    /// Pause playback
    Pause,
    /// Toggle play/pause
    PlayPause,
    /// Stop playback
    Stop,
    /// Next track
    Next,
    /// Previous track
    Previous,
    /// Volume up
    VolumeUp,
    /// Volume down
    VolumeDown,
    /// What is playing?
    Status,
    /// Search and play anything
    Search { query: String },
    /// Health check all players
    Check,
    /// Scan media directory into local library
    Scan {
        /// Path to scan (overrides config)
        path: Option<String>,
        /// Clear database before scanning
        #[arg(short, long)]
        clear: bool,
    },
    /// Play a playlist by name
    Playlist { name: String },
    /// List the current "Playing Now" queue
    Tracks,
    /// Jump to a track position (1-based) in the current queue.
    /// Accepts digits (`5`), number words (`five`), or compound forms (`twenty one`).
    Goto { position: String },
    /// List albums, optionally filtered by artist
    Albums {
        /// Only show albums by this artist
        #[arg(short, long)]
        artist: Option<String>,
    },

    /// Listen for voice command (uses SpeechD-NG). Requires `voice` feature.
    #[cfg(feature = "voice")]
    Listen {
        /// Text command to simulate (optional)
        #[arg(short, long)]
        text: Option<String>,
    },
    /// Add a voice correction (e.g., "heard" -> "meant"). Requires `voice` feature.
    #[cfg(feature = "voice")]
    AddCorrection { heard: String, meant: String },

    /// Run the voice assistant loop (always listening). Requires `voice` feature.
    #[cfg(feature = "voice")]
    Daemon,
}

#[derive(Serialize)]
struct QueueEntry {
    position: usize,
    title: String,
}

#[derive(Serialize)]
struct JsonSuccess<T: Serialize> {
    ok: bool,
    result: T,
}

#[derive(Serialize)]
struct JsonFailure {
    ok: bool,
    error: JsonErrorBody,
}

#[derive(Serialize)]
struct JsonErrorBody {
    message: String,
    /// Stable machine-readable tag when set (`runtime`, `application`, …).
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'static str>,
}

#[derive(Serialize)]
#[serde(tag = "type", content = "data")]
enum CliOutput {
    Status {
        title: String,
        artist: String,
        album: String,
        player: String,
        raw: String,
    },
    Search {
        result: String,
        options: Vec<SelectionItem>,
    },
    Check {
        results: std::collections::HashMap<String, bool>,
    },
    Tracks {
        tracks: Vec<QueueEntry>,
    },
    Albums {
        albums: Vec<String>,
    },
    Success {
        message: String,
    },
    Error {
        message: String,
    },
    #[cfg(feature = "voice")]
    Listen {
        transcription: String,
        intent: String,
        action_result: String,
    },
    #[cfg(feature = "voice")]
    AddCorrection {
        heard: String,
        meant: String,
    },
}

// `main` holds the full CLI subcommand dispatch table. Splitting it per
// subcommand would scatter the `match` arms and the shared output-handling
// tail across many helpers without making the dispatch any easier to follow.
#[allow(clippy::too_many_lines)]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging (SRE standard)
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    // 1. Load Config
    let config = PlayerConfig::load();

    // 2. Initialize Speaker & Library
    // Speaker pushes utterances into a channel; the spawned worker routes them
    // through speechd-ng → spd-say → tracing. Override with `TUXTALKS_TTS=...`.
    let (speaker, speaker_rx) = Speaker::new();
    tuxtalks_oxide::utils::speaker::spawn_tts_worker(speaker_rx);

    let library = if config.library_db_path.is_empty() {
        None
    } else {
        let db_path = std::path::Path::new(&config.library_db_path);
        Some(Arc::new(tuxtalks_oxide::utils::library::LocalLibrary::new(
            db_path,
        )))
    };

    let ctx = Arc::new(PlayerContext {
        config,
        speaker: Arc::new(speaker),
        library,
    });

    // 3. Initialize Manager
    let mut manager = PlayerManager::new(&ctx);

    // 4. Set active player if specified
    if let Some(id) = cli.player {
        manager.set_active(&id)?;
    }

    // 5. Execute command.
    //
    // With no subcommand and no `voice` feature, we print help and exit 2 — this
    // build is a media-control CLI, not a daemon. Building with `--features voice`
    // restores the assistant loop (and the Daemon/Listen/AddCorrection commands).
    let res: anyhow::Result<Option<CliOutput>> = match &cli.command {
        #[cfg(feature = "voice")]
        None | Some(Commands::Daemon) => {
            if !cli.json {
                println!("Running TuxTalks Oxide voice assistant (feature = voice)");
            }
            let loop_runner = tuxtalks_oxide::active_loop::VoiceLoop::new(ctx.clone());
            loop_runner.run().await?;
            Ok(None)
        }
        #[cfg(not(feature = "voice"))]
        None => {
            eprintln!(
                "tuxtalks-oxide: no subcommand provided.\n\n\
                 This build is media-control only. Run `tuxtalks-oxide --help` for subcommands,\n\
                 or rebuild with `--features voice` to enable the assistant loop."
            );
            std::process::exit(2);
        }
        Some(Commands::Check) => {
            let results = manager.health_check_all().await;
            Ok(Some(CliOutput::Check { results }))
        }
        Some(Commands::Scan { path, clear }) => {
            if let Some(lib) = &ctx.library {
                let scan_path_str = path.as_ref().unwrap_or(&ctx.config.library_path);
                if scan_path_str.is_empty() {
                    return Err(anyhow::anyhow!("No scan path provided or configured"));
                }
                let scan_path = std::path::Path::new(scan_path_str);
                println!("Scanning {}...", scan_path.display());
                lib.scan_directory(scan_path, *clear).await?;
                Ok(Some(CliOutput::Success {
                    message: "Scan complete".to_string(),
                }))
            } else {
                Err(anyhow::anyhow!(
                    "Local library not configured (missing library_db_path)"
                ))
            }
        }
        #[cfg(feature = "voice")]
        Some(Commands::AddCorrection { heard, meant }) => {
            let conn = Connection::session().await?;
            let speech = speech_service::connect(&conn).await?;
            speech.add_correction(heard, meant).await?;
            Ok(Some(CliOutput::AddCorrection {
                heard: heard.clone(),
                meant: meant.clone(),
            }))
        }
        Some(cmd) => {
            let p = manager.get_active()?;
            match cmd {
                Commands::Play => {
                    p.play().await?;
                    Ok(Some(CliOutput::Success {
                        message: "Playback started".to_string(),
                    }))
                }
                Commands::Pause => {
                    p.pause().await?;
                    Ok(Some(CliOutput::Success {
                        message: "Playback paused".to_string(),
                    }))
                }
                Commands::PlayPause => {
                    p.play_pause().await?;
                    Ok(Some(CliOutput::Success {
                        message: "Playback toggled".to_string(),
                    }))
                }
                Commands::Stop => {
                    p.stop().await?;
                    Ok(Some(CliOutput::Success {
                        message: "Playback stopped".to_string(),
                    }))
                }
                Commands::Next => {
                    p.next_track().await?;
                    Ok(Some(CliOutput::Success {
                        message: "Skipped to next track".to_string(),
                    }))
                }
                Commands::Previous => {
                    p.previous_track().await?;
                    Ok(Some(CliOutput::Success {
                        message: "Returned to previous track".to_string(),
                    }))
                }
                Commands::VolumeUp => {
                    p.volume_up().await?;
                    Ok(Some(CliOutput::Success {
                        message: "Volume increased".to_string(),
                    }))
                }
                Commands::VolumeDown => {
                    p.volume_down().await?;
                    Ok(Some(CliOutput::Success {
                        message: "Volume decreased".to_string(),
                    }))
                }
                Commands::Status => {
                    // Announce audibly (matches Python `what_is_playing`) AND
                    // return structured data for text / JSON rendering. One fetch.
                    let np = p.now_playing().await?;
                    ctx.speaker.announce_now_playing(&np).await;
                    let NowPlaying {
                        title,
                        artist,
                        album,
                        player,
                        summary,
                    } = np;
                    Ok(Some(CliOutput::Status {
                        title,
                        artist,
                        album,
                        player,
                        raw: summary,
                    }))
                }
                Commands::Search { query } => match p.play_any(query.as_str()).await? {
                    tuxtalks_oxide::SearchResult::Done(label) => Ok(Some(CliOutput::Search {
                        result: label,
                        options: vec![],
                    })),
                    tuxtalks_oxide::SearchResult::SelectionRequired(options) => {
                        Ok(Some(CliOutput::Search {
                            result: "Selection Required".to_string(),
                            options,
                        }))
                    }
                    tuxtalks_oxide::SearchResult::Error(err) => {
                        Ok(Some(CliOutput::Error { message: err }))
                    }
                },
                Commands::Playlist { name } => {
                    p.play_playlist(name).await?;
                    Ok(Some(CliOutput::Success {
                        message: format!("Playlist {name} started"),
                    }))
                }
                Commands::Tracks => {
                    let queue = p.now_playing_queue().await?;
                    Ok(Some(CliOutput::Tracks {
                        tracks: queue
                            .into_iter()
                            .map(|(t, position)| QueueEntry {
                                position,
                                title: t.0,
                            })
                            .collect(),
                    }))
                }
                Commands::Goto { position } => {
                    let parsed = tuxtalks_oxide::utils::text_normalize::parse_number(position)
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "Could not parse track position from {position:?} (try a digit, \
                                 e.g. `5`, or spoken form like `five` / `twenty one`)"
                            )
                        })?;
                    let parsed = usize::try_from(parsed)
                        .map_err(|_| anyhow::anyhow!("Track position {parsed} out of range"))?;
                    p.go_to_track(parsed).await?;
                    Ok(Some(CliOutput::Success {
                        message: format!("Went to track {parsed}"),
                    }))
                }
                Commands::Albums { artist } => {
                    let artist_arg = artist
                        .as_deref()
                        .map(|a| tuxtalks_oxide::Artist(a.to_string()));
                    let albums = p.list_albums(artist_arg.as_ref()).await?;
                    Ok(Some(CliOutput::Albums {
                        albums: albums.into_iter().map(|a| a.0).collect(),
                    }))
                }
                #[cfg(feature = "voice")]
                Commands::Listen { text } => {
                    let conn = Connection::session().await?;
                    let speech = speech_service::connect(&conn).await?;

                    // 1. Get Text (either argument or Listen() syscall)
                    let raw_text = if let Some(t) = text {
                        t.clone()
                    } else {
                        println!("Listening...");
                        speech.listen_vad().await?
                    };

                    if raw_text.trim().is_empty() {
                        Ok(Some(CliOutput::Error {
                            message: "No speech detected.".to_string(),
                        }))
                    } else {
                        println!("Heard: {raw_text}");

                        // 2. Build Prompt & Think
                        let library_summary = if let Some(lib) = &ctx.library {
                            lib.get_summary().ok()
                        } else {
                            None
                        };
                        let prompt =
                            IntentEngine::construct_prompt(&raw_text, library_summary.as_deref());
                        let json_response = speech.think(&prompt).await?;

                        // 3. Parse Intent
                        let intent = IntentEngine::parse_response(&json_response)?;
                        println!("Intent: {intent:?}");

                        // 4. Execute Intent
                        let action_msg = match &intent {
                            Intent::PlayArtist { artist } => {
                                p.play_artist(&tuxtalks_oxide::Artist(artist.clone()))
                                    .await?;
                                format!("Playing artist {artist}")
                            }
                            Intent::PlayAlbum { album } => {
                                p.play_album(&tuxtalks_oxide::Album(album.clone())).await?;
                                format!("Playing album {album}")
                            }
                            Intent::PlayTrack { track } => {
                                p.play_any(track).await?; // Use flexible search for now
                                format!("Playing track {track}")
                            }
                            Intent::PlayGenre { genre } => {
                                p.play_genre(&tuxtalks_oxide::Genre(genre.clone())).await?;
                                format!("Playing genre {genre}")
                            }
                            Intent::PlayPlaylist { name } => {
                                p.play_playlist(name).await?;
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
                                p.play().await?; // Some players treat play as resume
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
                                // TODO: Route to game manager
                                format!("Game command (not implemented): {command}")
                            }
                            Intent::Unknown {} => "Unknown command".to_string(),
                        };

                        // 5. Speak confirmation
                        Ok(Some(CliOutput::Listen {
                            transcription: raw_text,
                            intent: format!("{intent:?}"),
                            action_result: action_msg,
                        }))
                    }
                }
                Commands::Check | Commands::Scan { .. } => {
                    unreachable!("handled in outer match")
                }
                #[cfg(feature = "voice")]
                Commands::Daemon | Commands::AddCorrection { .. } => {
                    unreachable!("handled in outer match")
                }
            }
        }
    };

    let mut exit_code = 0i32;

    match res {
        Ok(Some(output)) => {
            if cli.json {
                match output {
                    CliOutput::Error { message } => {
                        exit_code = 1;
                        let envelope = JsonFailure {
                            ok: false,
                            error: JsonErrorBody {
                                message,
                                code: Some("application"),
                            },
                        };
                        println!("{}", serde_json::to_string_pretty(&envelope)?);
                    }
                    other => {
                        let envelope = JsonSuccess {
                            ok: true,
                            result: other,
                        };
                        println!("{}", serde_json::to_string_pretty(&envelope)?);
                    }
                }
            } else {
                let is_app_error = matches!(&output, CliOutput::Error { .. });
                if is_app_error {
                    exit_code = 1;
                }
                match output {
                    CliOutput::Status { raw, .. } => {
                        println!("Currently playing: {raw}");
                    }
                    CliOutput::Search { result, options } => {
                        if options.is_empty() {
                            println!("Playing: {result}");
                        } else {
                            println!("Multiple matches found:");
                            for (i, opt) in options.iter().enumerate() {
                                println!("{}: {}", i + 1, opt.label);
                            }
                        }
                    }
                    CliOutput::Check { results } => {
                        for (id, healthy) in results {
                            println!("{}: {}", id, if healthy { "Healthy" } else { "Unhealthy" });
                        }
                    }
                    CliOutput::Tracks { tracks } => {
                        if tracks.is_empty() {
                            println!("Queue is empty");
                        } else {
                            for t in tracks {
                                println!("{:>3}. {}", t.position, t.title);
                            }
                        }
                    }
                    CliOutput::Albums { albums } => {
                        if albums.is_empty() {
                            println!("No albums found");
                        } else {
                            for a in albums {
                                println!("{a}");
                            }
                        }
                    }
                    CliOutput::Success { message } => println!("{message}"),
                    CliOutput::Error { message } => eprintln!("Error: {message}"),
                    #[cfg(feature = "voice")]
                    CliOutput::Listen {
                        transcription: _,
                        intent: _,
                        action_result,
                    } => {
                        println!("Final Action: {action_result}");
                    }
                    #[cfg(feature = "voice")]
                    CliOutput::AddCorrection { heard, meant } => {
                        println!("Added correction: '{heard}' -> '{meant}'");
                    }
                }
            }
        }
        Ok(None) => {}
        Err(e) => {
            exit_code = 1;
            if cli.json {
                let envelope = JsonFailure {
                    ok: false,
                    error: JsonErrorBody {
                        message: e.to_string(),
                        code: Some("runtime"),
                    },
                };
                println!("{}", serde_json::to_string_pretty(&envelope)?);
            } else {
                eprintln!("Error: {e}");
            }
        }
    }

    if exit_code != 0 {
        std::process::exit(exit_code);
    }
    Ok(())
}
