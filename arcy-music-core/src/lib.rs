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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostInfo {
    pub os: String,
    pub host: String,
    pub kernel: String,
    pub uptime: String,
    pub shell: String,
    pub wm: String,
    pub cpu: String,
    pub gpu: String,
    pub packages: String,
}

impl Default for HostInfo {
    fn default() -> Self {
        Self {
            os: "Arch Linux".to_string(),
            host: "PC".to_string(),
            kernel: "Linux".to_string(),
            uptime: "0m".to_string(),
            shell: "zsh".to_string(),
            wm: "AwesomeWM".to_string(),
            cpu: "CPU".to_string(),
            gpu: "NVIDIA GPU".to_string(),
            packages: "pacman".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStatus {
    pub cpu_usage: f32,
    pub ram_usage: f32,
    pub ram_used_mb: u64,
    pub ram_total_mb: u64,
    pub disk_usage: String,
    pub temp_celsius: i32,
    pub power_profile: String,
    pub night_mode: String,
    pub vsync: bool,
    pub gamemode: bool,
    pub gpu_profile: String,
    pub network_status: String,
    pub network_ssid: String,
    pub master_volume: u32,
    pub master_muted: bool,
    pub brightness: u32,
    pub quote: String,
    pub weather: String,
    pub active_theme: String,
    pub uptime: String,
    pub host: HostInfo,
}

impl Default for SystemStatus {
    fn default() -> Self {
        Self {
            cpu_usage: 0.0,
            ram_usage: 0.0,
            ram_used_mb: 0,
            ram_total_mb: 16384,
            disk_usage: "0%".to_string(),
            temp_celsius: 40,
            power_profile: "balanced".to_string(),
            night_mode: "off".to_string(),
            vsync: true,
            gamemode: false,
            gpu_profile: "default".to_string(),
            network_status: "normal".to_string(),
            network_ssid: "Online".to_string(),
            master_volume: 50,
            master_muted: false,
            brightness: 50,
            quote: "Talk is cheap. Show me the code. - Linus Torvalds".to_string(),
            weather: "⛅ Online".to_string(),
            active_theme: "mocha".to_string(),
            uptime: "0m".to_string(),
            host: HostInfo::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SystemCommand {
    SetPowerProfile(String),
    CyclePowerProfile,
    SetNightMode(String),
    CycleNightMode,
    SetGpuProfile(String),
    CycleGpuProfile,
    ToggleGamemode,
    ToggleVsync,
    ToggleWarp,
    SetMasterVolume(u32),
    ToggleMasterMute,
    SetBrightness(u32),
    RefreshQuote,
    SetTheme(String),
    LaunchApp(String),
    SessionLock,
    SessionSleep,
    SessionReboot,
    SessionPoweroff,
}

pub mod fetcher;
pub mod dsp;
pub mod theme;

pub fn is_valid_youtube_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 32 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

