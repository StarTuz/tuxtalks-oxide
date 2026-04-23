use rusqlite::Connection;
use std::sync::Arc;
use tuxtalks_oxide::config::{PlayerConfig, PlayerContext};
use tuxtalks_oxide::players::elisa::ElisaPlayer;
use tuxtalks_oxide::utils::speaker::Speaker;
use tuxtalks_oxide::{Artist, MediaPlayer};

#[tokio::test]
async fn test_elisa_sql_logic() {
    let db_path = "/tmp/elisa_test.db";
    let conn = Connection::open(db_path).unwrap();
    conn.execute("DROP TABLE IF EXISTS Tracks", []).unwrap();
    conn.execute(
        "CREATE TABLE Tracks (
            FileName TEXT,
            ArtistName TEXT,
            AlbumArtistName TEXT,
            AlbumTitle TEXT,
            Genre TEXT,
            TrackNumber INTEGER,
            DiscNumber INTEGER
        )",
        [],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO Tracks (FileName, ArtistName, AlbumTitle, Genre) VALUES (?, ?, ?, ?)",
        ["file:///test/1.mp3", "Daft Punk", "Discovery", "Electronic"],
    )
    .unwrap();

    let mut config = PlayerConfig::load();
    config.elisa_db_path = db_path.to_string();

    let (speaker, _handle) = Speaker::new();
    let ctx = Arc::new(PlayerContext {
        config,
        speaker: Arc::new(speaker),
        library: None,
    });

    let player = ElisaPlayer::new(ctx);

    // Verify artist search
    let albums = player
        .get_artist_albums(&Artist("Daft Punk".to_string()))
        .await
        .unwrap();
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].0, "Discovery");
}
