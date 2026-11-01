use axum::{extract::{Path, State}, Json};
use std::sync::Arc;
use crate::AppState;
use arcy_music_core::{Command, EqConfig, Playlist, Track, TrackSource};
use crate::lyrics::{fetch_lyrics, TrackLyrics};
use crate::spotify::{self, SpotifySyncRequest, SpotifySyncResponse};

pub async fn get_library(State(state): State<Arc<AppState>>) -> Json<Vec<Track>> {
    let conn = state.db.lock().await;
    let mut stmt = match conn.prepare("SELECT id, title, artist, album, duration_secs, source_type, source_data, thumbnail_url FROM tracks ORDER BY title COLLATE NOCASE ASC") {
        Ok(s) => s,
        Err(_) => return Json(Vec::new()),
    };

    let track_iter = stmt.query_map([], |row| {
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

    let mut tracks = Vec::new();
    if let Ok(iter) = track_iter {
        for track in iter.flatten() {
            tracks.push(track);
        }
    }
    
    Json(tracks)
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct SettingRequest {
    pub key: String,
    pub value: String,
}

pub async fn get_setting(State(state): State<Arc<AppState>>, Path(key): Path<String>) -> Json<Option<String>> {
    let conn = state.db.lock().await;
    let value: Result<String, _> = conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        [&key],
        |row| row.get(0),
    );
    Json(value.ok())
}

pub async fn set_setting(State(state): State<Arc<AppState>>, Json(req): Json<SettingRequest>) -> Json<bool> {
    let conn = state.db.lock().await;
    let success = conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
        [&req.key, &req.value],
    ).is_ok();
    Json(success)
}

use crate::plugins::YouTubeFetcher;
use arcy_music_core::fetcher::MusicFetcher;

pub async fn search_youtube(Path(query): Path<String>) -> Json<Vec<Track>> {
    let fetcher = YouTubeFetcher::new();
    let res = fetcher.search(&query).await.unwrap_or_default();
    Json(res)
}

pub async fn search_local(State(state): State<Arc<AppState>>, Path(query): Path<String>) -> Json<Vec<Track>> {
    let conn = state.db.lock().await;
    let mut stmt = match conn.prepare("SELECT id, title, artist, album, duration_secs, source_type, source_data, thumbnail_url FROM tracks WHERE title LIKE ?1 OR artist LIKE ?1 LIMIT 50") {
        Ok(s) => s,
        Err(_) => return Json(Vec::new()),
    };

    let query_param = format!("%{}%", query);
    let track_iter = stmt.query_map([&query_param], |row| {
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

    let mut tracks = Vec::new();
    if let Ok(iter) = track_iter {
        for track in iter.flatten() {
            tracks.push(track);
        }
    }
    
    Json(tracks)
}

pub async fn get_eq(State(state): State<Arc<AppState>>) -> Json<EqConfig> {
    let cfg = state.eq_config.read().map(|c| c.clone()).unwrap_or_default();
    Json(cfg)
}

pub async fn set_eq(State(state): State<Arc<AppState>>, Json(cfg): Json<EqConfig>) -> Json<bool> {
    if let Ok(mut lock) = state.eq_config.write() {
        *lock = cfg.clone();
    }
    let _ = state.command_tx.send(Command::SetEq(cfg.clone())).await;

    // Persist to database
    let conn = state.db.lock().await;
    if let Ok(json_str) = serde_json::to_string(&cfg) {
        let _ = conn.execute(
            "INSERT INTO settings (key, value) VALUES ('eq_config', ?1) ON CONFLICT(key) DO UPDATE SET value = ?1",
            [&json_str],
        );
    }
    Json(true)
}

pub async fn get_lyrics(State(state): State<Arc<AppState>>, Path(track_id): Path<String>) -> Json<Option<TrackLyrics>> {
    // 1. Check local SQLite cache first
    {
        let conn = state.db.lock().await;
        let cached = conn.query_row(
            "SELECT plain_lyrics, synced_lyrics FROM lyrics WHERE track_id = ?1",
            [&track_id],
            |row| {
                let plain: Option<String> = row.get(0)?;
                let synced_raw: Option<String> = row.get(1)?;
                let synced = synced_raw
                    .as_deref()
                    .and_then(|s| serde_json::from_str::<Vec<crate::lyrics::LyricLine>>(s).ok())
                    .unwrap_or_default();
                Ok(TrackLyrics {
                    track_id: track_id.clone(),
                    plain_lyrics: plain,
                    synced_lyrics: synced,
                })
            },
        );
        if let Ok(res) = cached {
            return Json(Some(res));
        }
    }

    // 2. Fetch track metadata from tracks table
    let track_meta: Option<(String, String)> = {
        let conn = state.db.lock().await;
        conn.query_row(
            "SELECT title, artist FROM tracks WHERE id = ?1",
            [&track_id],
            |row| Ok((row.get(0)?, row.get::<_, Option<String>>(1)?.unwrap_or_default())),
        ).ok()
    };

    if let Some((title, artist)) = track_meta {
        if let Some(mut lyrics) = fetch_lyrics(&title, &artist).await {
            lyrics.track_id = track_id.clone();
            
            // Cache to SQLite
            let conn = state.db.lock().await;
            let synced_json = serde_json::to_string(&lyrics.synced_lyrics).unwrap_or_default();
            let _ = conn.execute(
                "INSERT INTO lyrics (track_id, plain_lyrics, synced_lyrics) VALUES (?1, ?2, ?3) ON CONFLICT(track_id) DO UPDATE SET plain_lyrics = ?2, synced_lyrics = ?3",
                (&track_id, &lyrics.plain_lyrics, &synced_json),
            );
            return Json(Some(lyrics));
        }
    }

    Json(None)
}

#[derive(serde::Deserialize)]
pub struct SetThemeRequest {
    pub theme: String,
}

pub async fn get_theme(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let current_name = state.active_theme.read().await.clone();
    let current_colors = arcy_music_core::theme::ThemeColors::get_by_name(&current_name);
    let all = arcy_music_core::theme::ThemeColors::all();
    Json(serde_json::json!({
        "current": current_name,
        "colors": current_colors,
        "available": all,
    }))
}

pub async fn set_theme(State(state): State<Arc<AppState>>, Json(req): Json<SetThemeRequest>) -> Json<bool> {
    *state.active_theme.write().await = req.theme;
    Json(true)
}

#[derive(serde::Deserialize)]
pub struct CreatePlaylistRequest {
    pub name: String,
}

#[derive(serde::Deserialize)]
pub struct AddTrackRequest {
    pub track_id: String,
}

pub async fn get_playlists(State(state): State<Arc<AppState>>) -> Json<Vec<Playlist>> {
    let conn = state.db.lock().await;

    let local_pinned: bool = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'playlist_pin_local'",
            [],
            |r| r.get::<_, String>(0),
        )
        .map(|v| v == "1")
        .unwrap_or(false);

    let local_count: usize = conn
        .query_row(
            "SELECT COUNT(*) FROM tracks WHERE source_type = 'local'",
            [],
            |r| r.get::<_, i64>(0).map(|c| c as usize),
        )
        .unwrap_or(0);

    let local_last_played: i64 = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'playlist_last_played_local'",
            [],
            |r| r.get::<_, String>(0),
        )
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let mut playlists = Vec::new();
    playlists.push(Playlist {
        id: "local".to_string(),
        name: "Local Music".to_string(),
        track_count: local_count,
        is_pinned: local_pinned,
        last_played: local_last_played,
        sort_order: 0,
        is_default: true,
    });

    if let Ok(mut stmt) = conn.prepare(
        "SELECT p.id, p.name, p.is_pinned, p.sort_order, p.last_played, COUNT(pt.track_id)
         FROM playlists p
         LEFT JOIN playlist_tracks pt ON p.id = pt.playlist_id
         GROUP BY p.id
         ORDER BY p.is_pinned DESC, p.last_played DESC, p.sort_order ASC"
    ) {
        if let Ok(rows) = stmt.query_map([], |row| {
            let is_pinned: i32 = row.get(2)?;
            let count: i64 = row.get(5)?;
            Ok(Playlist {
                id: row.get(0)?,
                name: row.get(1)?,
                is_pinned: is_pinned != 0,
                sort_order: row.get(3)?,
                last_played: row.get(4)?,
                track_count: count as usize,
                is_default: false,
            })
        }) {
            for pl in rows.flatten() {
                playlists.push(pl);
            }
        }
    }

    // Pinned playlists always appear first, followed by recently listened (last_played desc), then sort order
    playlists.sort_by(|a, b| {
        b.is_pinned.cmp(&a.is_pinned)
            .then_with(|| b.last_played.cmp(&a.last_played))
            .then_with(|| a.sort_order.cmp(&b.sort_order))
    });

    Json(playlists)
}

pub async fn create_playlist(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreatePlaylistRequest>,
) -> Json<Option<Playlist>> {
    let name = req.name.trim();
    if name.is_empty() {
        return Json(None);
    }
    let conn = state.db.lock().await;
    let id = format!("pl_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis());
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    
    let max_order: i32 = conn.query_row("SELECT COALESCE(MAX(sort_order), 0) FROM playlists", [], |r| r.get(0)).unwrap_or(0);

    let res = conn.execute(
        "INSERT INTO playlists (id, name, is_pinned, sort_order, last_played) VALUES (?1, ?2, 0, ?3, ?4)",
        rusqlite::params![id, name, max_order + 1, now],
    );

    if res.is_ok() {
        Json(Some(Playlist {
            id,
            name: name.to_string(),
            track_count: 0,
            is_pinned: false,
            last_played: now,
            sort_order: max_order + 1,
            is_default: false,
        }))
    } else {
        Json(None)
    }
}

pub async fn delete_playlist(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<bool> {
    if id == "local" {
        return Json(false); // Local music playlist cannot be deleted
    }
    let conn = state.db.lock().await;
    let _ = conn.execute("DELETE FROM playlist_tracks WHERE playlist_id = ?1", [&id]);
    let ok = conn.execute("DELETE FROM playlists WHERE id = ?1", [&id]).is_ok();
    Json(ok)
}

pub async fn toggle_pin_playlist(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<bool> {
    let conn = state.db.lock().await;
    if id == "local" {
        let current: String = conn.query_row(
            "SELECT value FROM settings WHERE key = 'playlist_pin_local'",
            [],
            |r| r.get(0),
        ).unwrap_or_else(|_| "0".to_string());
        let new_val = if current == "1" { "0" } else { "1" };
        let ok = conn.execute(
            "INSERT INTO settings (key, value) VALUES ('playlist_pin_local', ?1) ON CONFLICT(key) DO UPDATE SET value = ?1",
            [new_val],
        ).is_ok();
        return Json(ok);
    }
    let ok = conn.execute(
        "UPDATE playlists SET is_pinned = 1 - is_pinned WHERE id = ?1",
        [&id],
    ).is_ok();
    Json(ok)
}

pub async fn move_playlist_up(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<bool> {
    let conn = state.db.lock().await;
    let ok = conn.execute(
        "UPDATE playlists SET sort_order = sort_order - 1 WHERE id = ?1",
        [&id],
    ).is_ok();
    Json(ok)
}

pub async fn move_playlist_down(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<bool> {
    let conn = state.db.lock().await;
    let ok = conn.execute(
        "UPDATE playlists SET sort_order = sort_order + 1 WHERE id = ?1",
        [&id],
    ).is_ok();
    Json(ok)
}

pub async fn get_playlist_tracks(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<Vec<Track>> {
    let conn = state.db.lock().await;
    if id == "local" {
        let mut stmt = match conn.prepare("SELECT id, title, artist, album, duration_secs, source_type, source_data, thumbnail_url FROM tracks WHERE source_type = 'local' ORDER BY title COLLATE NOCASE ASC") {
            Ok(s) => s,
            Err(_) => return Json(Vec::new()),
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
        return Json(iter.map(|it| it.flatten().collect()).unwrap_or_default());
    }

    let mut stmt = match conn.prepare(
        "SELECT t.id, t.title, t.artist, t.album, t.duration_secs, t.source_type, t.source_data, t.thumbnail_url 
         FROM playlist_tracks pt 
         JOIN tracks t ON pt.track_id = t.id 
         WHERE pt.playlist_id = ? 
         ORDER BY pt.position ASC"
    ) {
        Ok(s) => s,
        Err(_) => return Json(Vec::new()),
    };

    let iter = stmt.query_map([&id], |row| {
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

    Json(iter.map(|it| it.flatten().collect()).unwrap_or_default())
}

pub async fn add_track_to_playlist(
    State(state): State<Arc<AppState>>,
    Path(playlist_id): Path<String>,
    Json(req): Json<AddTrackRequest>,
) -> Json<bool> {
    if playlist_id == "local" {
        return Json(false);
    }
    let conn = state.db.lock().await;
    let max_pos: i32 = conn.query_row(
        "SELECT COALESCE(MAX(position), 0) FROM playlist_tracks WHERE playlist_id = ?1",
        [&playlist_id],
        |r| r.get(0),
    ).unwrap_or(0);

    let ok = conn.execute(
        "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?1, ?2, ?3) ON CONFLICT(playlist_id, track_id) DO NOTHING",
        rusqlite::params![playlist_id, req.track_id, max_pos + 1],
    ).is_ok();
    Json(ok)
}

pub async fn remove_track_from_playlist(
    State(state): State<Arc<AppState>>,
    Path((playlist_id, track_id)): Path<(String, String)>,
) -> Json<bool> {
    if playlist_id == "local" {
        return Json(false);
    }
    let conn = state.db.lock().await;
    let ok = conn.execute(
        "DELETE FROM playlist_tracks WHERE playlist_id = ?1 AND track_id = ?2",
        [&playlist_id, &track_id],
    ).is_ok();
    Json(ok)
}

pub async fn move_playlist_track_up(
    State(state): State<Arc<AppState>>,
    Path((playlist_id, track_id)): Path<(String, String)>,
) -> Json<bool> {
    if playlist_id == "local" {
        return Json(false);
    }
    let conn = state.db.lock().await;
    let cur_pos: i32 = match conn.query_row(
        "SELECT position FROM playlist_tracks WHERE playlist_id = ?1 AND track_id = ?2",
        [&playlist_id, &track_id],
        |r| r.get(0),
    ) {
        Ok(p) => p,
        Err(_) => return Json(false),
    };

    let prev: Option<(String, i32)> = conn.query_row(
        "SELECT track_id, position FROM playlist_tracks WHERE playlist_id = ?1 AND position < ?2 ORDER BY position DESC LIMIT 1",
        rusqlite::params![&playlist_id, cur_pos],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ).ok();

    if let Some((prev_id, prev_pos)) = prev {
        let _ = conn.execute(
            "UPDATE playlist_tracks SET position = ?1 WHERE playlist_id = ?2 AND track_id = ?3",
            rusqlite::params![prev_pos, &playlist_id, &track_id],
        );
        let _ = conn.execute(
            "UPDATE playlist_tracks SET position = ?1 WHERE playlist_id = ?2 AND track_id = ?3",
            rusqlite::params![cur_pos, &playlist_id, &prev_id],
        );
        return Json(true);
    }
    Json(false)
}

pub async fn move_playlist_track_down(
    State(state): State<Arc<AppState>>,
    Path((playlist_id, track_id)): Path<(String, String)>,
) -> Json<bool> {
    if playlist_id == "local" {
        return Json(false);
    }
    let conn = state.db.lock().await;
    let cur_pos: i32 = match conn.query_row(
        "SELECT position FROM playlist_tracks WHERE playlist_id = ?1 AND track_id = ?2",
        [&playlist_id, &track_id],
        |r| r.get(0),
    ) {
        Ok(p) => p,
        Err(_) => return Json(false),
    };

    let next: Option<(String, i32)> = conn.query_row(
        "SELECT track_id, position FROM playlist_tracks WHERE playlist_id = ?1 AND position > ?2 ORDER BY position ASC LIMIT 1",
        rusqlite::params![&playlist_id, cur_pos],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ).ok();

    if let Some((next_id, next_pos)) = next {
        let _ = conn.execute(
            "UPDATE playlist_tracks SET position = ?1 WHERE playlist_id = ?2 AND track_id = ?3",
            rusqlite::params![next_pos, &playlist_id, &track_id],
        );
        let _ = conn.execute(
            "UPDATE playlist_tracks SET position = ?1 WHERE playlist_id = ?2 AND track_id = ?3",
            rusqlite::params![cur_pos, &playlist_id, &next_id],
        );
        return Json(true);
    }
    Json(false)
}

pub async fn reorder_playlist_tracks(
    State(state): State<Arc<AppState>>,
    Path(playlist_id): Path<String>,
    Json(track_ids): Json<Vec<String>>,
) -> Json<bool> {
    if playlist_id == "local" {
        return Json(false);
    }
    let mut conn = state.db.lock().await;
    let tx = match conn.transaction() {
        Ok(t) => t,
        Err(_) => return Json(false),
    };
    for (pos, tid) in track_ids.iter().enumerate() {
        let _ = tx.execute(
            "UPDATE playlist_tracks SET position = ?1 WHERE playlist_id = ?2 AND track_id = ?3",
            rusqlite::params![pos as i32, &playlist_id, tid],
        );
    }
    Json(tx.commit().is_ok())
}

pub async fn spotify_sync(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SpotifySyncRequest>,
) -> Json<SpotifySyncResponse> {
    let sp_dc_param = req.sp_dc.as_deref().unwrap_or("").trim();
    if !sp_dc_param.is_empty() {
        let conn = state.db.lock().await;
        let _ = conn.execute(
            "INSERT INTO settings (key, value) VALUES ('spotify_sp_dc', ?1) ON CONFLICT(key) DO UPDATE SET value = ?1",
            [sp_dc_param],
        );
    }

    // 1. High-Performance Pathfinder GraphQL CDP sync via helper
    let helper_args = if !sp_dc_param.is_empty() {
        vec!["sync", sp_dc_param]
    } else {
        vec!["sync"]
    };

    if let Some(res) = run_spotify_helper_args(&helper_args) {
        if res.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
            let pl_count = res.get("synced_playlists").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let tr_count = res.get("synced_tracks").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let msg = res.get("message").and_then(|v| v.as_str()).unwrap_or("Synced successfully").to_string();

            return Json(SpotifySyncResponse {
                success: true,
                synced_playlists: pl_count,
                synced_tracks: tr_count,
                message: msg,
            });
        } else {
            let err = res.get("message").and_then(|v| v.as_str()).unwrap_or("Spotify sync failed").to_string();
            // If user did not supply a direct BQ bearer token, return the error
            if req.access_token.as_ref().map(|t| !t.trim().starts_with("BQ")).unwrap_or(true) {
                return Json(SpotifySyncResponse {
                    success: false,
                    synced_playlists: 0,
                    synced_tracks: 0,
                    message: err,
                });
            }
        }
    }

    // 2. Direct Bearer access token fallback if provided
    let token = if let Some(tok) = req.access_token.filter(|t| !t.trim().is_empty()) {
        tok
    } else {
        return Json(SpotifySyncResponse {
            success: false,
            synced_playlists: 0,
            synced_tracks: 0,
            message: "Please log in to Spotify or enter your Spotify credentials".into(),
        });
    };

    let mut total_tracks = 0;
    let mut synced_playlists_count = 0;

    // 1. Fetch Liked Songs
    if let Ok(liked) = spotify::fetch_liked_songs(&token).await {
        if !liked.is_empty() {
            let conn = state.db.lock().await;
            let _ = conn.execute(
                "INSERT INTO playlists (id, name, is_pinned, sort_order) VALUES ('spotify_liked', 'Spotify Liked Songs', 1, -1) ON CONFLICT(id) DO NOTHING",
                [],
            );
            for (pos, tr) in liked.iter().enumerate() {
                let track_source_str = "spotify";
                let track_source_data = match &tr.source {
                    TrackSource::Spotify { track_id } => track_id.clone(),
                    _ => tr.id.clone(),
                };
                let _ = conn.execute(
                    "INSERT INTO tracks (id, title, artist, album, duration_secs, source_type, source_data, thumbnail_url)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT(id) DO UPDATE SET title=?2, artist=?3, album=?4, duration_secs=?5, thumbnail_url=?8",
                    rusqlite::params![
                        tr.id,
                        tr.title,
                        tr.artist,
                        tr.album,
                        tr.duration_secs.map(|d| d as i64),
                        track_source_str,
                        track_source_data,
                        tr.thumbnail_url,
                    ],
                );
                let _ = conn.execute(
                    "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES ('spotify_liked', ?1, ?2) ON CONFLICT(playlist_id, track_id) DO UPDATE SET position = ?2",
                    rusqlite::params![tr.id, pos as i64],
                );
            }
            total_tracks += liked.len();
            synced_playlists_count += 1;
        }
    }

    // 2. Fetch User Playlists
    if let Ok(user_playlists) = spotify::fetch_user_playlists(&token).await {
        for (pl_id, pl_name, _) in user_playlists {
            let db_pl_id = format!("sp_pl_{}", pl_id);
            if let Ok(tracks) = spotify::fetch_playlist_tracks(&token, &pl_id).await {
                let conn = state.db.lock().await;
                let _ = conn.execute(
                    "INSERT INTO playlists (id, name, is_pinned, sort_order) VALUES (?1, ?2, 0, 0) ON CONFLICT(id) DO UPDATE SET name = ?2",
                    rusqlite::params![db_pl_id, pl_name],
                );
                for (pos, tr) in tracks.iter().enumerate() {
                    let track_source_str = "spotify";
                    let track_source_data = match &tr.source {
                        TrackSource::Spotify { track_id } => track_id.clone(),
                        _ => tr.id.clone(),
                    };
                    let _ = conn.execute(
                        "INSERT INTO tracks (id, title, artist, album, duration_secs, source_type, source_data, thumbnail_url)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                         ON CONFLICT(id) DO UPDATE SET title=?2, artist=?3, album=?4, duration_secs=?5, thumbnail_url=?8",
                        rusqlite::params![
                            tr.id,
                            tr.title,
                            tr.artist,
                            tr.album,
                            tr.duration_secs.map(|d| d as i64),
                            track_source_str,
                            track_source_data,
                            tr.thumbnail_url,
                        ],
                    );
                    let _ = conn.execute(
                        "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?1, ?2, ?3) ON CONFLICT(playlist_id, track_id) DO UPDATE SET position = ?3",
                        rusqlite::params![db_pl_id, tr.id, pos as i64],
                    );
                }
                total_tracks += tracks.len();
                synced_playlists_count += 1;
            }
        }
    }

    Json(SpotifySyncResponse {
        success: true,
        synced_playlists: synced_playlists_count,
        synced_tracks: total_tracks,
        message: format!("Successfully synced {} playlists and {} tracks!", synced_playlists_count, total_tracks),
    })
}

#[derive(serde::Serialize)]
pub struct SpotifyStatusResponse {
    pub configured: bool,
    pub has_token: bool,
    pub sp_dc: Option<String>,
}

pub async fn get_spotify_status(State(state): State<Arc<AppState>>) -> Json<SpotifyStatusResponse> {
    let conn = state.db.lock().await;
    let sp_dc: Option<String> = conn.query_row(
        "SELECT value FROM settings WHERE key = 'spotify_sp_dc'",
        [],
        |r| r.get(0),
    ).ok();
    let token: Option<String> = conn.query_row(
        "SELECT value FROM settings WHERE key = 'spotify_token'",
        [],
        |r| r.get(0),
    ).ok();

    Json(SpotifyStatusResponse {
        configured: sp_dc.is_some() || token.is_some(),
        has_token: token.is_some(),
        sp_dc,
    })
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SpotifyLoginStatus {
    pub status: String,
    pub message: String,
    pub sp_dc: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct SpotifyImportResponse {
    pub success: bool,
    pub sp_dc: Option<String>,
    pub source: Option<String>,
    pub message: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct SpotifyLaunchResponse {
    pub success: bool,
    pub message: String,
}

pub fn run_spotify_helper(cmd: &str) -> Option<serde_json::Value> {
    run_spotify_helper_args(&[cmd])
}

pub fn run_spotify_helper_args(args: &[&str]) -> Option<serde_json::Value> {
    let script_paths = [
        dirs::data_dir().map(|d| d.join("arcy-music/scripts/spotify_helper.py")),
        dirs::config_dir().map(|d| d.join("arcy-music/spotify_helper.py")),
        std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("scripts/spotify_helper.py"))),
        std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("../share/arcy-music/scripts/spotify_helper.py"))),
        Some(std::path::PathBuf::from("/usr/share/arcy-music/scripts/spotify_helper.py")),
        Some(std::path::PathBuf::from("/usr/local/share/arcy-music/scripts/spotify_helper.py")),
        Some(std::path::PathBuf::from("arcy-music-daemon/scripts/spotify_helper.py")),
        Some(std::path::PathBuf::from("scripts/spotify_helper.py")),
    ];

    let script = script_paths.into_iter().flatten().find(|p| p.exists())?;

    let output = std::process::Command::new("python3")
        .arg(&script)
        .args(args)
        .output()
        .ok()?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        serde_json::from_str(&stdout).ok()
    } else {
        None
    }
}

pub async fn spotify_import_browser(
    State(state): State<Arc<AppState>>,
) -> Json<SpotifyImportResponse> {
    if let Some(json) = run_spotify_helper("import-browser") {
        if json.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
            let sp_dc = json.get("sp_dc").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let source = json.get("source").and_then(|v| v.as_str()).unwrap_or("Browser").to_string();
            let msg = json.get("message").and_then(|v| v.as_str()).unwrap_or("Imported successfully").to_string();

            if !sp_dc.is_empty() {
                let conn = state.db.lock().await;
                let _ = conn.execute(
                    "INSERT INTO settings (key, value) VALUES ('spotify_sp_dc', ?1) ON CONFLICT(key) DO UPDATE SET value = ?1",
                    [&sp_dc],
                );
                drop(conn);

                let mut status = state.spotify_login_status.write().await;
                *status = SpotifyLoginStatus {
                    status: "success".to_string(),
                    message: msg.clone(),
                    sp_dc: Some(sp_dc.clone()),
                };
                drop(status);

                let state_clone = Arc::clone(&state);
                let sp_dc_clone = sp_dc.clone();
                tokio::spawn(async move {
                    let _ = spotify_sync(State(state_clone), Json(SpotifySyncRequest {
                        sp_dc: Some(sp_dc_clone),
                        access_token: None,
                    })).await;
                });

                return Json(SpotifyImportResponse {
                    success: true,
                    sp_dc: Some(sp_dc),
                    source: Some(source),
                    message: msg,
                });
            }
        } else {
            let err = json.get("error").and_then(|v| v.as_str()).unwrap_or("No Spotify session found in local browsers").to_string();
            return Json(SpotifyImportResponse {
                success: false,
                sp_dc: None,
                source: None,
                message: err,
            });
        }
    }

    Json(SpotifyImportResponse {
        success: false,
        sp_dc: None,
        source: None,
        message: "Failed to execute browser cookie importer".into(),
    })
}

pub async fn spotify_launch_login(
    State(state): State<Arc<AppState>>,
) -> Json<SpotifyLaunchResponse> {
    if let Some(json) = run_spotify_helper("launch-login") {
        if json.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
            let msg = json.get("message").and_then(|v| v.as_str()).unwrap_or("Spotify login window opened").to_string();

            let mut status = state.spotify_login_status.write().await;
            *status = SpotifyLoginStatus {
                status: "logging_in".to_string(),
                message: msg.clone(),
                sp_dc: None,
            };
            drop(status);

            let state_clone = Arc::clone(&state);
            tokio::spawn(async move {
                for _ in 0..200 {
                    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;
                    if let Some(res) = run_spotify_helper("poll-login") {
                        if res.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                            let sp_dc = res.get("sp_dc").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let msg = res.get("message").and_then(|v| v.as_str()).unwrap_or("Logged in").to_string();
                            if !sp_dc.is_empty() {
                                let conn = state_clone.db.lock().await;
                                let _ = conn.execute(
                                    "INSERT INTO settings (key, value) VALUES ('spotify_sp_dc', ?1) ON CONFLICT(key) DO UPDATE SET value = ?1",
                                    [&sp_dc],
                                );
                                drop(conn);

                                let mut s = state_clone.spotify_login_status.write().await;
                                *s = SpotifyLoginStatus {
                                    status: "success".to_string(),
                                    message: msg,
                                    sp_dc: Some(sp_dc.clone()),
                                };
                                drop(s);

                                let sync_state = Arc::clone(&state_clone);
                                tokio::spawn(async move {
                                    let _ = spotify_sync(State(sync_state), Json(SpotifySyncRequest {
                                        sp_dc: Some(sp_dc),
                                        access_token: None,
                                    })).await;
                                });
                                break;
                            }
                        } else {
                            let in_progress = res.get("in_progress").and_then(|v| v.as_bool()).unwrap_or(false);
                            if !in_progress {
                                let err = res.get("error").and_then(|v| v.as_str()).unwrap_or("Login window was closed").to_string();
                                let mut s = state_clone.spotify_login_status.write().await;
                                *s = SpotifyLoginStatus {
                                    status: "error".to_string(),
                                    message: err,
                                    sp_dc: None,
                                };
                                break;
                            }
                        }
                    }
                }
            });

            return Json(SpotifyLaunchResponse {
                success: true,
                message: msg,
            });
        }
    }

    Json(SpotifyLaunchResponse {
        success: false,
        message: "Failed to launch Spotify login window".into(),
    })
}

pub async fn spotify_login_status(
    State(state): State<Arc<AppState>>,
) -> Json<SpotifyLoginStatus> {
    let s = state.spotify_login_status.read().await;
    Json(s.clone())
}

pub async fn search_spotify(
    State(state): State<Arc<AppState>>,
    Path(query): Path<String>,
) -> Json<Vec<Track>> {
    let conn = state.db.lock().await;
    let sp_dc: Option<String> = conn.query_row(
        "SELECT value FROM settings WHERE key = 'spotify_sp_dc'",
        [],
        |r| r.get(0),
    ).ok();
    let token_opt: Option<String> = conn.query_row(
        "SELECT value FROM settings WHERE key = 'spotify_token'",
        [],
        |r| r.get(0),
    ).ok();
    drop(conn);

    let token = if let Some(t) = token_opt {
        t
    } else if let Some(dc) = sp_dc {
        match spotify::get_bearer_token(&dc).await {
            Ok(t) => t,
            Err(_) => return Json(Vec::new()),
        }
    } else {
        return Json(Vec::new());
    };

    match spotify::search_spotify(&token, &query).await {
        Ok(tracks) => Json(tracks),
        Err(_) => Json(Vec::new()),
    }
}

pub async fn switch_to_native_gui() -> Json<bool> {
    tokio::task::spawn_blocking(|| {
        println!("[API] switch_to_native_gui requested");
        // Kill Web GUI processes
        if let Ok(pid_str) = std::fs::read_to_string("/tmp/arcy-music-gui-browser.pid") {
            if let Ok(pid) = pid_str.trim().parse::<i32>() {
                let _ = std::process::Command::new("kill").arg("-9").arg(pid.to_string()).status();
            }
        }
        if let Ok(pid_str) = std::fs::read_to_string("/tmp/arcy-music-gui-web.pid") {
            if let Ok(pid) = pid_str.trim().parse::<i32>() {
                let _ = std::process::Command::new("kill").arg("-9").arg(pid.to_string()).status();
            }
        }
        let _ = std::process::Command::new("pkill").arg("-f").arg("arcy-music-gui-web").status();

        // Spawn Native GUI
        let candidates = [
            std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("arcy-music-gui-native"))),
            std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("arcy-gui"))),
            dirs::home_dir().map(|h| h.join(".local/bin/arcy-music-gui-native")),
            dirs::home_dir().map(|h| h.join(".local/bin/arcy-gui")),
            Some(std::path::PathBuf::from("/usr/bin/arcy-music-gui-native")),
            Some(std::path::PathBuf::from("/usr/local/bin/arcy-music-gui-native")),
            Some(std::path::PathBuf::from("target/release/arcy-music-gui-native")),
            Some(std::path::PathBuf::from("target/debug/arcy-music-gui-native")),
        ];
        if let Some(bin) = candidates.into_iter().flatten().find(|p| p.exists()) {
            println!("[API] Spawning native GUI: {:?}", bin);
            let mut cmd = std::process::Command::new(bin);
            if let Ok(display) = std::env::var("DISPLAY") {
                cmd.env("DISPLAY", display);
            }
            if let Ok(pulse) = std::env::var("PULSE_SINK") {
                cmd.env("PULSE_SINK", pulse);
            }
            if let Ok(silent) = std::env::var("ARCY_SILENT") {
                cmd.env("ARCY_SILENT", silent);
            }
            let _ = cmd.spawn();
        } else {
            eprintln!("[API] arcy-music-gui-native binary not found");
        }
    });
    Json(true)
}

pub async fn switch_to_web_gui() -> Json<bool> {
    tokio::task::spawn_blocking(|| {
        println!("[API] switch_to_web_gui requested");
        // Kill Native GUI process
        if let Ok(pid_str) = std::fs::read_to_string("/tmp/arcy-music-gui-native.pid") {
            if let Ok(pid) = pid_str.trim().parse::<i32>() {
                let _ = std::process::Command::new("kill").arg("-15").arg(pid.to_string()).status();
            }
        }
        let _ = std::process::Command::new("pkill").arg("-f").arg("arcy-music-gui-native").status();

        // Spawn Web GUI launcher
        let candidates = [
            std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("arcy-music-gui-web"))),
            std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("arcy-web"))),
            dirs::home_dir().map(|h| h.join(".local/bin/arcy-music-gui-web")),
            dirs::home_dir().map(|h| h.join(".local/bin/arcy-web")),
            Some(std::path::PathBuf::from("/usr/bin/arcy-music-gui-web")),
            Some(std::path::PathBuf::from("/usr/local/bin/arcy-music-gui-web")),
            Some(std::path::PathBuf::from("target/release/arcy-music-gui-web")),
            Some(std::path::PathBuf::from("target/debug/arcy-music-gui-web")),
        ];
        if let Some(bin) = candidates.into_iter().flatten().find(|p| p.exists()) {
            println!("[API] Spawning web GUI launcher: {:?}", bin);
            let mut cmd = std::process::Command::new(bin);
            if let Ok(display) = std::env::var("DISPLAY") {
                cmd.env("DISPLAY", display);
            }
            if let Ok(pulse) = std::env::var("PULSE_SINK") {
                cmd.env("PULSE_SINK", pulse);
            }
            if let Ok(silent) = std::env::var("ARCY_SILENT") {
                cmd.env("ARCY_SILENT", silent);
            }
            let _ = cmd.spawn();
        } else {
            eprintln!("[API] arcy-music-gui-web binary not found");
        }
    });
    Json(true)
}



