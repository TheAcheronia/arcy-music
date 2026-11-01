use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Track {
    pub id: String,          // Unique ID (e.g., local file path hash, or youtube ID)
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_secs: Option<u64>,
    pub source: TrackSource,
    pub thumbnail_url: Option<String>, // Path to local cache or URL
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TrackSource {
    Local { path: String },
    YouTube { video_id: String },
    Spotify { track_id: String }, // Spotify just for metadata, audio usually fetched via YouTube
    SoundCloud { url: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EqConfig {
    pub enabled: bool,
    #[serde(default)]
    pub preamp_db: f32,
    pub bands: [f32; 5], // 60Hz, 250Hz, 1kHz, 4kHz, 12kHz in dB (-12.0 to +12.0)
    #[serde(default = "default_spatial_mode")]
    pub spatial_mode: String, // "off" (default bypass), "dolby_atmos", "eclipsa"
}

fn default_spatial_mode() -> String {
    "off".to_string()
}

impl Default for EqConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            preamp_db: 0.0,
            bands: [0.0, 0.0, 0.0, 0.0, 0.0],
            spatial_mode: "off".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackState {
    pub current_track: Option<Track>,
    pub is_playing: bool,
    pub volume: f32,
    pub position_secs: f32,
    pub duration_secs: f32,
    pub queue: Vec<Track>,
    pub queue_index: usize,
    pub shuffle: bool,
    pub repeat_mode: RepeatMode,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            current_track: None,
            is_playing: false,
            volume: 0.15,
            position_secs: 0.0,
            duration_secs: 0.0,
            queue: Vec::new(),
            queue_index: 0,
            shuffle: false,
            repeat_mode: RepeatMode::Off,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub track_count: usize,
    pub is_pinned: bool,
    pub last_played: i64,
    pub sort_order: i32,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    Play,
    Pause,
    PlayPause,
    Next,
    Previous,
    Stop,
    SetVolume(f32),
    Seek(f32),
    SeekRelative(f32),
    PlayTrack(Track),
    PlayTrackId(String),
    SetQueueAndPlay { tracks: Vec<Track>, start_index: usize },
    PlayPlaylist { playlist_id: String, start_index: usize },
    EnqueueTrack(Track),
    RemoveFromQueue(usize),
    ClearQueue,
    SetShuffle(bool),
    SetRepeat(RepeatMode),
    ToggleShuffle,
    ToggleRepeat,
    SetEq(EqConfig),
    DownloadTrack(Track),
}

pub mod fetcher;
pub mod dsp;
pub mod theme;

pub fn is_valid_youtube_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 32 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

