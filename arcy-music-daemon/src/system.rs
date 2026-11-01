use arcy_music_core::{HostInfo, SystemCommand, SystemStatus};
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

static PREV_IDLE: AtomicU64 = AtomicU64::new(0);
static PREV_TOTAL: AtomicU64 = AtomicU64::new(0);

pub struct SystemManager {
    status: Arc<RwLock<SystemStatus>>,
    #[allow(dead_code)]
    host_info: HostInfo,
}

impl SystemManager {
    pub fn new() -> Self {
        let host_info = Self::detect_host_info();
        let mut initial_status = SystemStatus::default();
        initial_status.host = host_info.clone();

        Self {
            status: Arc::new(RwLock::new(initial_status)),
            host_info,
        }
    }

    pub fn get_status_handle(&self) -> Arc<RwLock<SystemStatus>> {
        Arc::clone(&self.status)
    }

    fn detect_host_info() -> HostInfo {
        let os = fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|content| {
                for line in content.lines() {
                    if let Some(val) = line.strip_prefix("PRETTY_NAME=") {
                        return Some(val.trim_matches('"').to_string());
                    }
                }
                None
            })
            .unwrap_or_else(|| "Arch Linux".to_string());

        let kernel = fs::read_to_string("/proc/sys/kernel/osrelease")
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "Linux".to_string());

        let host = fs::read_to_string("/proc/sys/kernel/hostname")
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "localhost".to_string());

        let cpu = fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|content| {
                for line in content.lines() {
                    if line.starts_with("model name") {
                        if let Some((_, val)) = line.split_once(':') {
                            return Some(val.trim().to_string());
                        }
                    }
                }
                None
            })
            .unwrap_or_else(|| "Unknown CPU".to_string());

        let gpu = if let Ok(out) = std::process::Command::new("lspci").output() {
            let text = String::from_utf8_lossy(&out.stdout);
            text.lines()
                .find(|l| l.contains("VGA compatible controller") || l.contains("3D controller"))
                .map(|l| {
                    if let Some((_, rest)) = l.split_once(": ") {
                        rest.to_string()
                    } else {
                        l.to_string()
                    }
                })
                .unwrap_or_else(|| "NVIDIA GPU".to_string())
        } else {
            "NVIDIA GPU".to_string()
        };

        HostInfo {
            os,
            host,
            kernel,
            uptime: "0m".to_string(),
            shell: std::env::var("SHELL").unwrap_or_else(|_| "zsh".to_string()),
            wm: "AwesomeWM".to_string(),
            cpu,
            gpu,
            packages: "pacman".to_string(),
        }
    }

    pub fn read_cpu_usage() -> f32 {
        if let Ok(content) = fs::read_to_string("/proc/stat") {
            if let Some(first_line) = content.lines().next() {
                let parts: Vec<&str> = first_line.split_whitespace().collect();
                if parts.len() >= 5 {
                    let user: u64 = parts[1].parse().unwrap_or(0);
                    let nice: u64 = parts[2].parse().unwrap_or(0);
                    let system: u64 = parts[3].parse().unwrap_or(0);
                    let idle: u64 = parts[4].parse().unwrap_or(0);
                    let iowait: u64 = parts.get(5).and_then(|s| s.parse().ok()).unwrap_or(0);
                    let irq: u64 = parts.get(6).and_then(|s| s.parse().ok()).unwrap_or(0);
                    let softirq: u64 = parts.get(7).and_then(|s| s.parse().ok()).unwrap_or(0);

                    let total = user + nice + system + idle + iowait + irq + softirq;
                    let idle_total = idle + iowait;

                    let prev_idle = PREV_IDLE.swap(idle_total, Ordering::Relaxed);
                    let prev_total = PREV_TOTAL.swap(total, Ordering::Relaxed);

                    if prev_total > 0 && total > prev_total {
                        let total_diff = (total - prev_total) as f32;
                        let idle_diff = (idle_total.saturating_sub(prev_idle)) as f32;
                        let usage = 100.0 * (1.0 - (idle_diff / total_diff));
                        return usage.clamp(0.0, 100.0);
                    }
                }
            }
        }
        0.0
    }

    pub fn read_ram() -> (f32, u64, u64) {
        if let Ok(content) = fs::read_to_string("/proc/meminfo") {
            let mut total_kb = 0u64;
            let mut avail_kb = 0u64;

            for line in content.lines() {
                if line.starts_with("MemTotal:") {
                    if let Some(val) = line.split_whitespace().nth(1) {
                        total_kb = val.parse().unwrap_or(0);
                    }
                } else if line.starts_with("MemAvailable:") {
                    if let Some(val) = line.split_whitespace().nth(1) {
                        avail_kb = val.parse().unwrap_or(0);
                    }
                }
            }

            if total_kb > 0 {
                let used_kb = total_kb.saturating_sub(avail_kb);
                let pct = (used_kb as f32 / total_kb as f32) * 100.0;
                let used_mb = used_kb / 1024;
                let total_mb = total_kb / 1024;
                return (pct.clamp(0.0, 100.0), used_mb, total_mb);
            }
        }
        (0.0, 0, 16384)
    }

    pub fn read_disk_usage() -> String {
        if let Ok(out) = std::process::Command::new("df").args(["-h", "/"]).output() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines().skip(1) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 5 {
                    return parts[4].to_string();
                }
            }
        }
        "N/A".to_string()
    }

    pub fn read_temp() -> i32 {
        // Check hwmon or thermal_zone
        for i in 0..6 {
            let path = format!("/sys/class/thermal/thermal_zone{}/temp", i);
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(val) = content.trim().parse::<i32>() {
                    return val / 1000;
                }
            }
        }
        // Fallback to sensors
        if let Ok(out) = std::process::Command::new("sensors").output() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                if line.contains("Package id 0") || line.contains("Core 0") || line.contains("temp1:") {
                    if let Some((_, temp_str)) = line.split_once('+') {
                        if let Some((num, _)) = temp_str.split_once('°') {
                            if let Ok(val) = num.trim().parse::<f32>() {
                                return val as i32;
                            }
                        }
                    }
                }
            }
        }
        40
    }

    pub fn read_uptime() -> String {
        if let Ok(content) = fs::read_to_string("/proc/uptime") {
            if let Some(first) = content.split_whitespace().next() {
                if let Ok(secs) = first.parse::<f64>() {
                    let total_mins = (secs / 60.0) as u64;
                    let hours = total_mins / 60;
                    let mins = total_mins % 60;
                    if hours > 0 {
                        return format!("{}h {}m", hours, mins);
                    } else {
                        return format!("{}m", mins);
                    }
                }
            }
        }
        "0m".to_string()
    }

    pub fn read_power_profile() -> String {
        if let Ok(out) = std::process::Command::new("powerprofilesctl").arg("get").output() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                return s;
            }
        }
        "balanced".to_string()
    }

    pub fn read_night_mode() -> String {
        fs::read_to_string("/tmp/arcy_night_mode")
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "off".to_string())
    }

    pub fn read_vsync() -> bool {
        fs::read_to_string("/tmp/arcy_vsync")
            .map(|s| s.trim() == "on")
            .unwrap_or(true)
    }

    pub fn read_gamemode() -> bool {
        std::path::Path::new("/tmp/arcy_gamemode").exists()
    }

    pub fn read_gpu_profile() -> String {
        if let Ok(content) = fs::read_to_string("/tmp/arcy_gpu_profile") {
            let s = content.trim().to_string();
            if !s.is_empty() {
                return s;
            }
        }
        "default".to_string()
    }

    pub fn read_network() -> (String, String) {
        // Check warp
        let is_warp = if let Ok(out) = std::process::Command::new("warp-cli").arg("status").output() {
            let text = String::from_utf8_lossy(&out.stdout);
            text.contains("Connected")
        } else {
            false
        };

        if is_warp {
            return ("warp".to_string(), "Cloudflare WARP".to_string());
        }

        // Check active SSID
        if let Ok(out) = std::process::Command::new("nmcli")
            .args(["-t", "-f", "active,ssid", "dev", "wifi"])
            .output()
        {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                if line.starts_with("yes:") {
                    let ssid = line.trim_start_matches("yes:").trim().to_string();
                    if !ssid.is_empty() {
                        return ("wifi".to_string(), ssid);
                    }
                }
            }
        }

        ("normal".to_string(), "Online".to_string())
    }

    pub fn read_audio_master() -> (u32, bool) {
        let mut vol = 50u32;
        let mut muted = false;

        if let Ok(out) = std::process::Command::new("pactl")
            .args(["get-sink-volume", "@DEFAULT_SINK@"])
            .output()
        {
            let text = String::from_utf8_lossy(&out.stdout);
            if let Some(pct_idx) = text.find('%') {
                let start = text[..pct_idx].rfind(|c: char| !c.is_ascii_digit()).map(|i| i + 1).unwrap_or(0);
                if let Ok(v) = text[start..pct_idx].parse::<u32>() {
                    vol = v;
                }
            }
        }

        if let Ok(out) = std::process::Command::new("pactl")
            .args(["get-sink-mute", "@DEFAULT_SINK@"])
            .output()
        {
            let text = String::from_utf8_lossy(&out.stdout);
            muted = text.contains("yes");
        }

        (vol, muted)
    }

    pub fn read_brightness() -> u32 {
        if let Ok(out) = std::process::Command::new("brightnessctl").arg("i").output() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                if line.contains("Current brightness:") {
                    if let Some(pct_idx) = line.find('%') {
                        let start = line[..pct_idx].rfind(|c: char| !c.is_ascii_digit()).map(|i| i + 1).unwrap_or(0);
                        if let Ok(val) = line[start..pct_idx].parse::<u32>() {
                            return val;
                        }
                    }
                }
            }
        }
        50
    }

    pub fn get_random_quote() -> String {
        let quotes = [
            "Talk is cheap. Show me the code. - Linus Torvalds",
            "I use Arch btw.",
            "Any fool can write code that a computer can understand.",
            "First, solve the problem. Then, write the code.",
            "Experience is the name everyone gives to their mistakes.",
            "In order to be irreplaceable, one must always be different.",
            "Code is like humor. When you have to explain it, it's bad.",
            "Fix the cause, not the symptom.",
            "Simplicity is the ultimate sophistication.",
            "A smooth sea never made a skilled sailor.",
            "The quieter you become, the more you are able to hear.",
        ];
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
        let idx = (nanos as usize) % quotes.len();
        quotes[idx].to_string()
    }

    pub fn read_active_theme() -> String {
        let config_dir = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
        let theme_file = config_dir.join("arcy/theme.json");
        if let Ok(content) = fs::read_to_string(&theme_file) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(t) = val.get("theme").and_then(|v| v.as_str()) {
                    return t.to_string();
                }
            }
        }
        "mocha".to_string()
    }

    pub async fn poll_and_update(&self) {
        let cpu = Self::read_cpu_usage();
        let (ram, used_mb, total_mb) = Self::read_ram();
        let disk = Self::read_disk_usage();
        let temp = Self::read_temp();
        let uptime = Self::read_uptime();
        let power = Self::read_power_profile();
        let night = Self::read_night_mode();
        let vsync = Self::read_vsync();
        let gamemode = Self::read_gamemode();
        let gpu = Self::read_gpu_profile();
        let (net_status, net_ssid) = Self::read_network();
        let (vol, muted) = Self::read_audio_master();
        let bright = Self::read_brightness();
        let theme = Self::read_active_theme();

        let mut st = self.status.write().await;
        st.cpu_usage = cpu;
        st.ram_usage = ram;
        st.ram_used_mb = used_mb;
        st.ram_total_mb = total_mb;
        st.disk_usage = disk;
        st.temp_celsius = temp;
        st.uptime = uptime;
        st.power_profile = power;
        st.night_mode = night;
        st.vsync = vsync;
        st.gamemode = gamemode;
        st.gpu_profile = gpu;
        st.network_status = net_status;
        st.network_ssid = net_ssid;
        st.master_volume = vol;
        st.master_muted = muted;
        st.brightness = bright;
        st.active_theme = theme;
    }

    pub async fn apply_night_mode(mode: &str) {
        let displays = if let Ok(out) = std::process::Command::new("xrandr").output() {
            let text = String::from_utf8_lossy(&out.stdout);
            text.lines()
                .filter(|l| l.contains(" connected"))
                .filter_map(|l| l.split_whitespace().next())
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        let brightness = match mode {
            "evening" => "0.8",
            "night" => "0.6",
            _ => "1.0",
        };

        for d in displays {
            let _ = tokio::process::Command::new("xrandr")
                .args(["--output", &d, "--brightness", brightness])
                .output()
                .await;
        }

        if mode == "off" {
            let _ = fs::remove_file("/tmp/arcy_night_mode");
        } else {
            let _ = fs::write("/tmp/arcy_night_mode", mode);
        }
    }

    pub async fn apply_gpu_profile(profile: &str) {
        let (core, mem) = match profile {
            "turbo" => ("400", "390"),
            "oc" => ("350", "350"),
            _ => ("0", "0"),
        };

        let core_arg = format!("[gpu:0]/GPUGraphicsClockOffset[1]={}", core);
        let mem_arg = format!("[gpu:0]/GPUMemoryTransferRateOffset[1]={}", mem);

        let _ = tokio::process::Command::new("nvidia-settings")
            .env("DISPLAY", ":0")
            .args(["-a", &core_arg, "-a", &mem_arg])
            .output()
            .await;

        let _ = fs::write("/tmp/arcy_gpu_profile", profile);
    }

    pub async fn handle_command(&self, cmd: SystemCommand) -> Result<(), String> {
        match cmd {
            SystemCommand::SetPowerProfile(profile) => {
                let valid = ["power-saver", "balanced", "performance"];
                if valid.contains(&profile.as_str()) {
                    let _ = tokio::process::Command::new("powerprofilesctl")
                        .args(["set", &profile])
                        .output()
                        .await;
                }
            }
            SystemCommand::CyclePowerProfile => {
                let curr = Self::read_power_profile();
                let next = match curr.as_str() {
                    "power-saver" => "balanced",
                    "balanced" => "performance",
                    _ => "power-saver",
                };
                let _ = tokio::process::Command::new("powerprofilesctl")
                    .args(["set", next])
                    .output()
                    .await;
            }
            SystemCommand::SetNightMode(mode) => {
                Self::apply_night_mode(&mode).await;
            }
            SystemCommand::CycleNightMode => {
                let curr = Self::read_night_mode();
                let next = match curr.as_str() {
                    "off" => "evening",
                    "evening" => "night",
                    _ => "off",
                };
                Self::apply_night_mode(next).await;
            }
            SystemCommand::SetGpuProfile(profile) => {
                Self::apply_gpu_profile(&profile).await;
            }
            SystemCommand::CycleGpuProfile => {
                let curr = Self::read_gpu_profile();
                let next = match curr.as_str() {
                    "default" => "oc",
                    "oc" => "turbo",
                    _ => "default",
                };
                Self::apply_gpu_profile(next).await;
            }
            SystemCommand::ToggleGamemode => {
                let is_on = Self::read_gamemode();
                let home = std::env::var("HOME").unwrap_or_else(|_| dirs::home_dir().map(|h| h.to_string_lossy().to_string()).unwrap_or_else(|| "/tmp".to_string()));
                let conf = format!("{}/.config/awesome/configuration/picom.conf", home);
                let game_conf = format!("{}/.config/awesome/configuration/picom-game.conf", home);

                if is_on {
                    let _ = fs::remove_file("/tmp/arcy_gamemode");
                    let _ = tokio::process::Command::new("powerprofilesctl").args(["set", "balanced"]).output().await;
                    let _ = tokio::process::Command::new("killall").args(["-9", "picom"]).output().await;
                    let _ = tokio::process::Command::new("picom").args(["--config", &conf, "-b"]).spawn();
                } else {
                    let _ = fs::write("/tmp/arcy_gamemode", "on");
                    let _ = tokio::process::Command::new("powerprofilesctl").args(["set", "performance"]).output().await;
                    let _ = tokio::process::Command::new("killall").args(["-9", "picom"]).output().await;
                    let _ = tokio::process::Command::new("picom").args(["--config", &game_conf, "-b"]).spawn();
                    let _ = tokio::process::Command::new("gamemoded").arg("-r").output().await;
                }
            }
            SystemCommand::ToggleVsync => {
                let curr_on = Self::read_vsync();
                let next = if curr_on { "off" } else { "on" };
                let _ = fs::write("/tmp/arcy_vsync", next);

                let home = std::env::var("HOME").unwrap_or_else(|_| dirs::home_dir().map(|h| h.to_string_lossy().to_string()).unwrap_or_else(|| "/tmp".to_string()));
                let conf = format!("{}/.config/awesome/configuration/picom.conf", home);
                let game_conf = format!("{}/.config/awesome/configuration/picom-game.conf", home);

                let (from, to) = if curr_on {
                    ("vsync = true;", "vsync = false;")
                } else {
                    ("vsync = false;", "vsync = true;")
                };

                for p in &[&conf, &game_conf] {
                    if let Ok(content) = fs::read_to_string(p) {
                        let replaced = content.replace(from, to);
                        let _ = fs::write(p, replaced);
                    }
                }

                let target_conf = if Self::read_gamemode() { &game_conf } else { &conf };
                let _ = tokio::process::Command::new("killall").args(["-9", "picom"]).output().await;
                let _ = tokio::process::Command::new("picom").args(["--config", target_conf, "-b"]).spawn();
            }
            SystemCommand::ToggleWarp => {
                let (status, _) = Self::read_network();
                if status == "warp" {
                    let _ = tokio::process::Command::new("warp-cli").arg("disconnect").output().await;
                } else {
                    let _ = tokio::process::Command::new("warp-cli").arg("connect").output().await;
                }
            }
            SystemCommand::SetMasterVolume(vol) => {
                let clamped = vol.clamp(0, 150);
                let arg = format!("{}%", clamped);
                let _ = tokio::process::Command::new("pactl")
                    .args(["set-sink-volume", "@DEFAULT_SINK@", &arg])
                    .output()
                    .await;
            }
            SystemCommand::ToggleMasterMute => {
                let _ = tokio::process::Command::new("pactl")
                    .args(["set-sink-mute", "@DEFAULT_SINK@", "toggle"])
                    .output()
                    .await;
            }
            SystemCommand::SetBrightness(val) => {
                let clamped = val.clamp(1, 100);
                let arg = format!("{}%", clamped);
                let _ = tokio::process::Command::new("brightnessctl")
                    .args(["s", &arg])
                    .output()
                    .await;
            }
            SystemCommand::RefreshQuote => {
                let new_quote = Self::get_random_quote();
                let mut st = self.status.write().await;
                st.quote = new_quote;
            }
            SystemCommand::SetTheme(theme) => {
                let config_dir = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("/tmp")).join("arcy");
                let _ = fs::create_dir_all(&config_dir);
                let theme_file = config_dir.join("theme.json");
                let data = serde_json::json!({ "theme": theme });
                let _ = fs::write(&theme_file, data.to_string());

                let mut st = self.status.write().await;
                st.active_theme = theme.clone();

                // Trigger theme change hook script if present
                let hook_path = config_dir.join("hooks/on_theme_change.sh");
                if std::path::Path::new(&hook_path).exists() {
                    let _ = tokio::process::Command::new("bash")
                        .arg(&hook_path)
                        .arg(&theme)
                        .spawn();
                }
            }
            SystemCommand::LaunchApp(app) => {
                let safe_cmd = match app.as_str() {
                    "brave" => Some("brave"),
                    "terminal" | "alacritty" => Some("alacritty"),
                    "code" | "vscode" => Some("code"),
                    "thunar" | "files" => Some("thunar"),
                    "kitty" => Some("kitty"),
                    _ => None,
                };
                if let Some(cmd) = safe_cmd {
                    let _ = tokio::process::Command::new(cmd).spawn();
                }
            }
            SystemCommand::SessionLock => {
                let _ = tokio::process::Command::new("loginctl").arg("lock-session").output().await;
            }
            SystemCommand::SessionSleep => {
                let _ = tokio::process::Command::new("systemctl").arg("suspend").output().await;
            }
            SystemCommand::SessionReboot => {
                let _ = tokio::process::Command::new("systemctl").arg("reboot").output().await;
            }
            SystemCommand::SessionPoweroff => {
                let _ = tokio::process::Command::new("systemctl").arg("poweroff").output().await;
            }
        }

        // Refresh immediately after command
        self.poll_and_update().await;
        Ok(())
    }
}
