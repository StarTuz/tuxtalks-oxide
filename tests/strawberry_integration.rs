use rusqlite::Connection;
use std::sync::Arc;
use tuxtalks_oxide::config::{PlayerConfig, PlayerContext};
use tuxtalks_oxide::players::strawberry::StrawberryPlayer;
use tuxtalks_oxide::utils::speaker::Speaker;
use tuxtalks_oxide::{Artist, MediaPlayer};

#[tokio::test]
async fn test_strawberry_sql_logic() {
    let db_path = "/tmp/strawberry_test.db";
    let conn = Connection::open(db_path).unwrap();
    conn.execute("DROP TABLE IF EXISTS songs", []).unwrap();
    conn.execute(
        "CREATE TABLE songs (
            url TEXT,
            artist TEXT,
            album TEXT,
            genre TEXT,
            track INTEGER,
            disc INTEGER
        )",
        [],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO songs (url, artist, album, genre) VALUES (?, ?, ?, ?)",
        ["file:///test/1.mp3", "Pink Floyd", "The Wall", "Rock"],
    )
    .unwrap();

    let mut config = PlayerConfig::load();
    config.strawberry_db_path = db_path.to_string();

    let (speaker, _handle) = Speaker::new();
    let ctx = Arc::new(PlayerContext {
        config,
        speaker: Arc::new(speaker),
        library: None,
    });

    let player = StrawberryPlayer::new(ctx);

    // Verify artist search
    let albums = player
        .get_artist_albums(&Artist("Pink Floyd".to_string()))
        .await
        .unwrap();
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].0, "The Wall");

    // We don't verify play_artist here because it tries to spaw strawberry and connect to mpris
}
