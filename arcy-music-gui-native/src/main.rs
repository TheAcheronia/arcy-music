mod async_model;
use arcy_music_core::{
    theme::ThemeColors, Command, EqConfig, PlaybackState, RepeatMode,
    Track as CoreTrack, TrackSource,
};
use reqwest::Client;
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::rc::Rc;

slint::include_modules!();

fn format_time(seconds: f32) -> String {
    let s = seconds.max(0.0) as u64;
    let mins = s / 60;
    let secs = s % 60;
    format!("{}:{:02}", mins, secs)
}

fn parse_hex_color(hex: &str) -> slint::Color {
    let clean = hex.trim_start_matches('#');
    if clean.len() == 6 {
        let r = u8::from_str_radix(&clean[0..2], 16).unwrap_or(255);
        let g = u8::from_str_radix(&clean[2..4], 16).unwrap_or(255);
        let b = u8::from_str_radix(&clean[4..6], 16).unwrap_or(255);
        slint::Color::from_argb_u8(255, r, g, b)
    } else {
        slint::Color::from_argb_u8(255, 255, 255, 255)
    }
}

fn apply_theme_to_ui(ui: &MainWindow, theme_name: &str) {
    let colors = ThemeColors::get_by_name(theme_name);
    ui.set_theme_name(colors.name.into());
    ui.set_t_bg_base(parse_hex_color(&colors.bg_base));
    ui.set_t_bg_surface(parse_hex_color(&colors.bg_surface));
    ui.set_t_bg_overlay(parse_hex_color(&colors.bg_overlay));
    ui.set_t_card_bg(parse_hex_color(&colors.card_bg));
    ui.set_t_accent(parse_hex_color(&colors.accent));
    ui.set_t_accent_secondary(parse_hex_color(&colors.accent_secondary));
    ui.set_t_accent_pink(parse_hex_color(&colors.accent_pink));
    ui.set_t_accent_green(parse_hex_color(&colors.accent_green));
    ui.set_t_accent_yellow(parse_hex_color(&colors.accent_yellow));
    ui.set_t_accent_red(parse_hex_color(&colors.accent_red));
    ui.set_t_text_primary(parse_hex_color(&colors.text_primary));
    ui.set_t_text_secondary(parse_hex_color(&colors.text_secondary));
    ui.set_t_text_muted(parse_hex_color(&colors.text_muted));
    ui.set_t_border(parse_hex_color(&colors.border));
}

fn load_image_robust(path_str: &str) -> Option<slint::Image> {
    if path_str.starts_with("http://") || path_str.starts_with("https://") {
        let thumbs_dir = dirs::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
            .join("arcy-music/thumbnails");
        let _ = std::fs::create_dir_all(&thumbs_dir);

        let raw_name = path_str.split('/').last().unwrap_or("thumb").split('?').next().unwrap_or("thumb");
        let safe_name: String = raw_name.chars().filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-').collect();
        let cached_path = thumbs_dir.join(format!("{}.jpg", safe_name));

        if cached_path.exists() {
            if let Ok(dyn_img) = image::open(&cached_path) {
                let rgba = dyn_img.to_rgba8();
                let (w, h) = rgba.dimensions();
                let mut buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(w, h);
                let slice = buffer.make_mut_slice();
                let raw = rgba.as_raw();
                for (i, pixel) in slice.iter_mut().enumerate() {
                    let idx = i * 4;
                    pixel.r = raw[idx];
                    pixel.g = raw[idx + 1];
                    pixel.b = raw[idx + 2];
                    pixel.a = raw[idx + 3];
                }
                return Some(slint::Image::from_rgba8(buffer));
            }
        } else if let Ok(resp) = reqwest::blocking::get(path_str) {
            if let Ok(bytes) = resp.bytes() {
                if let Ok(dyn_img) = image::load_from_memory(&bytes) {
                    let _ = dyn_img.to_rgb8().save_with_format(&cached_path, image::ImageFormat::Jpeg);
                    let rgba = dyn_img.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    let mut buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(w, h);
                    let slice = buffer.make_mut_slice();
                    let raw = rgba.as_raw();
                    for (i, pixel) in slice.iter_mut().enumerate() {
                        let idx = i * 4;
                        pixel.r = raw[idx];
                        pixel.g = raw[idx + 1];
                        pixel.b = raw[idx + 2];
                        pixel.a = raw[idx + 3];
                    }
                    return Some(slint::Image::from_rgba8(buffer));
                }
            }
        }
        return None;
    }

    let path = std::path::Path::new(path_str);
    if let Ok(img) = slint::Image::load_from_path(path) {
        return Some(img);
    }
    if let Ok(dyn_img) = image::open(path) {
        let rgba = dyn_img.to_rgba8();
        let (w, h) = rgba.dimensions();
        let mut buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(w, h);
        let slice = buffer.make_mut_slice();
        let raw = rgba.as_raw();
        for (i, pixel) in slice.iter_mut().enumerate() {
            let idx = i * 4;
            pixel.r = raw[idx];
            pixel.g = raw[idx + 1];
            pixel.b = raw[idx + 2];
            pixel.a = raw[idx + 3];
        }
        return Some(slint::Image::from_rgba8(buffer));
    }
    None
}

static ALL_PLAYLISTS_CACHE: std::sync::RwLock<Vec<arcy_music_core::Playlist>> = std::sync::RwLock::new(Vec::new());

async fn refresh_playlists_helper(client: &Client, base: &str, ui_weak: slint::Weak<MainWindow>) {
    if let Ok(res) = client.get(format!("{}/api/playlists", base)).send().await {
        if let Ok(playlists) = res.json::<Vec<arcy_music_core::Playlist>>().await {
            if let Ok(mut cache) = ALL_PLAYLISTS_CACHE.write() {
                *cache = playlists.clone();
            }
            let ui_w = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_w.upgrade() {
                    let q = ui.get_playlist_filter_query().to_lowercase();
                    let slint_pls: Vec<Playlist> = playlists
                        .into_iter()
                        .filter(|p| q.is_empty() || p.name.to_lowercase().contains(&q))
                        .map(|p| Playlist {
                            id: p.id.into(),
                            name: p.name.into(),
                            track_count: p.track_count as i32,
                            is_pinned: p.is_pinned,
                            is_default: p.is_default,
                        })
                        .collect();
                    ui.set_playlists(ModelRc::from(Rc::new(VecModel::from(slint_pls))));
                }
            });
        }
    }
}

thread_local! {
    static ACTIVE_PLAYLIST_MODEL: std::cell::RefCell<Option<std::rc::Rc<crate::async_model::AsyncTrackModel>>> = std::cell::RefCell::new(None);
}

async fn refresh_playlist_tracks_helper(client: &Client, base: &str, playlist_id: &str, ui_weak: slint::Weak<MainWindow>) {
    if let Ok(res) = client.get(format!("{}/api/playlists/{}/tracks", base, playlist_id)).send().await {
        if let Ok(tracks) = res.json::<Vec<CoreTrack>>().await {
            let ui_w = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_w.upgrade() {
                    let async_model = crate::async_model::AsyncTrackModel::new(tracks);
                    ACTIVE_PLAYLIST_MODEL.with(|m| {
                        *m.borrow_mut() = Some(async_model.clone());
                    });
                    ui.set_playlist_tracks(ModelRc::new(async_model));
                }
            });
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pid = std::process::id();
    let _ = std::fs::write("/tmp/arcy-music-gui-native.pid", pid.to_string());

    let args: Vec<String> = std::env::args().collect();
    let mut is_dashboard_mode = false;
    let mut is_frameless = false;

    for (i, arg) in args.iter().enumerate() {
        if arg == "dashboard" || arg == "--dashboard" || arg == "-d" || arg == "--overlay" {
            is_dashboard_mode = true;
            is_frameless = true;
        }
        if arg == "window" || arg == "--window" || arg == "-w" {
            is_dashboard_mode = false;
            is_frameless = false;
        }
        if (arg == "--mode" || arg == "-m") && i + 1 < args.len() {
            if args[i + 1] == "dashboard" || args[i + 1] == "overlay" {
                is_dashboard_mode = true;
                is_frameless = true;
            } else if args[i + 1] == "window" {
                is_dashboard_mode = false;
                is_frameless = false;
            }
        }
    }

    let main_window = MainWindow::new()?;

    main_window.set_is_dashboard_mode(is_dashboard_mode);
    main_window.set_is_frameless(is_frameless);

    // Apply default theme
    apply_theme_to_ui(&main_window, "mocha");

    // Load User Avatar from .face or fallback
    let face_candidates = [
        dirs::home_dir().map(|h| h.join(".face")),
        dirs::home_dir().map(|h| h.join(".face.icon")),
        dirs::config_dir().map(|c| c.join("arcy-music/avatar.png")),
        dirs::data_dir().map(|d| d.join("arcy-music/avatar.png")),
    ];
    for candidate in face_candidates.into_iter().flatten() {
        if let Some(img) = load_image_robust(&candidate.to_string_lossy()) {
            main_window.set_user_avatar(img);
            break;
        }
    }

    let tracks_model = Rc::new(VecModel::default());
    main_window.set_library_tracks(ModelRc::from(tracks_model));
    main_window.set_queue_tracks(ModelRc::from(Rc::new(VecModel::<Track>::default())));
    main_window.set_playlists(ModelRc::from(Rc::new(VecModel::<Playlist>::default())));
    main_window.set_playlist_tracks(ModelRc::from(Rc::new(VecModel::<Track>::default())));

    let daemon_port: u16 = std::env::var("ARCY_MUSIC_PORT")
        .or_else(|_| std::env::var("ARCY_PORT"))
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(13338);
    let daemon_base = format!("http://127.0.0.1:{}", daemon_port);

    let client = Client::new();

    // Background Async Runtime
    let rt = tokio::runtime::Runtime::new()?;
    let handle = rt.handle().clone();

    let ui_weak = main_window.as_weak();
    let client_clone = client.clone();
    let base_clone = daemon_base.clone();

    // Spawn Background Telemetry & Audio Polling Thread
    std::thread::spawn(move || {
        rt.block_on(async move {
            let client = client_clone;

            // 1. Initial Theme Fetch
            if let Ok(res) = client.get(format!("{}/api/theme", base_clone)).send().await {
                if let Ok(val) = res.json::<serde_json::Value>().await {
                    if let Some(t) = val["current"].as_str() {
                        let theme_str = t.to_string();
                        let ui_w = ui_weak.clone();
                        slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_w.upgrade() {
                                apply_theme_to_ui(&ui, &theme_str);
                            }
                        })
                        .unwrap_or_default();
                    }
                }
            }

            // 2. Initial Local Library Fetch
            if let Ok(res) = client.get(format!("{}/api/library", base_clone)).send().await {
                if let Ok(tracks) = res.json::<Vec<CoreTrack>>().await {
                    let ui_w = ui_weak.clone();
                    slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_w.upgrade() {
                            let async_model = crate::async_model::AsyncTrackModel::new(tracks);
                            ui.set_library_tracks(ModelRc::new(async_model));
                        }
                    })
                    .unwrap_or_default();
                }
            }

            // 3. Initial Equalizer Fetch
            if let Ok(res) = client.get(format!("{}/api/dsp/eq", base_clone)).send().await {
                if let Ok(eq) = res.json::<EqConfig>().await {
                    let ui_w = ui_weak.clone();
                    slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_w.upgrade() {
                            ui.set_eq_enabled(eq.enabled);
                            ui.set_eq_band0(eq.bands[0]);
                            ui.set_eq_band1(eq.bands[1]);
                            ui.set_eq_band2(eq.bands[2]);
                            ui.set_eq_band3(eq.bands[3]);
                            ui.set_eq_band4(eq.bands[4]);
                            ui.set_eq_spatial_mode(eq.spatial_mode.into());
                        }
                    })
                    .unwrap_or_default();
                }
            }

            // 4. Initial Settings Fetch
            if let Ok(res) = client.get(format!("{}/api/settings/mini_thumbnails", base_clone)).send().await {
                if let Ok(val) = res.json::<Option<String>>().await {
                    let is_enabled = val.unwrap_or_else(|| "0".to_string()) == "1";
                    let ui_w = ui_weak.clone();
                    slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_w.upgrade() {
                            ui.set_setting_mini_thumbnails(is_enabled);
                        }
                    })
                    .unwrap_or_default();
                }
            }

            if let Ok(res) = client.get(format!("{}/api/settings/download_path", base_clone)).send().await {
                if let Ok(val) = res.json::<Option<String>>().await {
                    let path = val.unwrap_or_else(|| {
                        dirs::download_dir()
                            .unwrap_or_else(|| dirs::home_dir().unwrap().join("Downloads"))
                            .to_string_lossy()
                            .to_string()
                    });
                    let ui_w = ui_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_w.upgrade() {
                            ui.set_setting_download_path(path.into());
                        }
                    });
                }
            }

            if let Ok(res) = client.get(format!("{}/api/spotify/status", base_clone)).send().await {
                if let Ok(json) = res.json::<serde_json::Value>().await {
                    let configured = json.get("configured").and_then(|v| v.as_bool()).unwrap_or(false);
                    let sp_dc = json.get("sp_dc").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let ui_w = ui_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_w.upgrade() {
                            if !sp_dc.is_empty() {
                                ui.set_spotify_sp_dc_input(sp_dc.into());
                            }
                            if configured {
                                ui.set_spotify_sync_status("Spotify account linked & ready.".into());
                            }
                        }
                    });
                }
            }

            // Initial Playlists Fetch
            refresh_playlists_helper(&client, &base_clone, ui_weak.clone()).await;

            // 5. Polling Loop (150ms tick for fluid animations and seeking)
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(150));
            let mut last_track_id = String::new();
            let mut tick_counter: u64 = 0;
            let mut current_synced: Vec<LyricItem> = Vec::new();

            loop {
                interval.tick().await;
                tick_counter = tick_counter.wrapping_add(1);

                // Dynamic Visualizer bars (16 bars dancing when playing)
                let phase = tick_counter as f32 * 0.35;
                let mut bars: [f32; 16] = [0.08; 16];

                // Periodic Playlists Refresh (every ~2.2 seconds = 15 ticks)
                if tick_counter % 15 == 0 {
                    refresh_playlists_helper(&client, &base_clone, ui_weak.clone()).await;
                }

                // Playback State Polling
                if let Ok(res) = client.get(format!("{}/api/state", base_clone)).send().await {
                    if let Ok(state) = res.json::<PlaybackState>().await {
                        let is_playing = state.is_playing;
                        let track_changed = if let Some(ref track) = state.current_track {
                            if track.id != last_track_id {
                                last_track_id = track.id.clone();
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        };

                        let mut new_lyrics = None;
                        if track_changed {
                            if let Some(ref track) = state.current_track {
                                if let Ok(lyr_res) = client.get(format!("{}/api/lyrics/{}", base_clone, track.id)).send().await {
                                    if let Ok(Some(lyrics_data)) = lyr_res.json::<Option<serde_json::Value>>().await {
                                        let plain = lyrics_data["plain_lyrics"].as_str().unwrap_or("").to_string();
                                        let mut synced = Vec::new();
                                        if let Some(arr) = lyrics_data["synced_lyrics"].as_array() {
                                            for item in arr {
                                                synced.push(LyricItem {
                                                    time_secs: item["time_secs"].as_f64().unwrap_or(0.0) as f32,
                                                    text: item["text"].as_str().unwrap_or("").into(),
                                                });
                                            }
                                        }
                                        new_lyrics = Some((plain, synced));
                                    }
                                }
                            }
                        }

                        if let Some((_, ref s)) = new_lyrics {
                            current_synced = s.clone();
                        }

                        let pos = state.position_secs;
                        let mut active_idx: i32 = -1;
                        for (i, lyr) in current_synced.iter().enumerate() {
                            if lyr.time_secs <= pos {
                                active_idx = i as i32;
                            } else {
                                break;
                            }
                        }

                        if is_playing {
                            for (i, bar) in bars.iter_mut().enumerate() {
                                let offset = (i as f32) * 0.45;
                                let wave = ((phase + offset).sin() * 0.5 + 0.5) * 0.75 + 0.2;
                                *bar = wave.clamp(0.1, 1.0);
                            }
                        }

                        let bars_vec = bars.to_vec();
                        let ui_w = ui_weak.clone();

                        slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_w.upgrade() {
                                ui.set_visualizer_bars(ModelRc::from(Rc::new(VecModel::from(bars_vec))));

                                ui.set_is_playing(state.is_playing);
                                ui.set_is_shuffle(state.shuffle);
                                ui.set_repeat_mode(match state.repeat_mode {
                                    RepeatMode::Off => "Off".into(),
                                    RepeatMode::All => "All".into(),
                                    RepeatMode::One => "One".into(),
                                });

                                if (ui.get_volume() - state.volume).abs() > 0.02 {
                                    ui.set_volume(state.volume);
                                }
                                ui.set_playback_position(state.position_secs);
                                ui.set_playback_position_str(format_time(state.position_secs).into());

                                if state.duration_secs > 0.0 && (ui.get_playback_duration() - state.duration_secs).abs() > 0.5 {
                                    ui.set_playback_duration(state.duration_secs);
                                    ui.set_playback_duration_str(format_time(state.duration_secs).into());
                                }

                                let prev_active_idx = ui.get_active_lyric_index();
                                ui.set_active_lyric_index(active_idx);

                                let target_y = (-(active_idx * 60) + 220).min(0) as f32;
                                let current_viewport_y = ui.get_lyrics_viewport_y();
                                let last_lyrics_target_y = ui.get_last_lyrics_target_y();

                                let suppress_ticks = ui.get_lyrics_suppress_ticks();
                                if suppress_ticks > 0 {
                                    ui.set_lyrics_suppress_ticks(suppress_ticks - 1);
                                } else if ui.get_lyrics_auto_locked() {
                                    if (current_viewport_y - last_lyrics_target_y).abs() > 60.0 {
                                        ui.set_lyrics_auto_locked(false);
                                    }
                                }

                                if active_idx != prev_active_idx && active_idx >= 0 && ui.get_lyrics_auto_locked() {
                                    ui.set_lyrics_viewport_y(target_y);
                                    ui.set_last_lyrics_target_y(target_y);
                                    ui.set_lyrics_suppress_ticks(4);
                                }

                                if track_changed {
                                    ui.set_lyrics_auto_locked(true);
                                    ui.set_last_lyrics_target_y(0.0);
                                    ui.set_lyrics_viewport_y(0.0);
                                    ui.set_lyrics_suppress_ticks(4);
                                    if let Some(track) = state.current_track {
                                        ui.set_current_track_id(track.id.clone().into());
                                        ui.set_current_track_title(track.title.into());
                                        ui.set_current_track_artist(
                                            track.artist.unwrap_or_else(|| "Unknown Artist".to_string()).into(),
                                        );
                                        let mut dur = track.duration_secs.unwrap_or(0) as f32;
                                        if dur <= 0.0 && state.duration_secs > 0.0 {
                                            dur = state.duration_secs;
                                        }
                                        ui.set_playback_duration(dur);
                                        ui.set_playback_duration_str(format_time(dur).into());

                                        if let Some(path) = track.thumbnail_url {
                                            if let Some(img) = load_image_robust(&path) {
                                                ui.set_current_track_thumbnail(img);
                                            } else {
                                                ui.set_current_track_thumbnail(ui.get_user_avatar());
                                            }
                                        } else {
                                            // Fallback to user avatar for fancy aesthetic
                                            ui.set_current_track_thumbnail(ui.get_user_avatar());
                                        }
                                    }
                                }

                                if let Some((plain, synced)) = new_lyrics {
                                    ui.set_plain_lyrics(plain.into());
                                    ui.set_synced_lyrics(ModelRc::from(Rc::new(VecModel::from(synced))));
                                }

                                let mut queue_slint = Vec::new();
                                for q in &state.queue {
                                    queue_slint.push(Track {
                                        id: q.id.clone().into(),
                                        title: q.title.clone().into(),
                                        artist: q.artist.clone().unwrap_or_else(|| "Unknown Artist".into()).into(),
                                        duration: format_time(q.duration_secs.unwrap_or(0) as f32).into(),
                                        thumbnail: slint::Image::default(),
                                    });
                                }
                                ui.set_queue_tracks(ModelRc::from(Rc::new(VecModel::from(queue_slint))));
                            }
                        })
                        .unwrap_or_default();
                    }
                }
            }
        });
    });

    // Helper closures for sending commands through background Tokio handle
    macro_rules! bind_audio_cmd {
        ($window:expr, $method:ident, $cmd:expr, $h:expr, $cl:expr, $base:expr) => {{
            let h = $h.clone();
            let cl = $cl.clone();
            let base = $base.clone();
            $window.$method(move || {
                let cl = cl.clone();
                let base = base.clone();
                h.spawn(async move {
                    let _ = cl.post(format!("{}/api/command", base)).json(&$cmd).send().await;
                });
            });
        }};
    }

    // Toggle Window Maximize Callback
    let ui_weak_max = main_window.as_weak();
    main_window.on_toggle_maximize(move || {
        if let Some(ui) = ui_weak_max.upgrade() {
            let next = !ui.get_is_maximized();
            ui.window().set_maximized(next);
            ui.set_is_maximized(next);
        }
    });

    // Exit GUI (Keep Playing) Callback
    let ui_weak_exit = main_window.as_weak();
    main_window.on_exit_gui(move || {
        let _ = std::fs::remove_file("/tmp/arcy-music-gui-native.pid");
        if let Some(ui) = ui_weak_exit.upgrade() {
            let _ = ui.window().hide();
        }
        std::process::exit(0);
    });

    // Stop Daemon & Exit Callback
    let h = handle.clone();
    let cl = client.clone();
    let base = daemon_base.clone();
    let ui_weak_stop = main_window.as_weak();
    main_window.on_stop_daemon_and_exit(move || {
        let _ = std::fs::remove_file("/tmp/arcy-music-gui-native.pid");
        let cl = cl.clone();
        let base = base.clone();
        let ui_w = ui_weak_stop.clone();
        h.spawn(async move {
            let _ = cl.post(format!("{}/api/daemon/shutdown", base)).send().await;
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_w.upgrade() {
                    let _ = ui.window().hide();
                }
                std::process::exit(0);
            });
        });
    });

    // Switch to Web GUI Callback
    let h_web = handle.clone();
    let cl_web = client.clone();
    let base_web = daemon_base.clone();
    let ui_weak_web = main_window.as_weak();
    main_window.on_switch_to_web_gui(move || {
        let cl = cl_web.clone();
        let base = base_web.clone();
        let ui_w = ui_weak_web.clone();
        let _ = std::fs::remove_file("/tmp/arcy-music-gui-native.pid");
        h_web.spawn(async move {
            let _ = cl.post(format!("{}/api/gui/switch-to-web", base)).send().await;
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_w.upgrade() {
                    let _ = ui.window().hide();
                }
                std::process::exit(0);
            });
        });
    });

    // Dynamic Theme Switching Callback
    let h = handle.clone();
    let cl = client.clone();
    let base = daemon_base.clone();
    let ui_weak_theme = main_window.as_weak();
    main_window.on_switch_theme(move |theme_name| {
        let name_str = theme_name.to_string();
        if let Some(ui) = ui_weak_theme.upgrade() {
            apply_theme_to_ui(&ui, &name_str);
        }
        let cl = cl.clone();
        let base = base.clone();
        h.spawn(async move {
            let body = serde_json::json!({ "theme": name_str });
            let _ = cl.post(format!("{}/api/theme", base)).json(&body).send().await;
        });
    });

    // GUI Form Factor Switcher Callback
    main_window.on_toggle_gui_mode(|| {});

    // Close Window Callback
    let ui_weak_close = main_window.as_weak();
    main_window.on_close_window(move || {
        if let Some(ui) = ui_weak_close.upgrade() {
            let _ = ui.window().hide();
        }
        std::process::exit(0);
    });

    // Audio Playback Bindings
    bind_audio_cmd!(main_window, on_toggle_play, Command::PlayPause, handle, client, daemon_base);
    bind_audio_cmd!(main_window, on_next_track, Command::Next, handle, client, daemon_base);
    bind_audio_cmd!(main_window, on_prev_track, Command::Previous, handle, client, daemon_base);
    bind_audio_cmd!(main_window, on_toggle_shuffle, Command::ToggleShuffle, handle, client, daemon_base);
    bind_audio_cmd!(main_window, on_toggle_repeat, Command::ToggleRepeat, handle, client, daemon_base);
    bind_audio_cmd!(main_window, on_clear_queue, Command::ClearQueue, handle, client, daemon_base);

    let h = handle.clone();
    let cl = client.clone();
    let base = daemon_base.clone();
    main_window.on_play_track(move |id| {
        let cl = cl.clone();
        let base = base.clone();
        let id_str = id.to_string();
        h.spawn(async move {
            let _ = cl
                .post(format!("{}/api/command", base))
                .json(&Command::PlayTrackId(id_str))
                .send()
                .await;
        });
    });

    let h = handle.clone();
    let cl = client.clone();
    let base = daemon_base.clone();
    main_window.on_volume_changed(move |val| {
        let cl = cl.clone();
        let base = base.clone();
        h.spawn(async move {
            let _ = cl
                .post(format!("{}/api/command", base))
                .json(&Command::SetVolume(val))
                .send()
                .await;
        });
    });

    let h = handle.clone();
    let cl = client.clone();
    let base = daemon_base.clone();
    main_window.on_seek(move |pos| {
        let cl = cl.clone();
        let base = base.clone();
        h.spawn(async move {
            let _ = cl
                .post(format!("{}/api/command", base))
                .json(&Command::Seek(pos))
                .send()
                .await;
        });
    });

    let h = handle.clone();
    let cl = client.clone();
    let base = daemon_base.clone();
    main_window.on_seek_relative(move |delta| {
        let cl = cl.clone();
        let base = base.clone();
        h.spawn(async move {
            let _ = cl
                .post(format!("{}/api/command", base))
                .json(&Command::SeekRelative(delta))
                .send()
                .await;
        });
    });

    let h = handle.clone();
    let cl = client.clone();
    let base = daemon_base.clone();
    let ui_w = main_window.as_weak();
    main_window.on_open_playlist(move |id| {
        let cl = cl.clone();
        let base = base.clone();
        let ui_w = ui_w.clone();
        let id_str = id.to_string();
        h.spawn(async move {
            if let Ok(res) = cl.get(format!("{}/api/playlists/{}/tracks", base, id_str)).send().await {
                if let Ok(tracks) = res.json::<Vec<CoreTrack>>().await {
                    let id_for_ui = id_str.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_w.upgrade() {
                            let async_model = crate::async_model::AsyncTrackModel::new(tracks);
                            ACTIVE_PLAYLIST_MODEL.with(|m| {
                                *m.borrow_mut() = Some(async_model.clone());
                            });
                            ui.set_current_playlist_id(id_for_ui.clone().into());
                            ui.set_current_playlist_is_default(id_for_ui == "local");
                            ui.set_playlist_sort_mode("custom".into());
                            ui.set_is_editing_playlist(false);
                            ui.set_playlist_search_query("".into());
                            ui.set_is_searching_playlist(false);
                            if let Some(model) = ui.get_playlists().as_any().downcast_ref::<VecModel<Playlist>>() {
                                for i in 0..model.row_count() {
                                    if let Some(p) = model.row_data(i) {
                                        if p.id == id_for_ui {
                                            ui.set_current_playlist_name(p.name);
                                            break;
                                        }
                                    }
                                }
                            }
                            ui.set_playlist_tracks(ModelRc::new(async_model));
                        }
                    });
                }
            }
        });
    });

    let ui_w = main_window.as_weak();
    main_window.on_close_playlist(move || {
        ACTIVE_PLAYLIST_MODEL.with(|m| {
            *m.borrow_mut() = None;
        });
        if let Some(ui) = ui_w.upgrade() {
            ui.set_current_playlist_id("".into());
            ui.set_is_editing_playlist(false);
            ui.set_playlist_search_query("".into());
            ui.set_is_searching_playlist(false);
        }
    });

    main_window.on_filter_playlist_tracks(move |query| {
        let q_str = query.to_string();
        ACTIVE_PLAYLIST_MODEL.with(|m| {
            if let Some(ref model) = *m.borrow() {
                model.filter_tracks(&q_str);
            }
        });
    });

    let ui_w_filter = main_window.as_weak();
    main_window.on_filter_playlists(move |query| {
        let q = query.to_lowercase();
        if let Ok(cache) = ALL_PLAYLISTS_CACHE.read() {
            let slint_pls: Vec<Playlist> = cache
                .iter()
                .filter(|p| q.is_empty() || p.name.to_lowercase().contains(&q))
                .map(|p| Playlist {
                    id: p.id.clone().into(),
                    name: p.name.clone().into(),
                    track_count: p.track_count as i32,
                    is_pinned: p.is_pinned,
                    is_default: p.is_default,
                })
                .collect();
            if let Some(ui) = ui_w_filter.upgrade() {
                ui.set_playlists(ModelRc::from(Rc::new(VecModel::from(slint_pls))));
            }
        }
    });

    let ui_w = main_window.as_weak();
    main_window.on_set_playlist_sort(move |mode| {
        let mode_str = mode.to_string();
        ACTIVE_PLAYLIST_MODEL.with(|m| {
            if let Some(ref model) = *m.borrow() {
                model.sort_tracks(&mode_str);
            }
        });
        if let Some(ui) = ui_w.upgrade() {
            ui.set_playlist_sort_mode(mode_str.into());
        }
    });

    let ui_w = main_window.as_weak();
    main_window.on_toggle_playlist_edit_mode(move || {
        if let Some(ui) = ui_w.upgrade() {
            let current = ui.get_is_editing_playlist();
            let next = !current;
            ui.set_is_editing_playlist(next);
            if next {
                ACTIVE_PLAYLIST_MODEL.with(|m| {
                    if let Some(ref model) = *m.borrow() {
                        model.sort_tracks("custom");
                    }
                });
                ui.set_playlist_sort_mode("custom".into());
            }
        }
    });

    let cl = client.clone();
    let base = daemon_base.clone();
    let h = handle.clone();
    main_window.on_move_playlist_track(move |playlist_id, idx, direction| {
        let pl_id = playlist_id.to_string();
        let dir = direction.to_string();
        let mut moved = None;
        ACTIVE_PLAYLIST_MODEL.with(|m| {
            if let Some(ref model) = *m.borrow() {
                moved = model.move_track(idx as usize, &dir);
            }
        });

        if let Some((_from, _to, track_id)) = moved {
            if pl_id != "local" {
                let cl = cl.clone();
                let base = base.clone();
                let endpoint = if dir == "up" { "move_up" } else { "move_down" };
                h.spawn(async move {
                    let _ = cl.post(format!("{}/api/playlists/{}/tracks/{}/{}", base, pl_id, track_id, endpoint)).send().await;
                });
            }
        }
    });

    let cl = client.clone();
    let base = daemon_base.clone();
    let h = handle.clone();
    main_window.on_play_playlist_track(move |id, idx| {
        let cl = cl.clone();
        let base = base.clone();
        let id_str = id.to_string();
        let mut tracks_opt = None;
        ACTIVE_PLAYLIST_MODEL.with(|m| {
            if let Some(ref model) = *m.borrow() {
                tracks_opt = Some(model.get_all_tracks());
            }
        });

        h.spawn(async move {
            if let Some(tracks) = tracks_opt {
                let _ = cl.post(format!("{}/api/command", base))
                    .json(&Command::SetQueueAndPlay { tracks, start_index: idx as usize })
                    .send().await;
            } else {
                let _ = cl.post(format!("{}/api/command", base))
                    .json(&Command::PlayPlaylist { playlist_id: id_str, start_index: idx as usize })
                    .send().await;
            }
        });
    });

    let cl = client.clone();
    let base = daemon_base.clone();
    let h = handle.clone();
    main_window.on_play_playlist_all(move |id| {
        let cl = cl.clone();
        let base = base.clone();
        let id_str = id.to_string();
        let mut tracks_opt = None;
        ACTIVE_PLAYLIST_MODEL.with(|m| {
            if let Some(ref model) = *m.borrow() {
                tracks_opt = Some(model.get_all_tracks());
            }
        });

        h.spawn(async move {
            if let Some(tracks) = tracks_opt {
                let _ = cl.post(format!("{}/api/command", base))
                    .json(&Command::SetQueueAndPlay { tracks, start_index: 0 })
                    .send().await;
            } else {
                let _ = cl.post(format!("{}/api/command", base))
                    .json(&Command::PlayPlaylist { playlist_id: id_str, start_index: 0 })
                    .send().await;
            }
        });
    });

    let h = handle.clone();
    let cl = client.clone();
    let base = daemon_base.clone();
    let ui_w = main_window.as_weak();
    main_window.on_create_playlist(move |name| {
        let cl = cl.clone();
        let base = base.clone();
        let ui_w = ui_w.clone();
        let name_str = name.to_string();
        h.spawn(async move {
            let body = serde_json::json!({ "name": name_str });
            let _ = cl.post(format!("{}/api/playlists", base)).json(&body).send().await;
            refresh_playlists_helper(&cl, &base, ui_w).await;
        });
    });

    let h = handle.clone();
    let cl = client.clone();
    let base = daemon_base.clone();
    let ui_w = main_window.as_weak();
    main_window.on_delete_playlist(move |id| {
        let cl = cl.clone();
        let base = base.clone();
        let ui_w = ui_w.clone();
        let id_str = id.to_string();
        h.spawn(async move {
            let _ = cl.delete(format!("{}/api/playlists/{}", base, id_str)).send().await;
            refresh_playlists_helper(&cl, &base, ui_w).await;
        });
    });

    let h = handle.clone();
    let cl = client.clone();
    let base = daemon_base.clone();
    let ui_w = main_window.as_weak();
    main_window.on_toggle_pin_playlist(move |id| {
        let cl = cl.clone();
        let base = base.clone();
        let ui_w = ui_w.clone();
        let id_str = id.to_string();
        h.spawn(async move {
            let _ = cl.post(format!("{}/api/playlists/{}/pin", base, id_str)).send().await;
            refresh_playlists_helper(&cl, &base, ui_w).await;
        });
    });

    let h = handle.clone();
    let cl = client.clone();
    let base = daemon_base.clone();
    let ui_w = main_window.as_weak();
    main_window.on_move_playlist(move |id, dir| {
        let cl = cl.clone();
        let base = base.clone();
        let ui_w = ui_w.clone();
        let id_str = id.to_string();
        let dir_str = dir.to_string();
        h.spawn(async move {
            let action = if dir_str == "up" { "move_up" } else { "move_down" };
            let _ = cl.post(format!("{}/api/playlists/{}/{}", base, id_str, action)).send().await;
            refresh_playlists_helper(&cl, &base, ui_w).await;
        });
    });

    let h = handle.clone();
    let cl = client.clone();
    let base = daemon_base.clone();
    let ui_w = main_window.as_weak();
    main_window.on_remove_track_from_playlist(move |pl_id, track_id| {
        let cl = cl.clone();
        let base = base.clone();
        let ui_w = ui_w.clone();
        let pl_str = pl_id.to_string();
        let tr_str = track_id.to_string();
        ACTIVE_PLAYLIST_MODEL.with(|m| {
            if let Some(ref model) = *m.borrow() {
                model.remove_track(&tr_str);
            }
        });
        h.spawn(async move {
            let _ = cl.delete(format!("{}/api/playlists/{}/tracks/{}", base, pl_str, tr_str)).send().await;
            refresh_playlist_tracks_helper(&cl, &base, &pl_str, ui_w).await;
        });
    });

    let h = handle.clone();
    let cl = client.clone();
    let base = daemon_base.clone();
    let ui_w = main_window.as_weak();
    main_window.on_add_track_to_playlist(move |pl_id, track_id| {
        let cl = cl.clone();
        let base = base.clone();
        let ui_w = ui_w.clone();
        let pl_str = pl_id.to_string();
        let tr_str = track_id.to_string();
        h.spawn(async move {
            let body = serde_json::json!({ "track_id": tr_str });
            let _ = cl.post(format!("{}/api/playlists/{}/tracks", base, pl_str)).json(&body).send().await;
            refresh_playlists_helper(&cl, &base, ui_w).await;
        });
    });

    let h = handle.clone();
    let cl = client.clone();
    let base = daemon_base.clone();
    main_window.on_remove_from_queue(move |idx| {
        let cl = cl.clone();
        let base = base.clone();
        h.spawn(async move {
            let _ = cl
                .post(format!("{}/api/command", base))
                .json(&Command::RemoveFromQueue(idx as usize))
                .send()
                .await;
        });
    });

    // Equalizer Bindings
    let h = handle.clone();
    let cl = client.clone();
    let base = daemon_base.clone();
    let ui_weak_eq = main_window.as_weak();
    main_window.on_update_eq(move || {
        if let Some(ui) = ui_weak_eq.upgrade() {
            let config = EqConfig {
                enabled: ui.get_eq_enabled(),
                preamp_db: 0.0,
                bands: [
                    ui.get_eq_band0(),
                    ui.get_eq_band1(),
                    ui.get_eq_band2(),
                    ui.get_eq_band3(),
                    ui.get_eq_band4(),
                ],
                spatial_mode: ui.get_eq_spatial_mode().to_string(),
            };
            let cl = cl.clone();
            let base = base.clone();
            h.spawn(async move {
                let _ = cl.post(format!("{}/api/dsp/eq", base)).json(&config).send().await;
            });
        }
    });

    let h = handle.clone();
    let cl = client.clone();
    let base = daemon_base.clone();
    let ui_weak_eq_preset = main_window.as_weak();
    main_window.on_apply_eq_preset(move |preset| {
        let (b0, b1, b2, b3, b4) = match preset.as_str() {
            "bass_boost" => (6.0, 4.0, 1.0, 0.0, 0.0),
            "vocal" => (-2.0, 0.0, 4.0, 3.0, 1.0),
            "electronic" => (5.0, 3.0, -1.0, 2.0, 4.0),
            "rock" => (4.0, 2.0, -2.0, 3.0, 4.0),
            _ => (0.0, 0.0, 0.0, 0.0, 0.0),
        };

        if let Some(ui) = ui_weak_eq_preset.upgrade() {
            ui.set_eq_enabled(true);
            ui.set_eq_band0(b0);
            ui.set_eq_band1(b1);
            ui.set_eq_band2(b2);
            ui.set_eq_band3(b3);
            ui.set_eq_band4(b4);

            let config = EqConfig {
                enabled: true,
                preamp_db: 0.0,
                bands: [b0, b1, b2, b3, b4],
                spatial_mode: ui.get_eq_spatial_mode().to_string(),
            };
            let cl = cl.clone();
            let base = base.clone();
            h.spawn(async move {
                let _ = cl.post(format!("{}/api/dsp/eq", base)).json(&config).send().await;
            });
        }
    });

    // Search Bindings & Cache
    let cached_search_results = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::<String, CoreTrack>::new()));
    let h = handle.clone();
    let cl = client.clone();
    let base = daemon_base.clone();
    let ui_weak_search = main_window.as_weak();
    let search_cache_store = cached_search_results.clone();
    main_window.on_perform_search(move |query, source| {
        let query_str = query.to_string();
        let source_str = source.to_string();
        println!("GUI on_perform_search query: '{}', source: '{}'", query_str, source_str);
        if query_str.trim().is_empty() {
            return;
        }

        if let Some(ui) = ui_weak_search.upgrade() {
            ui.set_is_searching(true);
        }

        let cl = cl.clone();
        let base = base.clone();
        let ui_w = ui_weak_search.clone();
        let cache_store = search_cache_store.clone();

        h.spawn(async move {
            let encoded = query_str.replace(' ', "+");
            let url = if source_str.to_lowercase() == "spotify" {
                format!("{}/api/search/spotify/{}", base, encoded)
            } else {
                format!("{}/api/search/youtube/{}", base, encoded)
            };

            let mut fetched_tracks = Vec::new();
            if let Ok(res) = cl.get(&url).send().await {
                if let Ok(tracks) = res.json::<Vec<CoreTrack>>().await {
                    fetched_tracks = tracks;
                }
            }
            println!("GUI search got {} tracks from daemon", fetched_tracks.len());

            {
                let mut cache = cache_store.lock().unwrap();
                cache.clear();
                for t in &fetched_tracks {
                    cache.insert(t.id.clone(), t.clone());
                }
            }

            slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_w.upgrade() {
                    let mut slint_tracks = Vec::new();
                    for t in fetched_tracks {
                        let thumb_img = if let Some(ref path) = t.thumbnail_url {
                            load_image_robust(path).unwrap_or_default()
                        } else {
                            slint::Image::default()
                        };
                        let dur_secs = t.duration_secs.unwrap_or(0);
                        let dur_str = if dur_secs > 0 { format_time(dur_secs as f32) } else { "--:--".to_string() };

                        slint_tracks.push(Track {
                            id: t.id.into(),
                            title: t.title.into(),
                            artist: t.artist.unwrap_or_else(|| "Unknown".into()).into(),
                            duration: dur_str.into(),
                            thumbnail: thumb_img,
                        });
                    }
                    ui.set_search_results(ModelRc::from(Rc::new(VecModel::from(slint_tracks))));
                    ui.set_is_searching(false);
                }
            })
            .unwrap_or_default();
        });
    });

    // Spotify Sync Binding
    let h = handle.clone();
    let cl = client.clone();
    let base = daemon_base.clone();
    let ui_weak_sp = main_window.as_weak();
    main_window.on_sync_spotify(move |sp_input| {
        let input_str = sp_input.to_string();
        let cl = cl.clone();
        let base = base.clone();
        let ui_w = ui_weak_sp.clone();

        if let Some(ui) = ui_w.upgrade() {
            ui.set_is_syncing_spotify(true);
            ui.set_spotify_sync_status("Syncing with Spotify...".into());
        }

        h.spawn(async move {
            let trimmed = input_str.trim();
            let req_body = if trimmed.starts_with("BQ") || (trimmed.len() > 100 && !trimmed.contains("sp_dc")) {
                serde_json::json!({ "access_token": trimmed })
            } else {
                serde_json::json!({ "sp_dc": trimmed })
            };

            let res = cl.post(format!("{}/api/spotify/sync", base))
                .json(&req_body)
                .send()
                .await;

            let (success, msg) = match res {
                Ok(r) => {
                    if let Ok(json) = r.json::<serde_json::Value>().await {
                        let ok = json.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                        let msg = json.get("message").and_then(|v| v.as_str()).unwrap_or("Done").to_string();
                        (ok, msg)
                    } else {
                        (false, "Failed to parse Spotify response".to_string())
                    }
                }
                Err(e) => (false, format!("Network error: {}", e)),
            };

            let mut refreshed_playlists = Vec::new();
            if success {
                if let Ok(res) = cl.get(format!("{}/api/playlists", base)).send().await {
                    if let Ok(pls) = res.json::<Vec<arcy_music_core::Playlist>>().await {
                        refreshed_playlists = pls;
                    }
                }
            }

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_w.upgrade() {
                    ui.set_is_syncing_spotify(false);
                    ui.set_spotify_sync_status(msg.into());
                    if !refreshed_playlists.is_empty() {
                        let slint_pls: Vec<Playlist> = refreshed_playlists.into_iter().map(|p| Playlist {
                            id: p.id.into(),
                            name: p.name.into(),
                            track_count: p.track_count as i32,
                            is_pinned: p.is_pinned,
                            is_default: p.is_default,
                        }).collect();
                        ui.set_playlists(ModelRc::new(VecModel::from(slint_pls)));
                    }
                }
            });
        });
    });

    // Import from Browser Binding
    let h_import = handle.clone();
    let cl_import = client.clone();
    let base_import = daemon_base.clone();
    let ui_weak_import = main_window.as_weak();
    main_window.on_spotify_import_browser(move || {
        let cl = cl_import.clone();
        let base = base_import.clone();
        let ui_w = ui_weak_import.clone();

        if let Some(ui) = ui_w.upgrade() {
            ui.set_is_syncing_spotify(true);
            ui.set_spotify_sync_status("Importing session from browser...".into());
        }

        h_import.spawn(async move {
            let res = cl.post(format!("{}/api/spotify/import-browser", base)).send().await;
            let (success, sp_dc_opt, msg) = match res {
                Ok(r) => {
                    if let Ok(json) = r.json::<serde_json::Value>().await {
                        let ok = json.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                        let dc = json.get("sp_dc").and_then(|v| v.as_str()).map(|s| s.to_string());
                        let m = json.get("message").and_then(|v| v.as_str()).unwrap_or("Import completed").to_string();
                        (ok, dc, m)
                    } else {
                        (false, None, "Failed to parse daemon response".to_string())
                    }
                }
                Err(e) => (false, None, format!("Network error: {}", e)),
            };

            let mut refreshed_playlists = Vec::new();
            if success {
                tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;
                if let Ok(res) = cl.get(format!("{}/api/playlists", base)).send().await {
                    if let Ok(pls) = res.json::<Vec<arcy_music_core::Playlist>>().await {
                        refreshed_playlists = pls;
                    }
                }
            }

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_w.upgrade() {
                    ui.set_is_syncing_spotify(false);
                    if let Some(dc) = sp_dc_opt {
                        ui.set_spotify_sp_dc_input(dc.into());
                    }
                    ui.set_spotify_sync_status(msg.into());
                    if !refreshed_playlists.is_empty() {
                        let slint_pls: Vec<Playlist> = refreshed_playlists.into_iter().map(|p| Playlist {
                            id: p.id.into(),
                            name: p.name.into(),
                            track_count: p.track_count as i32,
                            is_pinned: p.is_pinned,
                            is_default: p.is_default,
                        }).collect();
                        ui.set_playlists(ModelRc::new(VecModel::from(slint_pls)));
                    }
                }
            });
        });
    });

    // Launch Spotify Login Window Binding
    let h_login = handle.clone();
    let cl_login = client.clone();
    let base_login = daemon_base.clone();
    let ui_weak_login = main_window.as_weak();
    main_window.on_spotify_launch_login(move || {
        let cl = cl_login.clone();
        let base = base_login.clone();
        let ui_w = ui_weak_login.clone();

        if let Some(ui) = ui_w.upgrade() {
            ui.set_is_logging_in_spotify(true);
            ui.set_spotify_sync_status("Opening Spotify login window...".into());
        }

        h_login.spawn(async move {
            let res = cl.post(format!("{}/api/spotify/launch-login", base)).send().await;
            match res {
                Ok(r) => {
                    if let Ok(json) = r.json::<serde_json::Value>().await {
                        let ok = json.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                        let m = json.get("message").and_then(|v| v.as_str()).unwrap_or("Waiting for login...").to_string();
                        if !ok {
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_w.upgrade() {
                                    ui.set_is_logging_in_spotify(false);
                                    ui.set_spotify_sync_status(m.into());
                                }
                            });
                            return;
                        }
                    }
                }
                Err(e) => {
                    let err_msg = format!("Network error: {}", e);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_w.upgrade() {
                            ui.set_is_logging_in_spotify(false);
                            ui.set_spotify_sync_status(err_msg.into());
                        }
                    });
                    return;
                }
            }

            // Poll for completion
            for _ in 0..200 {
                tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;
                let status_res = cl.get(format!("{}/api/spotify/login-status", base)).send().await;
                if let Ok(r) = status_res {
                    if let Ok(st) = r.json::<serde_json::Value>().await {
                        let status_val = st.get("status").and_then(|v| v.as_str()).unwrap_or("idle");
                        let msg_val = st.get("message").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let sp_dc_val = st.get("sp_dc").and_then(|v| v.as_str()).map(|s| s.to_string());

                        if status_val == "success" {
                            let mut refreshed_playlists = Vec::new();
                            if let Ok(res) = cl.get(format!("{}/api/playlists", base)).send().await {
                                if let Ok(pls) = res.json::<Vec<arcy_music_core::Playlist>>().await {
                                    refreshed_playlists = pls;
                                }
                            }
                            let ui_w2 = ui_w.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_w2.upgrade() {
                                    ui.set_is_logging_in_spotify(false);
                                    if let Some(dc) = sp_dc_val {
                                        ui.set_spotify_sp_dc_input(dc.into());
                                    }
                                    ui.set_spotify_sync_status(msg_val.into());
                                    if !refreshed_playlists.is_empty() {
                                        let slint_pls: Vec<Playlist> = refreshed_playlists.into_iter().map(|p| Playlist {
                                            id: p.id.into(),
                                            name: p.name.into(),
                                            track_count: p.track_count as i32,
                                            is_pinned: p.is_pinned,
                                            is_default: p.is_default,
                                        }).collect();
                                        ui.set_playlists(ModelRc::new(VecModel::from(slint_pls)));
                                    }
                                }
                            });
                            break;
                        } else if status_val == "error" {
                            let ui_w2 = ui_w.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_w2.upgrade() {
                                    ui.set_is_logging_in_spotify(false);
                                    ui.set_spotify_sync_status(msg_val.into());
                                }
                            });
                            break;
                        }
                    }
                }
            }
        });
    });

    // Play & Enqueue Search Track
    let h = handle.clone();
    let cl = client.clone();
    let base = daemon_base.clone();
    let search_cache_play = cached_search_results.clone();
    main_window.on_play_search_track(move |id| {
        let id_str = id.to_string();
        println!("GUI on_play_search_track clicked for id: {}", id_str);
        let track_opt = {
            let cache = search_cache_play.lock().unwrap();
            cache.get(&id_str).cloned()
        };
        let track = if let Some(t) = track_opt {
            t
        } else if id_str.starts_with("sp_") {
            let track_id = id_str.strip_prefix("sp_").unwrap_or(&id_str).to_string();
            CoreTrack {
                id: id_str.clone(),
                title: "Spotify Track".to_string(),
                artist: None,
                album: None,
                duration_secs: None,
                source: TrackSource::Spotify { track_id },
                thumbnail_url: None,
            }
        } else {
            let video_id = id_str.strip_prefix("yt:").unwrap_or(&id_str).to_string();
            CoreTrack {
                id: id_str.clone(),
                title: "Online Track".to_string(),
                artist: None,
                album: None,
                duration_secs: None,
                source: TrackSource::YouTube { video_id },
                thumbnail_url: None,
            }
        };

        let cl = cl.clone();
        let base = base.clone();
        h.spawn(async move {
            let _ = cl
                .post(format!("{}/api/command", base))
                .json(&Command::PlayTrack(track))
                .send()
                .await;
        });
    });

    let h = handle.clone();
    let cl = client.clone();
    let base = daemon_base.clone();
    let search_cache_enq = cached_search_results.clone();
    main_window.on_enqueue_track(move |id| {
        let id_str = id.to_string();
        let track_opt = {
            let cache = search_cache_enq.lock().unwrap();
            cache.get(&id_str).cloned()
        };
        let track = if let Some(t) = track_opt {
            t
        } else if id_str.starts_with("sp_") {
            let track_id = id_str.strip_prefix("sp_").unwrap_or(&id_str).to_string();
            CoreTrack {
                id: id_str.clone(),
                title: "Spotify Track".to_string(),
                artist: None,
                album: None,
                duration_secs: None,
                source: TrackSource::Spotify { track_id },
                thumbnail_url: None,
            }
        } else {
            let video_id = id_str.strip_prefix("yt:").unwrap_or(&id_str).to_string();
            CoreTrack {
                id: id_str.clone(),
                title: "Online Track".to_string(),
                artist: None,
                album: None,
                duration_secs: None,
                source: TrackSource::YouTube { video_id },
                thumbnail_url: None,
            }
        };

        let cl = cl.clone();
        let base = base.clone();
        h.spawn(async move {
            let _ = cl
                .post(format!("{}/api/command", base))
                .json(&Command::EnqueueTrack(track))
                .send()
                .await;
        });
    });

    let h = handle.clone();
    let cl = client.clone();
    let base = daemon_base.clone();
    let search_cache_dl = cached_search_results.clone();
    main_window.on_download_track(move |id| {
        let id_str = id.to_string();
        let track_opt = {
            let cache = search_cache_dl.lock().unwrap();
            cache.get(&id_str).cloned()
        };
        if let Some(track) = track_opt {
            let cl = cl.clone();
            let base = base.clone();
            h.spawn(async move {
                let _ = cl
                    .post(format!("{}/api/command", base))
                    .json(&Command::DownloadTrack(track))
                    .send()
                    .await;
            });
        }
    });

    // Settings Callbacks
    let h = handle.clone();
    let cl = client.clone();
    let base = daemon_base.clone();
    let ui_weak_thumb = main_window.as_weak();
    main_window.on_toggle_mini_thumbnails(move || {
        if let Some(ui) = ui_weak_thumb.upgrade() {
            let val = if ui.get_setting_mini_thumbnails() { "1" } else { "0" };
            let cl = cl.clone();
            let base = base.clone();
            h.spawn(async move {
                let body = serde_json::json!({ "key": "mini_thumbnails", "value": val });
                let _ = cl.post(format!("{}/api/settings", base)).json(&body).send().await;
            });
        }
    });

    let h = handle.clone();
    let cl = client.clone();
    let base = daemon_base.clone();
    main_window.on_update_download_path(move |path| {
        let path_str = path.to_string();
        let cl = cl.clone();
        let base = base.clone();
        h.spawn(async move {
            let body = serde_json::json!({ "key": "download_path", "value": path_str });
            let _ = cl.post(format!("{}/api/settings", base)).json(&body).send().await;
        });
    });

    println!(
        "Arcy Desktop Package GUI loaded successfully (Mode: {}).",
        if is_dashboard_mode { "Dashboard Overlay" } else { "Window" }
    );
    main_window.run()?;
    Ok(())
}
