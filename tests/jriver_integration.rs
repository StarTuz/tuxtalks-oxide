use std::sync::Arc;
use tuxtalks_oxide::config::{PlayerConfig, PlayerContext};
use tuxtalks_oxide::players::jriver::JRiverPlayer;
use tuxtalks_oxide::utils::speaker::Speaker;
use tuxtalks_oxide::{MediaPlayer, NowPlaying, SearchResult, Track};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn test_ctx(key: &str) -> Arc<PlayerContext> {
    let mut config = PlayerConfig::load();
    config.jriver_access_key = key.to_string();
    let (speaker, _handle) = Speaker::new();
    Arc::new(PlayerContext {
        config,
        speaker: Arc::new(speaker),
        library: None,
    })
}

fn ok_xml(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body, "text/xml")
}

#[tokio::test]
async fn test_jriver_play() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/MCWS/v1/Playback/Play"))
        .and(query_param("Key", "testkey"))
        .respond_with(ok_xml("<Response Status=\"OK\" />"))
        .mount(&mock_server)
        .await;

    let ctx = test_ctx("testkey");
    let player = JRiverPlayer::with_base_url(ctx, format!("{}/MCWS/v1/", mock_server.uri()));

    assert!(player.play().await.is_ok());
}

#[tokio::test]
async fn test_jriver_pause() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/MCWS/v1/Playback/Pause"))
        .and(query_param("Key", "testkey"))
        .respond_with(ok_xml("<Response Status=\"OK\" />"))
        .mount(&mock_server)
        .await;

    let ctx = test_ctx("testkey");
    let player = JRiverPlayer::with_base_url(ctx, format!("{}/MCWS/v1/", mock_server.uri()));

    assert!(player.pause().await.is_ok());
}

#[tokio::test]
async fn test_jriver_play_pause() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/MCWS/v1/Playback/PlayPause"))
        .and(query_param("Key", "testkey"))
        .respond_with(ok_xml("<Response Status=\"OK\" />"))
        .mount(&mock_server)
        .await;

    let ctx = test_ctx("testkey");
    let player = JRiverPlayer::with_base_url(ctx, format!("{}/MCWS/v1/", mock_server.uri()));

    assert!(player.play_pause().await.is_ok());
}

#[tokio::test]
async fn test_jriver_stop() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/MCWS/v1/Playback/Stop"))
        .and(query_param("Key", "testkey"))
        .respond_with(ok_xml("<Response Status=\"OK\" />"))
        .mount(&mock_server)
        .await;

    let ctx = test_ctx("testkey");
    let player = JRiverPlayer::with_base_url(ctx, format!("{}/MCWS/v1/", mock_server.uri()));

    assert!(player.stop().await.is_ok());
}

fn playback_info_xml() -> &'static str {
    // Real JRiver `Playback/Info` returns flat <Item Name="X">value</Item>
    // entries — see Python `players/jriver.py::what_is_playing_silent`.
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?>
<Response Status="OK">
<Item Name="Name">Comfortably Numb</Item>
<Item Name="Artist">Pink Floyd</Item>
<Item Name="Album">The Wall</Item>
<Item Name="FileKey">-1</Item>
<Item Name="PlayingNowPosition">0</Item>
<Item Name="PlayingNowTracks">12</Item>
</Response>"#
}

#[tokio::test]
async fn test_jriver_next_triggers_info_refresh() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/MCWS/v1/Playback/Next"))
        .and(query_param("Key", "testkey"))
        .respond_with(ok_xml("<Response Status=\"OK\" />"))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/MCWS/v1/Playback/Info"))
        .respond_with(ok_xml(playback_info_xml()))
        .mount(&mock_server)
        .await;

    let ctx = test_ctx("testkey");
    let player = JRiverPlayer::with_base_url(ctx, format!("{}/MCWS/v1/", mock_server.uri()));

    assert!(player.next_track().await.is_ok());
}

#[tokio::test]
async fn test_jriver_previous_triggers_info_refresh() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/MCWS/v1/Playback/Previous"))
        .and(query_param("Key", "testkey"))
        .respond_with(ok_xml("<Response Status=\"OK\" />"))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/MCWS/v1/Playback/Info"))
        .respond_with(ok_xml(playback_info_xml()))
        .mount(&mock_server)
        .await;

    let ctx = test_ctx("testkey");
    let player = JRiverPlayer::with_base_url(ctx, format!("{}/MCWS/v1/", mock_server.uri()));

    assert!(player.previous_track().await.is_ok());
}

#[tokio::test]
async fn test_jriver_volume_up() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/MCWS/v1/Playback/Volume"))
        .and(query_param("Key", "testkey"))
        .and(query_param("Level", "600"))
        .respond_with(ok_xml("<Response Status=\"OK\" />"))
        .mount(&mock_server)
        .await;

    let ctx = test_ctx("testkey");
    let player = JRiverPlayer::with_base_url(ctx, format!("{}/MCWS/v1/", mock_server.uri()));

    assert!(player.volume_up().await.is_ok());
}

#[tokio::test]
async fn test_jriver_volume_down() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/MCWS/v1/Playback/Volume"))
        .and(query_param("Key", "testkey"))
        .and(query_param("Level", "400"))
        .respond_with(ok_xml("<Response Status=\"OK\" />"))
        .mount(&mock_server)
        .await;

    let ctx = test_ctx("testkey");
    let player = JRiverPlayer::with_base_url(ctx, format!("{}/MCWS/v1/", mock_server.uri()));

    assert!(player.volume_down().await.is_ok());
}

#[tokio::test]
async fn test_jriver_what_is_playing() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/MCWS/v1/Playback/Info"))
        .respond_with(ok_xml(playback_info_xml()))
        .mount(&mock_server)
        .await;

    let ctx = test_ctx("testkey");
    let player = JRiverPlayer::with_base_url(ctx, format!("{}/MCWS/v1/", mock_server.uri()));

    let result = player.what_is_playing().await.unwrap();
    assert_eq!(result, "Comfortably Numb by Pink Floyd");
}

#[tokio::test]
async fn test_jriver_now_playing_structured() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/MCWS/v1/Playback/Info"))
        .respond_with(ok_xml(playback_info_xml()))
        .mount(&mock_server)
        .await;

    let ctx = test_ctx("testkey");
    let player = JRiverPlayer::with_base_url(ctx, format!("{}/MCWS/v1/", mock_server.uri()));

    let np = player.now_playing().await.unwrap();
    assert_eq!(
        np,
        NowPlaying {
            title: "Comfortably Numb".to_string(),
            artist: "Pink Floyd".to_string(),
            album: "The Wall".to_string(),
            player: "jriver".to_string(),
            summary: "Comfortably Numb by Pink Floyd".to_string(),
        }
    );
}

#[tokio::test]
async fn test_jriver_playlist_by_fuzzy_name() {
    let mock_server = MockServer::start().await;

    let list_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Response Status="OK">
<Item>
<Field Name="ID">42</Field>
<Field Name="Name">My Favorites</Field>
</Item>
</Response>"#;

    Mock::given(method("GET"))
        .and(path("/MCWS/v1/Playlists/List"))
        .respond_with(ok_xml(list_xml))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/MCWS/v1/Playback/PlayPlaylist"))
        .and(query_param("Key", "testkey"))
        .and(query_param("Playlist", "42"))
        .and(query_param("PlaylistType", "ID"))
        .respond_with(ok_xml("<Response Status=\"OK\" />"))
        .mount(&mock_server)
        .await;

    let ctx = test_ctx("testkey");
    let player = JRiverPlayer::with_base_url(ctx, format!("{}/MCWS/v1/", mock_server.uri()));

    assert!(player.play_playlist("My Favorites").await.is_ok());
}

#[tokio::test]
async fn test_jriver_play_any_falls_back_to_play_doctor() {
    let mock_server = MockServer::start().await;

    let empty_values = "<Response Status=\"OK\"></Response>";

    for field in ["Artist", "Composer", "Album"] {
        Mock::given(method("GET"))
            .and(path("/MCWS/v1/Library/Values"))
            .and(query_param("Field", field))
            .respond_with(ok_xml(empty_values))
            .mount(&mock_server)
            .await;
    }

    Mock::given(method("GET"))
        .and(path("/MCWS/v1/Playlists/List"))
        .respond_with(ok_xml(empty_values))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/MCWS/v1/Playback/PlayDoctor"))
        .and(query_param("Key", "testkey"))
        .and(query_param("Action", "Play"))
        .respond_with(ok_xml("<Response Status=\"OK\" />"))
        .mount(&mock_server)
        .await;

    let ctx = test_ctx("testkey");
    let player = JRiverPlayer::with_base_url(ctx, format!("{}/MCWS/v1/", mock_server.uri()));

    let res = player.play_any("obscure artist xyz").await.unwrap();
    match res {
        SearchResult::Done(s) => assert!(s.contains("Playing generic search")),
        other => panic!("expected Done, got {other:?}"),
    }
}

#[tokio::test]
async fn test_jriver_now_playing_queue() {
    let mock_server = MockServer::start().await;

    let playlist_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Response Status="OK">
<Item>
<Field Name="Name">Speak to Me</Field>
</Item>
<Item>
<Field Name="Name">Breathe</Field>
</Item>
<Item>
<Field Name="Name">Time</Field>
</Item>
</Response>"#;

    Mock::given(method("GET"))
        .and(path("/MCWS/v1/Playback/Playlist"))
        .and(query_param("Key", "testkey"))
        .respond_with(ok_xml(playlist_xml))
        .mount(&mock_server)
        .await;

    let ctx = test_ctx("testkey");
    let player = JRiverPlayer::with_base_url(ctx, format!("{}/MCWS/v1/", mock_server.uri()));

    let queue = player.now_playing_queue().await.unwrap();
    assert_eq!(
        queue
            .into_iter()
            .map(|(Track(t), i)| (t, i))
            .collect::<Vec<_>>(),
        vec![
            ("Speak to Me".to_string(), 1),
            ("Breathe".to_string(), 2),
            ("Time".to_string(), 3),
        ]
    );
}

#[tokio::test]
async fn test_jriver_go_to_track_forward() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use wiremock::matchers::any;

    let mock_server = MockServer::start().await;

    // Position 0 of 12 total (from playback_info_xml).
    Mock::given(method("GET"))
        .and(path("/MCWS/v1/Playback/Info"))
        .respond_with(ok_xml(playback_info_xml()))
        .mount(&mock_server)
        .await;

    let next_calls = Arc::new(AtomicUsize::new(0));
    let next_calls_for_mock = next_calls.clone();
    Mock::given(method("GET"))
        .and(path("/MCWS/v1/Playback/Next"))
        .respond_with(move |_: &wiremock::Request| {
            next_calls_for_mock.fetch_add(1, Ordering::SeqCst);
            ok_xml("<Response Status=\"OK\" />")
        })
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/MCWS/v1/Playback/Previous"))
        .respond_with(ok_xml("<Response Status=\"OK\" />"))
        .mount(&mock_server)
        .await;

    Mock::given(any())
        .respond_with(ok_xml("<Response Status=\"OK\" />"))
        .mount(&mock_server)
        .await;

    let ctx = test_ctx("testkey");
    let player = JRiverPlayer::with_base_url(ctx, format!("{}/MCWS/v1/", mock_server.uri()));

    player.go_to_track(5).await.unwrap();
    // current_pos=0, target=4 → 4 "Next" calls.
    assert_eq!(next_calls.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn test_jriver_go_to_track_out_of_range() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/MCWS/v1/Playback/Info"))
        .respond_with(ok_xml(playback_info_xml()))
        .mount(&mock_server)
        .await;

    let ctx = test_ctx("testkey");
    let player = JRiverPlayer::with_base_url(ctx, format!("{}/MCWS/v1/", mock_server.uri()));

    let err = player.go_to_track(99).await.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("out of range"), "unexpected error: {msg}");
}

#[tokio::test]
async fn test_jriver_list_albums_for_artist() {
    let mock_server = MockServer::start().await;

    let search_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Response Status="OK">
<Item>
<Field Name="Album">The Wall</Field>
</Item>
<Item>
<Field Name="Album">Dark Side of the Moon</Field>
</Item>
<Item>
<Field Name="Album">The Wall</Field>
</Item>
</Response>"#;

    Mock::given(method("GET"))
        .and(path("/MCWS/v1/Files/Search"))
        .and(query_param("Query", "[Artist]=[Pink Floyd]"))
        .and(query_param("Fields", "Album"))
        .respond_with(ok_xml(search_xml))
        .mount(&mock_server)
        .await;

    let ctx = test_ctx("testkey");
    let player = JRiverPlayer::with_base_url(ctx, format!("{}/MCWS/v1/", mock_server.uri()));

    let albums = player
        .list_albums(Some(&tuxtalks_oxide::Artist("Pink Floyd".to_string())))
        .await
        .unwrap();
    // Deduplicated + sorted (BTreeSet): "Dark Side of the Moon", "The Wall".
    assert_eq!(
        albums.into_iter().map(|a| a.0).collect::<Vec<_>>(),
        vec!["Dark Side of the Moon".to_string(), "The Wall".to_string(),]
    );
}

#[tokio::test]
async fn test_jriver_list_albums_all() {
    let mock_server = MockServer::start().await;

    let values_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Response Status="OK">
<Item>A Night at the Opera</Item>
<Item>Abbey Road</Item>
</Response>"#;

    Mock::given(method("GET"))
        .and(path("/MCWS/v1/Library/Values"))
        .and(query_param("Field", "Album"))
        .respond_with(ok_xml(values_xml))
        .mount(&mock_server)
        .await;

    let ctx = test_ctx("testkey");
    let player = JRiverPlayer::with_base_url(ctx, format!("{}/MCWS/v1/", mock_server.uri()));

    let albums = player.list_albums(None).await.unwrap();
    assert_eq!(
        albums.into_iter().map(|a| a.0).collect::<Vec<_>>(),
        vec!["A Night at the Opera".to_string(), "Abbey Road".to_string(),]
    );
}

#[tokio::test]
async fn test_jriver_health_check_alive() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/MCWS/v1/Alive"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    let ctx = test_ctx("testkey");
    let player = JRiverPlayer::with_base_url(ctx, format!("{}/MCWS/v1/", mock_server.uri()));

    assert!(player.health_check().await);
}
