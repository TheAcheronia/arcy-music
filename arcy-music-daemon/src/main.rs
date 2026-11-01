use arcy_music_core::{
    Command, EqConfig, PlaybackState, RepeatMode, Track, TrackSource,
};
mod api;
mod dsp;
mod ffmpeg_source;
mod indexer;
mod lyrics;
mod mpris;
mod plugins;
mod spotify;

use axum::{
    extract::{Json, State},
    routing::{delete, get, post},
    Router,
};
use rand::Rng;
use rusqlite::Connection;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tower_http::cors::CorsLayer;

#[derive(Clone)]
pub struct ResolvedStream {
    pub url: String,
    pub duration_secs: Option<u64>,
    pub thumbnail_url: Option<String>,
    pub expires_at: std::time::Instant,
}

pub type StreamCache = Arc<std::sync::Mutex<std::collections::HashMap<String, ResolvedStream>>>;

pub struct AppState {
    pub db: Mutex<Connection>,
    pub command_tx: mpsc::Sender<Command>,
    pub playback_state: Mutex<PlaybackState>,
    pub eq_config: Arc<RwLock<EqConfig>>,
    pub mpris_state_tx: mpsc::Sender<PlaybackState>,
    pub active_theme: Arc<tokio::sync::RwLock<String>>,
    pub spotify_login_status: Arc<tokio::sync::RwLock<api::SpotifyLoginStatus>>,
    pub stream_cache: StreamCache,
}

pub fn get_cache_key(track: &Track) -> Option<String> {
    match &track.source {
        TrackSource::YouTube { video_id } => Some(format!("yt:{}", video_id)),
        TrackSource::Spotify { .. } => Some(format!("sp:{}", track.id)),
        _ => None,
    }
}

pub fn get_cached_stream(cache: &StreamCache, key: &str) -> Option<ResolvedStream> {
    let mut lock = cache.lock().unwrap();
    if let Some(entry) = lock.get(key) {
        if std::time::Instant::now() < entry.expires_at {
            return Some(entry.clone());
        } else {
            lock.remove(key);
        }
    }
    None
}

pub fn set_cached_stream(cache: &StreamCache, key: String, stream: ResolvedStream) {
    let mut lock = cache.lock().unwrap();
    lock.insert(key, stream);
}

pub fn resolve_youtube_stream_fast(video_id: &str) -> Option<(String, Option<u64>, Option<String>)> {
    if !arcy_music_core::is_valid_youtube_id(video_id) {
        return None;
    }
    for client in ["visionos", "tv_samsung"] {
        let url_cmd = std::process::Command::new("yt-dlp")
            .args([
                "--no-playlist",
                "--no-warnings",
                "--extractor-args", &format!("youtube:player_client={}", client),
                "-f", "251/140/bestaudio",
                "--print", "%(url)s",
                "--print", "%(duration)s",
                "--print", "%(thumbnail)s",
                "--",
                &format!("https://www.youtube.com/watch?v={}", video_id),
            ])
            .output();

        if let Ok(output) = url_cmd {
            if output.status.success() {
                let output_str = String::from_utf8_lossy(&output.stdout);
                let mut lines = output_str.lines();
                let url = lines.next().unwrap_or("").trim().to_string();
                let dur_str = lines.next().unwrap_or("").trim();
                let thumb_str = lines.next().unwrap_or("").trim();
                if !url.is_empty() {
                    let duration = dur_str.parse::<f64>().ok().map(|d| d as u64);
                    let thumb = if !thumb_str.is_empty() && thumb_str != "NA" {
                        Some(thumb_str.to_string())
                    } else {
                        None
                    };
                    return Some((url, duration, thumb));
                }
            }
        }
    }
    None
}

pub fn resolve_spotify_stream_fast(title: &str, artist: &str) -> Option<(String, Option<u64>, Option<String>)> {
    let query = format!("ytsearch1:{} {}", title, artist);
    for client in ["visionos", "tv_samsung"] {
        let url_cmd = std::process::Command::new("yt-dlp")
            .args([
                "--no-playlist",
                "--no-warnings",
                "--extractor-args", &format!("youtube:player_client={}", client),
                "-f", "251/140/bestaudio",
                "--print", "%(url)s",
                "--print", "%(duration)s",
                "--print", "%(thumbnail)s",
                "--",
                &query,
            ])
            .output();

        if let Ok(output) = url_cmd {
            if output.status.success() {
                let output_str = String::from_utf8_lossy(&output.stdout);
                let mut lines = output_str.lines();
                let url = lines.next().unwrap_or("").trim().to_string();
                let dur_str = lines.next().unwrap_or("").trim();
                let thumb_str = lines.next().unwrap_or("").trim();
                if !url.is_empty() {
                    let duration = dur_str.parse::<f64>().ok().map(|d| d as u64);
                    let thumb = if !thumb_str.is_empty() && thumb_str != "NA" {
                        Some(thumb_str.to_string())
                    } else {
                        None
                    };
                    return Some((url, duration, thumb));
                }
            }
        }
    }
    None
}

pub fn prefetch_stream(track: &Track, cache: &StreamCache) {
    if let Some(key) = get_cache_key(track) {
        if get_cached_stream(cache, &key).is_some() {
            return;
        }
        println!("Background prefetching audio stream for upcoming track: '{}'", track.title);
        let res = match &track.source {
            TrackSource::YouTube { video_id } => resolve_youtube_stream_fast(video_id),
            TrackSource::Spotify { .. } => resolve_spotify_stream_fast(&track.title, track.artist.as_deref().unwrap_or("")),
            _ => None,
        };
        if let Some((url, dur, thumb)) = res {
            let entry = ResolvedStream {
                url,
                duration_secs: dur,
                thumbnail_url: thumb,
                expires_at: std::time::Instant::now() + Duration::from_secs(4 * 3600),
            };
            set_cached_stream(cache, key, entry);
            println!("Prefetched audio stream successfully cached for: '{}'", track.title);
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting Arcy Music Daemon (Performance & Security Core)...");

    // Setup SQLite database
    let db_path = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("arcy-music");
    std::fs::create_dir_all(&db_path)?;

    let db_file = db_path.join("library.db");
    let conn = Connection::open(&db_file)?;

    // Enable WAL mode for blazing fast concurrent reads/writes
    conn.pragma_update(None, "journal_mode", "WAL")?;

    // Initialize database schema
    conn.execute(
        "CREATE TABLE IF NOT EXISTS tracks (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            artist TEXT,
            album TEXT,
            duration_secs INTEGER,
            source_type TEXT NOT NULL,
            source_data TEXT NOT NULL,
            thumbnail_url TEXT
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS lyrics (
            track_id TEXT PRIMARY KEY,
            plain_lyrics TEXT,
            synced_lyrics TEXT
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS playlists (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            is_pinned INTEGER NOT NULL DEFAULT 0,
            sort_order INTEGER NOT NULL DEFAULT 0,
            last_played INTEGER NOT NULL DEFAULT 0,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS playlist_tracks (
            playlist_id TEXT NOT NULL,
            track_id TEXT NOT NULL,
            position INTEGER NOT NULL,
            PRIMARY KEY (playlist_id, track_id)
        )",
        [],
    )?;

    // Load persisted EQ settings
    let mut initial_eq = EqConfig::default();
    if let Ok(json_str) = conn.query_row::<String, _, _>(
        "SELECT value FROM settings WHERE key = 'eq_config'",
        [],
        |row| row.get(0),
    ) {
        if let Ok(loaded) = serde_json::from_str(&json_str) {
            initial_eq = loaded;
        }
    }
    let eq_config = Arc::new(RwLock::new(initial_eq));

    // Load persisted Volume settings (default 0.5)
    let mut initial_volume: f32 = 0.5;
    if let Ok(val_str) = conn.query_row::<String, _, _>(
        "SELECT value FROM settings WHERE key = 'volume'",
        [],
        |row| row.get(0),
    ) {
        if let Ok(v) = val_str.parse::<f32>() {
            initial_volume = v.clamp(0.0, 1.0);
        }
    }

    // Restore persisted audio sink if configured
    if let Ok(sink_name) = conn.query_row::<String, _, _>(
        "SELECT value FROM settings WHERE key = 'audio_sink'",
        [],
        |row| row.get(0),
    ) {
        if !sink_name.is_empty() {
            let _ = std::process::Command::new("pactl")
                .args(["set-default-sink", &sink_name])
                .status();
        }
    }

    // Run background directory indexer
    let music_dir = dirs::audio_dir().unwrap_or_else(|| dirs::home_dir().unwrap().join("Music"));
    if music_dir.exists() {
        let mut index_conn = Connection::open(&db_file)?;
        let music_dir_str = music_dir.to_string_lossy().to_string();
        std::thread::spawn(move || {
            println!("Indexing local music library: {}", music_dir_str);
            if let Err(e) = indexer::sync_directory(&mut index_conn, &music_dir_str) {
                eprintln!("Indexing error: {}", e);
            }
        });
    }

    // Channels
    let (command_tx, command_rx) = std::sync::mpsc::channel::<Command>();
    let (async_cmd_tx, mut async_cmd_rx) = mpsc::channel::<Command>(64);
    let (mpris_state_tx, mpris_state_rx) = mpsc::channel::<PlaybackState>(32);

    // Bridge async commands to sync channel for audio thread
    let bridge_tx = command_tx.clone();
    tokio::spawn(async move {
        while let Some(cmd) = async_cmd_rx.recv().await {
            let _ = bridge_tx.send(cmd);
        }
    });

    let initial_playback_state = PlaybackState {
        current_track: None,
        is_playing: false,
        volume: initial_volume,
        position_secs: 0.0,
        duration_secs: 0.0,
        queue: Vec::new(),
        queue_index: 0,
        shuffle: false,
        repeat_mode: RepeatMode::Off,
    };

    let active_theme = Arc::new(tokio::sync::RwLock::new("mocha".to_string()));
    let stream_cache: StreamCache = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));

    let shared_state = Arc::new(AppState {
        db: Mutex::new(conn),
        command_tx: async_cmd_tx.clone(),
        playback_state: Mutex::new(initial_playback_state),
        eq_config: eq_config.clone(),
        mpris_state_tx: mpris_state_tx.clone(),
        active_theme,
        spotify_login_status: Arc::new(tokio::sync::RwLock::new(api::SpotifyLoginStatus {
            status: "idle".to_string(),
            message: String::new(),
            sp_dc: None,
        })),
        stream_cache: stream_cache.clone(),
    });

    // Start MPRIS2 D-Bus service
    mpris::start_mpris_service(async_cmd_tx.clone(), mpris_state_rx);

    // Start Dedicated Audio Thread
    let audio_state = shared_state.clone();
    let audio_eq = eq_config.clone();
    let audio_stream_cache = stream_cache.clone();
    let tokio_rt = tokio::runtime::Handle::current();
    std::thread::spawn(move || {
        println!("Audio DSP engine thread initialized");

        let is_silent = std::env::var("ARCY_SILENT")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let handle = rodio::DeviceSinkBuilder::open_default_sink().unwrap();
        let player = rodio::Player::connect_new(&handle.mixer());
        if is_silent {
            println!("ARCY_SILENT=1 active: Hardware audio output completely muted (volume 0.0)");
            player.set_volume(0.0);
        } else {
            player.set_volume(initial_volume);
        }

        let mut current_track_active = false;
        let mut last_play_req: Option<(String, std::time::Instant)> = None;

        loop {
            // Receive command: 200ms when playing, 1000ms when idle for near-zero CPU consumption
            let timeout = if current_track_active {
                Duration::from_millis(200)
            } else {
                Duration::from_millis(1000)
            };
            match command_rx.recv_timeout(timeout) {
                Ok(cmd) => {
                    match cmd {
                        Command::Play => {
                            player.play();
                            current_track_active = true;
                            let rt = tokio_rt.clone();
                            let s = audio_state.clone();
                            rt.spawn(async move {
                                let mut ps = s.playback_state.lock().await;
                                ps.is_playing = true;
                                let _ = s.mpris_state_tx.try_send(ps.clone());
                            });
                        }
                        Command::Pause => {
                            player.pause();
                            let rt = tokio_rt.clone();
                            let s = audio_state.clone();
                            rt.spawn(async move {
                                let mut ps = s.playback_state.lock().await;
                                ps.is_playing = false;
                                let _ = s.mpris_state_tx.try_send(ps.clone());
                            });
                        }
                        Command::PlayPause => {
                            if player.is_paused() {
                                player.play();
                                current_track_active = true;
                            } else {
                                player.pause();
                            }
                            let rt = tokio_rt.clone();
                            let s = audio_state.clone();
                            let is_p = !player.is_paused();
                            rt.spawn(async move {
                                let mut ps = s.playback_state.lock().await;
                                ps.is_playing = is_p;
                                let _ = s.mpris_state_tx.try_send(ps.clone());
                            });
                        }
                        Command::Stop => {
                            player.stop();
                            current_track_active = false;
                            let rt = tokio_rt.clone();
                            let s = audio_state.clone();
                            rt.spawn(async move {
                                let mut ps = s.playback_state.lock().await;
                                ps.is_playing = false;
                                ps.position_secs = 0.0;
                                let _ = s.mpris_state_tx.try_send(ps.clone());
                            });
                        }
                        Command::SetVolume(vol) => {
                            let clamped_vol = vol.clamp(0.0, 1.0);
                            if is_silent {
                                player.set_volume(0.0);
                            } else {
                                player.set_volume(clamped_vol);
                            }
                            let rt = tokio_rt.clone();
                            let s = audio_state.clone();
                            rt.spawn(async move {
                                {
                                    let db = s.db.lock().await;
                                    let _ = db.execute(
                                        "INSERT OR REPLACE INTO settings (key, value) VALUES ('volume', ?1)",
                                        [clamped_vol.to_string()],
                                    );
                                }
                                let mut ps = s.playback_state.lock().await;
                                ps.volume = clamped_vol;
                                let _ = s.mpris_state_tx.try_send(ps.clone());
                            });
                        }
                        Command::Seek(pos) => {
                            let seek_res = player.try_seek(Duration::from_secs_f32(pos));
                            if let Err(ref e) = seek_res {
                                eprintln!("Audio seek error for pos {:.2}s: {:?}", pos, e);
                            }
                            let rt = tokio_rt.clone();
                            let s = audio_state.clone();
                            rt.spawn(async move {
                                let mut ps = s.playback_state.lock().await;
                                ps.position_secs = pos;
                            });
                        }
                        Command::PlayTrack(mut track) => {
                            println!("Loading track: {} - {:?}", track.title, track.artist);

                            // Debounce rapid duplicate play requests (e.g. double click)
                            if let Some((ref last_id, ref last_time)) = last_play_req {
                                if last_id == &track.id && last_time.elapsed() < Duration::from_millis(3000) {
                                    println!("Debounce: Ignoring duplicate PlayTrack for track '{}' within 3s", track.id);
                                    continue;
                                }
                            }
                            last_play_req = Some((track.id.clone(), std::time::Instant::now()));

                            // Immediately stop any existing playback so previous song stops at once
                            player.stop();
                            current_track_active = false;

                            // Immediately update state with current track info and reset position
                            {
                                let rt = tokio_rt.clone();
                                let s = audio_state.clone();
                                let t_prev = track.clone();
                                rt.spawn(async move {
                                    let mut ps = s.playback_state.lock().await;
                                    ps.current_track = Some(t_prev.clone());
                                    ps.is_playing = false;
                                    ps.position_secs = 0.0;
                                    if let Some(dur) = t_prev.duration_secs {
                                        ps.duration_secs = dur as f32;
                                    }
                                    let _ = s.mpris_state_tx.try_send(ps.clone());
                                });
                            }

                            match &track.source {
                                TrackSource::Local { path } => {
                                    if let Ok(file) = std::fs::File::open(path) {
                                        if let Ok(decoder) = rodio::Decoder::try_from(file) {
                                            let safe_source = crate::dsp::SafeAudioWrapper::new(decoder, audio_eq.clone());
                                            player.stop();
                                            player.append(safe_source);
                                            player.play();
                                            current_track_active = true;
                                        } else {
                                            eprintln!("Audio: Failed to decode local audio: {}", path);
                                        }
                                    }
                                }
                                TrackSource::YouTube { video_id } => {
                                    if !arcy_music_core::is_valid_youtube_id(video_id) {
                                        eprintln!("Security: Blocked invalid YouTube ID: {}", video_id);
                                        continue;
                                    }

                                    let cache_key = get_cache_key(&track);
                                    let cached_info = cache_key.as_ref().and_then(|k| get_cached_stream(&audio_stream_cache, k));
                                    let (url, dur, thumb_url) = if let Some(cached) = cached_info {
                                        println!("Instant playback (cache HIT) for YouTube track: '{}'", track.title);
                                        (cached.url, cached.duration_secs, cached.thumbnail_url)
                                    } else {
                                        println!("Fast resolving YouTube stream for: '{}' (ID: {})...", track.title, video_id);
                                        let start_resolve = std::time::Instant::now();
                                        if let Some((u, d, t)) = resolve_youtube_stream_fast(video_id) {
                                            println!("Resolved YouTube stream in {:.2?} for '{}'", start_resolve.elapsed(), track.title);
                                            if let Some(ref k) = cache_key {
                                                set_cached_stream(&audio_stream_cache, k.clone(), ResolvedStream {
                                                    url: u.clone(),
                                                    duration_secs: d,
                                                    thumbnail_url: t.clone(),
                                                    expires_at: std::time::Instant::now() + Duration::from_secs(4 * 3600),
                                                });
                                            }
                                            (u, d, t)
                                        } else {
                                            eprintln!("Audio: Failed to resolve YouTube stream for {}", video_id);
                                            continue;
                                        }
                                    };

                                    if track.duration_secs.is_none() || track.duration_secs == Some(0) {
                                        if let Some(d) = dur {
                                            track.duration_secs = Some(d);
                                        }
                                    }

                                    // Thumbnail: Use local cached file or remote URL immediately without blocking audio playback
                                    let thumbs_dir = dirs::data_dir()
                                        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
                                        .join("arcy-music/thumbnails");
                                    let cached_thumb = thumbs_dir.join(format!("yt_{}.jpg", video_id));

                                    if cached_thumb.exists() {
                                        track.thumbnail_url = Some(cached_thumb.to_string_lossy().to_string());
                                    } else {
                                        let target_url = thumb_url.unwrap_or_else(|| format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", video_id));
                                        track.thumbnail_url = Some(target_url.clone());
                                        // Detached non-blocking background thumbnail download and save
                                        let s = audio_state.clone();
                                        let t_id = track.id.clone();
                                        let vid = video_id.clone();
                                        tokio_rt.spawn(async move {
                                            let _ = std::fs::create_dir_all(&thumbs_dir);
                                            let temp_thumb = thumbs_dir.join(format!("temp_yt_{}", vid));
                                            let res = tokio::process::Command::new("curl")
                                                .args(["-s", "-f", "-L", &target_url, "-o", &temp_thumb.to_string_lossy()])
                                                .status()
                                                .await;
                                            if let Ok(st) = res {
                                                if st.success() {
                                                    if let Ok(dyn_img) = image::open(&temp_thumb) {
                                                        let _ = dyn_img.to_rgb8().save_with_format(&cached_thumb, image::ImageFormat::Jpeg);
                                                        let _ = std::fs::remove_file(&temp_thumb);
                                                    } else if temp_thumb.exists() {
                                                        let _ = std::fs::rename(&temp_thumb, &cached_thumb);
                                                    }
                                                    if cached_thumb.exists() {
                                                        let local_path = cached_thumb.to_string_lossy().to_string();
                                                        let mut ps = s.playback_state.lock().await;
                                                        if let Some(ref mut cur) = ps.current_track {
                                                            if cur.id == t_id {
                                                                cur.thumbnail_url = Some(local_path.clone());
                                                            }
                                                        }
                                                        if let Some(pos) = ps.queue.iter().position(|q| q.id == t_id) {
                                                            ps.queue[pos].thumbnail_url = Some(local_path);
                                                        }
                                                    }
                                                }
                                            }
                                        });
                                    }

                                    // Instantly open FFmpegSource and play
                                    if let Ok(ff_source) = crate::ffmpeg_source::FFmpegSource::new(&url) {
                                        let safe_source = crate::dsp::SafeAudioWrapper::new(ff_source, audio_eq.clone());
                                        player.stop();
                                        player.append(safe_source);
                                        player.play();
                                        current_track_active = true;
                                    } else {
                                        eprintln!("Audio: Failed to open FFmpeg source for YouTube URL: {}", url);
                                        continue;
                                    }
                                }
                                TrackSource::Spotify { .. } => {
                                    let cache_key = get_cache_key(&track);
                                    let cached_info = cache_key.as_ref().and_then(|k| get_cached_stream(&audio_stream_cache, k));
                                    let (url, dur, _thumb_url) = if let Some(cached) = cached_info {
                                        println!("Instant playback (cache HIT) for Spotify track: '{}'", track.title);
                                        (cached.url, cached.duration_secs, cached.thumbnail_url)
                                    } else {
                                        println!("Resolving audio stream for Spotify track: '{} - {:?}'", track.title, track.artist);
                                        let start_resolve = std::time::Instant::now();
                                        if let Some((u, d, t)) = resolve_spotify_stream_fast(&track.title, track.artist.as_deref().unwrap_or("")) {
                                            println!("Resolved Spotify track audio stream in {:.2?} for '{}'", start_resolve.elapsed(), track.title);
                                            if let Some(ref k) = cache_key {
                                                set_cached_stream(&audio_stream_cache, k.clone(), ResolvedStream {
                                                    url: u.clone(),
                                                    duration_secs: d,
                                                    thumbnail_url: t.clone(),
                                                    expires_at: std::time::Instant::now() + Duration::from_secs(4 * 3600),
                                                });
                                            }
                                            (u, d, t)
                                        } else {
                                            eprintln!("Audio: Failed to resolve stream for Spotify track: {}", track.title);
                                            continue;
                                        }
                                    };

                                    if track.duration_secs.is_none() || track.duration_secs == Some(0) {
                                        if let Some(d) = dur {
                                            track.duration_secs = Some(d);
                                        }
                                    }

                                    if let Ok(ff_source) = crate::ffmpeg_source::FFmpegSource::new(&url) {
                                        let safe_source = crate::dsp::SafeAudioWrapper::new(ff_source, audio_eq.clone());
                                        player.stop();
                                        player.append(safe_source);
                                        player.play();
                                        current_track_active = true;
                                    } else {
                                        eprintln!("Audio: Failed to open FFmpeg source for Spotify track URL: {}", url);
                                        continue;
                                    }
                                }
                                _ => {}
                            }

                            // Update last_play_req now that loading/resolving has finished
                            last_play_req = Some((track.id.clone(), std::time::Instant::now()));

                            let rt = tokio_rt.clone();
                            let s = audio_state.clone();
                            let t = track.clone();
                            let cache_for_prefetch = audio_stream_cache.clone();
                            rt.spawn(async move {
                                let mut ps = s.playback_state.lock().await;
                                ps.current_track = Some(t.clone());
                                ps.is_playing = true;
                                ps.position_secs = 0.0;
                                ps.duration_secs = t.duration_secs.unwrap_or(0) as f32;

                                // Sync queue
                                if let Some(pos) = ps.queue.iter().position(|q| q.id == t.id) {
                                    ps.queue_index = pos;
                                } else {
                                    ps.queue.push(t.clone());
                                    ps.queue_index = ps.queue.len() - 1;
                                }
                                let _ = s.mpris_state_tx.try_send(ps.clone());

                                // Prefetch upcoming track in queue to guarantee zero-latency transition
                                let next_track_opt = if ps.queue.len() > 1 && ps.queue_index + 1 < ps.queue.len() {
                                    Some(ps.queue[ps.queue_index + 1].clone())
                                } else {
                                    None
                                };
                                drop(ps);

                                if let Some(next_track) = next_track_opt {
                                    tokio::task::spawn_blocking(move || {
                                        prefetch_stream(&next_track, &cache_for_prefetch);
                                    });
                                }
                            });
                        }
                        Command::Next => {
                            let rt = tokio_rt.clone();
                            let s = audio_state.clone();
                            let bridge = s.command_tx.clone();
                            rt.spawn(async move {
                                let mut ps = s.playback_state.lock().await;
                                if ps.queue.is_empty() {
                                    return;
                                }
                                match ps.repeat_mode {
                                    RepeatMode::One => {
                                        if let Some(track) = ps.current_track.clone() {
                                            let _ = bridge.send(Command::PlayTrack(track)).await;
                                        }
                                    }
                                    _ => {
                                        let next_idx = if ps.shuffle {
                                            rand::rng().random_range(0..ps.queue.len())
                                        } else if ps.queue_index + 1 < ps.queue.len() {
                                            ps.queue_index + 1
                                        } else if ps.repeat_mode == RepeatMode::All {
                                            0
                                        } else {
                                            ps.is_playing = false;
                                            let _ = s.mpris_state_tx.try_send(ps.clone());
                                            return;
                                        };
                                        ps.queue_index = next_idx;
                                        if let Some(track) = ps.queue.get(next_idx).cloned() {
                                            let _ = bridge.send(Command::PlayTrack(track)).await;
                                        }
                                    }
                                }
                            });
                        }
                        Command::Previous => {
                            let rt = tokio_rt.clone();
                            let s = audio_state.clone();
                            let bridge = s.command_tx.clone();
                            rt.spawn(async move {
                                let mut ps = s.playback_state.lock().await;
                                if ps.position_secs > 3.0 {
                                    // Rewind to beginning
                                    let _ = bridge.send(Command::Seek(0.0)).await;
                                    return;
                                }
                                if ps.queue.is_empty() {
                                    return;
                                }
                                let prev_idx = if ps.queue_index > 0 {
                                    ps.queue_index - 1
                                } else if ps.repeat_mode == RepeatMode::All {
                                    ps.queue.len().saturating_sub(1)
                                } else {
                                    0
                                };
                                ps.queue_index = prev_idx;
                                if let Some(track) = ps.queue.get(prev_idx).cloned() {
                                    let _ = bridge.send(Command::PlayTrack(track)).await;
                                }
                            });
                        }
                        Command::ClearQueue => {
                            let rt = tokio_rt.clone();
                            let s = audio_state.clone();
                            rt.spawn(async move {
                                let mut ps = s.playback_state.lock().await;
                                ps.queue.clear();
                                ps.queue_index = 0;
                            });
                        }
                        Command::RemoveFromQueue(idx) => {
                            let rt = tokio_rt.clone();
                            let s = audio_state.clone();
                            rt.spawn(async move {
                                let mut ps = s.playback_state.lock().await;
                                if idx < ps.queue.len() {
                                    ps.queue.remove(idx);
                                    if ps.queue_index >= ps.queue.len() {
                                        ps.queue_index = ps.queue.len().saturating_sub(1);
                                    }
                                }
                            });
                        }
                        Command::SetShuffle(sh) => {
                            let rt = tokio_rt.clone();
                            let s = audio_state.clone();
                            rt.spawn(async move {
                                let mut ps = s.playback_state.lock().await;
                                ps.shuffle = sh;
                            });
                        }
                        Command::SetRepeat(rep) => {
                            let rt = tokio_rt.clone();
                            let s = audio_state.clone();
                            rt.spawn(async move {
                                let mut ps = s.playback_state.lock().await;
                                ps.repeat_mode = rep;
                            });
                        }
                        Command::ToggleShuffle => {
                            let rt = tokio_rt.clone();
                            let s = audio_state.clone();
                            rt.spawn(async move {
                                let mut ps = s.playback_state.lock().await;
                                ps.shuffle = !ps.shuffle;
                            });
                        }
                        Command::ToggleRepeat => {
                            let rt = tokio_rt.clone();
                            let s = audio_state.clone();
                            rt.spawn(async move {
                                let mut ps = s.playback_state.lock().await;
                                ps.repeat_mode = match ps.repeat_mode {
                                    RepeatMode::Off => RepeatMode::All,
                                    RepeatMode::All => RepeatMode::One,
                                    RepeatMode::One => RepeatMode::Off,
                                };
                            });
                        }
                        _ => {}
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    // Periodic Tick (200ms)
                    if current_track_active {
                        let cur_pos = player.get_pos().as_secs_f32();
                        let is_empty = player.empty();

                        if is_empty {
                            current_track_active = false;
                            if cur_pos > 1.0 {
                                let rt = tokio_rt.clone();
                                let s = audio_state.clone();
                                let bridge = s.command_tx.clone();
                                rt.spawn(async move {
                                    let _ = bridge.send(Command::Next).await;
                                });
                            }
                        } else {
                            let rt = tokio_rt.clone();
                            let s = audio_state.clone();
                            rt.spawn(async move {
                                let mut ps = s.playback_state.lock().await;
                                ps.position_secs = cur_pos;
                            });
                        }
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });

    // Axum Web & API Routes
    let web_dir = if let Ok(custom) = std::env::var("ARCY_WEB_DIR") {
        std::path::PathBuf::from(custom)
    } else if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        std::path::PathBuf::from(manifest_dir)
            .parent()
            .map(|p| p.join("arcy-music-gui-web/web"))
            .unwrap_or_else(|| std::path::PathBuf::from("arcy-music-gui-web/web"))
    } else {
        let candidates = [
            std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("web"))),
            std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("../share/arcy-music/web"))),
            dirs::data_dir().map(|d| d.join("arcy-music/web")),
            Some(std::path::PathBuf::from("/usr/share/arcy-music/web")),
            Some(std::path::PathBuf::from("/usr/local/share/arcy-music/web")),
            Some(std::path::PathBuf::from("arcy-music-gui-web/web")),
            Some(std::path::PathBuf::from("web")),
        ];
        candidates
            .into_iter()
            .flatten()
            .find(|p| p.exists())
            .unwrap_or_else(|| std::path::PathBuf::from("arcy-music-gui-web/web"))
    };

    let thumbnails_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("arcy-music/thumbnails");
    let _ = std::fs::create_dir_all(&thumbnails_dir);

    let app = Router::new()
        .route("/api/state", get(get_state))
        .route("/api/command", post(post_command))
        .route("/api/library", get(api::get_library))
        .route("/api/search/youtube/{query}", get(api::search_youtube))
        .route("/api/search/local/{query}", get(api::search_local))
        .route("/api/settings/{key}", get(api::get_setting))
        .route("/api/settings", post(api::set_setting))
        .route("/api/dsp/eq", get(api::get_eq).post(api::set_eq))
        .route("/api/lyrics/{track_id}", get(api::get_lyrics))
        .route("/api/theme", get(api::get_theme).post(api::set_theme))
        .route("/api/playlists", get(api::get_playlists).post(api::create_playlist))
        .route("/api/playlists/{id}", delete(api::delete_playlist))
        .route("/api/playlists/{id}/tracks", get(api::get_playlist_tracks).post(api::add_track_to_playlist))
        .route("/api/playlists/{id}/tracks/{track_id}", delete(api::remove_track_from_playlist))
        .route("/api/playlists/{id}/tracks/{track_id}/move_up", post(api::move_playlist_track_up))
        .route("/api/playlists/{id}/tracks/{track_id}/move_down", post(api::move_playlist_track_down))
        .route("/api/playlists/{id}/tracks/reorder", post(api::reorder_playlist_tracks))
        .route("/api/playlists/{id}/pin", post(api::toggle_pin_playlist))
        .route("/api/playlists/{id}/move_up", post(api::move_playlist_up))
        .route("/api/playlists/{id}/move_down", post(api::move_playlist_down))
        .route("/api/spotify/sync", post(api::spotify_sync))
        .route("/api/spotify/status", get(api::get_spotify_status))
        .route("/api/spotify/import-browser", post(api::spotify_import_browser))
        .route("/api/spotify/launch-login", post(api::spotify_launch_login))
        .route("/api/spotify/login-status", get(api::spotify_login_status))
        .route("/api/search/spotify/{query}", get(api::search_spotify))
        .route("/api/audio/devices", get(get_audio_devices))
        .route("/api/audio/devices/select", post(select_audio_device))
        .route("/api/gui/switch-to-native", post(api::switch_to_native_gui))
        .route("/api/gui/switch-to-web", post(api::switch_to_web_gui))
        .route("/api/daemon/shutdown", post(daemon_shutdown))
        .nest_service("/web", tower_http::services::ServeDir::new(&web_dir))
        .nest_service("/thumbnails", tower_http::services::ServeDir::new(&thumbnails_dir))
        .fallback_service(tower_http::services::ServeDir::new(&web_dir))
        .layer(CorsLayer::permissive())
        .with_state(shared_state);

    let port: u16 = std::env::var("ARCY_MUSIC_PORT")
        .or_else(|_| std::env::var("ARCY_PORT"))
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(13338);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    println!("Arcy Music Daemon listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn get_state(State(state): State<Arc<AppState>>) -> Json<PlaybackState> {
    let ps = state.playback_state.lock().await;
    Json(ps.clone())
}

async fn daemon_shutdown() -> Json<bool> {
    tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        std::process::exit(0);
    });
    Json(true)
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct AudioDevice {
    pub name: String,
    pub description: String,
    pub is_default: bool,
    pub is_muted: bool,
}

async fn get_audio_devices() -> Json<Vec<AudioDevice>> {
    let mut devices = Vec::new();
    let default_sink = std::process::Command::new("pactl")
        .arg("get-default-sink")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    if let Ok(output) = std::process::Command::new("pactl").args(["-f", "json", "list", "sinks"]).output() {
        if let Ok(json_val) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
            if let Some(arr) = json_val.as_array() {
                for item in arr {
                    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    if name.is_empty() || name == "ArcySilentSink" {
                        continue;
                    }
                    let description = item.get("description").and_then(|v| v.as_str()).unwrap_or(&name).to_string();
                    let is_muted = item.get("mute").and_then(|v| v.as_bool()).unwrap_or(false);
                    let is_default = name == default_sink;
                    devices.push(AudioDevice {
                        name,
                        description,
                        is_default,
                        is_muted,
                    });
                }
            }
        }
    }
    Json(devices)
}

#[derive(serde::Deserialize)]
pub struct SelectAudioDeviceRequest {
    pub name: String,
}

async fn select_audio_device(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SelectAudioDeviceRequest>,
) -> Json<bool> {
    if req.name.is_empty() {
        return Json(false);
    }
    // 1. Set default sink in pactl
    let res = std::process::Command::new("pactl")
        .args(["set-default-sink", &req.name])
        .status();

    // 2. Move any active sink inputs to this sink
    if let Ok(inputs_out) = std::process::Command::new("pactl").args(["-f", "json", "list", "sink-inputs"]).output() {
        if let Ok(json_val) = serde_json::from_slice::<serde_json::Value>(&inputs_out.stdout) {
            if let Some(arr) = json_val.as_array() {
                for item in arr {
                    if let Some(idx) = item.get("index").and_then(|v| v.as_u64()) {
                        let _ = std::process::Command::new("pactl")
                            .args(["move-sink-input", &idx.to_string(), &req.name])
                            .status();
                    }
                }
            }
        }
    }

    // 3. Persist in settings
    let conn = state.db.lock().await;
    let _ = conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('audio_sink', ?1)",
        [&req.name],
    );

    Json(res.map(|s| s.success()).unwrap_or(false))
}



async fn post_command(State(state): State<Arc<AppState>>, Json(cmd): Json<Command>) -> Json<bool> {
    // HTTP layer debounce for PlayTrack / PlayTrackId
    static LAST_PLAY_CMD: std::sync::Mutex<Option<(String, std::time::Instant)>> = std::sync::Mutex::new(None);
    let requested_id = match &cmd {
        Command::PlayTrackId(id) => Some(id.clone()),
        Command::PlayTrack(t) => Some(t.id.clone()),
        _ => None,
    };
    if let Some(req_id) = requested_id {
        if let Ok(mut last) = LAST_PLAY_CMD.lock() {
            if let Some((ref prev_id, ref prev_time)) = *last {
                if prev_id == &req_id && prev_time.elapsed() < Duration::from_millis(3500) {
                    println!("HTTP post_command: Debounced duplicate play request for track '{}' ({}ms ago)", req_id, prev_time.elapsed().as_millis());
                    return Json(true);
                }
            }
            *last = Some((req_id, std::time::Instant::now()));
        }
    }

    let final_cmd = match cmd {
        Command::PlayTrackId(id) => {
            let conn = state.db.lock().await;
            let mut stmt = conn.prepare("SELECT title, artist, album, duration_secs, source_type, source_data, thumbnail_url FROM tracks WHERE id = ?").unwrap();
            let track_opt = stmt.query_row([&id], |row| {
                let source_type: String = row.get(4)?;
                let source_data: String = row.get(5)?;
                let source = if source_type == "local" {
                    TrackSource::Local { path: source_data }
                } else if source_type == "spotify" {
                    TrackSource::Spotify { track_id: source_data }
                } else {
                    TrackSource::YouTube { video_id: source_data }
                };
                let duration_secs: Option<i64> = row.get(3)?;
                Ok(Track {
                    id: id.clone(),
                    title: row.get(0)?,
                    artist: row.get(1)?,
                    album: row.get(2)?,
                    duration_secs: duration_secs.map(|d| d as u64),
                    source,
                    thumbnail_url: row.get(6)?,
                })
            }).ok();

            if let Some(track) = track_opt {
                Command::PlayTrack(track)
            } else {
                return Json(false);
            }
        }
        Command::SeekRelative(delta) => {
            let mut ps = state.playback_state.lock().await;
            let new_pos = (ps.position_secs + delta).clamp(0.0, ps.duration_secs.max(0.0));
            ps.position_secs = new_pos;
            Command::Seek(new_pos)
        }
        Command::SetQueueAndPlay { tracks, start_index } => {
            if tracks.is_empty() {
                return Json(false);
            }
            let idx = start_index.min(tracks.len() - 1);
            let track_to_play = tracks[idx].clone();
            {
                let mut ps = state.playback_state.lock().await;
                ps.queue = tracks;
                ps.queue_index = idx;
            }
            let _ = state.command_tx.send(Command::PlayTrack(track_to_play)).await;
            return Json(true);
        }
        Command::PlayPlaylist { playlist_id, start_index } => {
            let conn = state.db.lock().await;
            let tracks = if playlist_id == "local" {
                let mut stmt = match conn.prepare("SELECT id, title, artist, album, duration_secs, source_type, source_data, thumbnail_url FROM tracks WHERE source_type = 'local' ORDER BY title COLLATE NOCASE ASC") {
                    Ok(s) => s,
                    Err(_) => return Json(false),
                };
                let iter = stmt.query_map([], |row| {
                    let duration_secs: Option<i64> = row.get(4)?;
                    Ok(Track {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        artist: row.get(2)?,
                        album: row.get(3)?,
                        duration_secs: duration_secs.map(|d| d as u64),
                        source: TrackSource::Local { path: row.get(6)? },
                        thumbnail_url: row.get(7)?,
                    })
                });
                iter.map(|it| it.flatten().collect::<Vec<_>>()).unwrap_or_default()
            } else {
                let mut stmt = match conn.prepare("SELECT t.id, t.title, t.artist, t.album, t.duration_secs, t.source_type, t.source_data, t.thumbnail_url FROM playlist_tracks pt JOIN tracks t ON pt.track_id = t.id WHERE pt.playlist_id = ? ORDER BY pt.position ASC") {
                    Ok(s) => s,
                    Err(_) => return Json(false),
                };
                let iter = stmt.query_map([&playlist_id], |row| {
                    let source_type: String = row.get(5)?;
                    let source_data: String = row.get(6)?;
                    let source = if source_type == "local" {
                        TrackSource::Local { path: source_data }
                    } else if source_type == "spotify" {
                        TrackSource::Spotify { track_id: source_data }
                    } else {
                        TrackSource::YouTube { video_id: source_data }
                    };
                    let duration_secs: Option<i64> = row.get(4)?;
                    Ok(Track {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        artist: row.get(2)?,
                        album: row.get(3)?,
                        duration_secs: duration_secs.map(|d| d as u64),
                        source,
                        thumbnail_url: row.get(7)?,
                    })
                });
                iter.map(|it| it.flatten().collect::<Vec<_>>()).unwrap_or_default()
            };

            if tracks.is_empty() {
                return Json(false);
            }

            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
            if playlist_id == "local" {
                let _ = conn.execute("INSERT INTO settings (key, value) VALUES ('playlist_last_played_local', ?1) ON CONFLICT(key) DO UPDATE SET value = ?1", [now.to_string()]);
            } else {
                let _ = conn.execute("UPDATE playlists SET last_played = ?1 WHERE id = ?2", rusqlite::params![now, playlist_id]);
            }
            drop(conn);

            let idx = start_index.min(tracks.len() - 1);
            let track_to_play = tracks[idx].clone();
            {
                let mut ps = state.playback_state.lock().await;
                ps.queue = tracks;
                ps.queue_index = idx;
            }
            let _ = state.command_tx.send(Command::PlayTrack(track_to_play)).await;
            return Json(true);
        }
        Command::EnqueueTrack(ref track) => {
            let mut ps = state.playback_state.lock().await;
            ps.queue.push(track.clone());
            let is_next = ps.queue.len() == ps.queue_index + 2;
            let track_to_prefetch = if is_next { Some(track.clone()) } else { None };
            drop(ps);
            if let Some(t) = track_to_prefetch {
                let cache = state.stream_cache.clone();
                tokio::task::spawn_blocking(move || {
                    prefetch_stream(&t, &cache);
                });
            }
            return Json(true);
        }
        Command::ClearQueue => {
            let mut ps = state.playback_state.lock().await;
            ps.queue.clear();
            ps.queue_index = 0;
            return Json(true);
        }
        Command::RemoveFromQueue(idx) => {
            let mut ps = state.playback_state.lock().await;
            if idx < ps.queue.len() {
                ps.queue.remove(idx);
                if ps.queue_index >= ps.queue.len() {
                    ps.queue_index = ps.queue.len().saturating_sub(1);
                }
            }
            return Json(true);
        }
        Command::DownloadTrack(track) => {
            let state_clone = state.clone();
            tokio::spawn(async move {
                let mut download_dir = dirs::download_dir().unwrap_or_else(|| dirs::home_dir().unwrap().join("Downloads"));
                {
                    let conn = state_clone.db.lock().await;
                    if let Ok(path) = conn.query_row("SELECT value FROM settings WHERE key = 'download_path'", [], |r| r.get::<_, String>(0)) {
                        download_dir = std::path::PathBuf::from(path);
                    }
                }
                match &track.source {
                    TrackSource::YouTube { video_id } => {
                        if !arcy_music_core::is_valid_youtube_id(video_id) {
                            eprintln!("Security: Blocked invalid YouTube ID for download: {}", video_id);
                            return;
                        }
                        println!("Downloading YouTube track to: {:?}", download_dir);
                        let _ = tokio::process::Command::new("yt-dlp")
                            .arg("-x")
                            .arg("--audio-format")
                            .arg("mp3")
                            .arg("-P")
                            .arg(&download_dir)
                            .arg("--")
                            .arg(format!("https://www.youtube.com/watch?v={}", video_id))
                            .status()
                            .await;
                    }
                    TrackSource::Spotify { .. } => {
                        let query = format!("ytsearch1:{} {}", track.title, track.artist.as_deref().unwrap_or(""));
                        println!("Downloading Spotify track ({}) to: {:?}", query, download_dir);
                        let _ = tokio::process::Command::new("yt-dlp")
                            .arg("-x")
                            .arg("--audio-format")
                            .arg("mp3")
                            .arg("-P")
                            .arg(&download_dir)
                            .arg("--")
                            .arg(&query)
                            .status()
                            .await;
                    }
                    _ => {}
                }
            });
            return Json(true);
        }
        _ => cmd,
    };

    if state.command_tx.send(final_cmd).await.is_ok() {
        Json(true)
    } else {
        Json(false)
    }
}
