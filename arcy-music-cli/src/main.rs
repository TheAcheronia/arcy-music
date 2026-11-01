use clap::{Parser, Subcommand};
use arcy_music_core::{Command as CoreCommand, PlaybackState, SystemCommand, SystemStatus};
use reqwest::Client;

#[derive(Parser)]
#[command(author, version, about = "Fast CLI controller for Arcy Desktop & Music Core", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(short, long, default_value = "http://127.0.0.1:13338")]
    daemon_url: String,
}

#[derive(Subcommand)]
enum Commands {
    // Media Playback
    /// Play the current track
    Play,
    /// Pause the current track
    Pause,
    /// Stop playback entirely
    Stop,
    /// Toggle play / pause
    Toggle,
    /// Skip to next track in queue
    Next,
    /// Return to previous track in queue
    Prev,
    /// Toggle shuffle mode
    Shuffle,
    /// Toggle repeat mode (Off -> All -> One)
    Repeat,
    /// Seek to a specific timestamp in seconds
    Seek { seconds: f32 },
    /// Set music volume level (0.0 to 1.0)
    Volume { level: f32 },
    /// Print synchronized lyrics for the active track
    Lyrics,
    /// Get current playback status and track metadata
    Status,

    // System & Hardware Controls
    /// Print comprehensive system diagnostics, telemetry, and quick toggles
    Sys,
    /// Get or set power profile (power-saver, balanced, performance)
    Power { mode: Option<String> },
    /// Get or set night mode / display brightness (off, evening, night)
    Night { mode: Option<String> },
    /// Get or set GPU profile (default, oc, turbo)
    Gpu { profile: Option<String> },
    /// Toggle Game Mode (power profile + lightweight picom + feral gamemode)
    Game,
    /// Toggle display compositor VSync
    Vsync,
    /// Toggle Cloudflare WARP VPN tunnel
    Warp,
    /// Set system master volume (0 to 100)
    MasterVol { level: u32 },
    /// Toggle master audio mute
    MasterMute,
    /// Set screen brightness level (0 to 100)
    Bright { level: u32 },
    /// Get or switch Arcy theme (mocha, nord, tokyo, cyberpunk, dracula, gruvbox)
    Theme { name: Option<String> },
    /// Launch quick application (brave, terminal, code, thunar)
    App { name: String },
    /// Launch Arcy in Floating Dashboard Overlay mode
    Dashboard,
    /// Launch Arcy in Full Application Window mode
    Gui,
    /// Lock desktop session
    Lock,
    /// Suspend / Sleep system
    Sleep,
    /// Reboot system
    Reboot,
    /// Power off system
    Poweroff,
    /// Update Arcy Music & Desktop to the latest version while preserving all logins and playlists
    Update,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let client = Client::new();

    match cli.command {
        // Media Playback
        Commands::Play => send_audio_cmd(&client, &cli.daemon_url, CoreCommand::Play).await?,
        Commands::Pause => send_audio_cmd(&client, &cli.daemon_url, CoreCommand::Pause).await?,
        Commands::Stop => send_audio_cmd(&client, &cli.daemon_url, CoreCommand::Stop).await?,
        Commands::Toggle => send_audio_cmd(&client, &cli.daemon_url, CoreCommand::PlayPause).await?,
        Commands::Next => send_audio_cmd(&client, &cli.daemon_url, CoreCommand::Next).await?,
        Commands::Prev => send_audio_cmd(&client, &cli.daemon_url, CoreCommand::Previous).await?,
        Commands::Shuffle => send_audio_cmd(&client, &cli.daemon_url, CoreCommand::ToggleShuffle).await?,
        Commands::Repeat => send_audio_cmd(&client, &cli.daemon_url, CoreCommand::ToggleRepeat).await?,
        Commands::Seek { seconds } => send_audio_cmd(&client, &cli.daemon_url, CoreCommand::Seek(seconds)).await?,
        Commands::Volume { level } => send_audio_cmd(&client, &cli.daemon_url, CoreCommand::SetVolume(level)).await?,
        Commands::Status => {
            let res = client.get(format!("{}/api/state", cli.daemon_url)).send().await?;
            if res.status().is_success() {
                let state: PlaybackState = res.json().await?;
                if let Some(t) = state.current_track {
                    println!("🎵 Now Playing: {} - {}", t.title, t.artist.unwrap_or_else(|| "Unknown".into()));
                    println!("⏱  Position: {:.1}s / {:.1}s", state.position_secs, state.duration_secs);
                    println!("🔊 Volume: {:.0}%", state.volume * 100.0);
                    println!("🔀 Shuffle: {} | 🔁 Repeat: {:?}", if state.shuffle { "ON" } else { "OFF" }, state.repeat_mode);
                    println!("📜 Queue: {} tracks", state.queue.len());
                } else {
                    println!("No track currently playing.");
                }
            } else {
                eprintln!("Daemon not responding: HTTP {}", res.status());
            }
        }
        Commands::Lyrics => {
            let res = client.get(format!("{}/api/state", cli.daemon_url)).send().await?;
            if res.status().is_success() {
                let state: PlaybackState = res.json().await?;
                if let Some(t) = state.current_track {
                    let lyr_res = client.get(format!("{}/api/lyrics/{}", cli.daemon_url, t.id)).send().await?;
                    if lyr_res.status().is_success() {
                        let lyrics_val: serde_json::Value = lyr_res.json().await?;
                        if let Some(plain) = lyrics_val["plain_lyrics"].as_str() {
                            println!("=== Lyrics: {} ===\n", t.title);
                            println!("{}", plain);
                        } else if let Some(synced) = lyrics_val["synced_lyrics"].as_array() {
                            println!("=== Synced Lyrics: {} ===\n", t.title);
                            for line in synced {
                                println!("[{:.1}s] {}", line["time_secs"].as_f64().unwrap_or(0.0), line["text"].as_str().unwrap_or(""));
                            }
                        } else {
                            println!("No lyrics available for this track.");
                        }
                    }
                }
            }
        }

        // System Controls
        Commands::Sys => {
            let res = client.get(format!("{}/api/system/status", cli.daemon_url)).send().await?;
            if res.status().is_success() {
                let st: SystemStatus = res.json().await?;
                println!("╔═══════════════════════════════════════════════════╗");
                println!("║                 ARCY SYSTEM STATUS                ║");
                println!("╠═══════════════════════════════════════════════════╣");
                println!("║ Host:     {:<39} ║", format!("{} ({})", st.host.host, st.host.os));
                println!("║ Kernel:   {:<39} ║", st.host.kernel);
                println!("║ Uptime:   {:<39} ║", st.uptime);
                println!("║ CPU:      {:<5.1}% | Temp: {}°C                     ║", st.cpu_usage, st.temp_celsius);
                println!("║ RAM:      {:<5.1}% ({}/{} MB)               ║", st.ram_usage, st.ram_used_mb, st.ram_total_mb);
                println!("║ Disk:     {:<39} ║", st.disk_usage);
                println!("╠═══════════════════════════════════════════════════╣");
                println!("║ Power:    {:<39} ║", st.power_profile.to_uppercase());
                println!("║ Night:    {:<39} ║", st.night_mode.to_uppercase());
                println!("║ GPU:      {:<39} ║", st.gpu_profile.to_uppercase());
                println!("║ GameMode: {:<39} ║", if st.gamemode { "ACTIVE (PERFORMANCE)" } else { "OFF" });
                println!("║ VSync:    {:<39} ║", if st.vsync { "ON" } else { "OFF" });
                println!("║ Network:  {:<39} ║", format!("{} ({})", st.network_status, st.network_ssid));
                println!("║ Master:   {:<39} ║", format!("{}% ({})", st.master_volume, if st.master_muted { "MUTED" } else { "UNMUTED" }));
                println!("╚═══════════════════════════════════════════════════╝");
            } else {
                eprintln!("Daemon error: HTTP {}", res.status());
            }
        }
        Commands::Power { mode } => {
            if let Some(m) = mode {
                send_sys_cmd(&client, &cli.daemon_url, SystemCommand::SetPowerProfile(m)).await?;
            } else {
                send_sys_cmd(&client, &cli.daemon_url, SystemCommand::CyclePowerProfile).await?;
            }
        }
        Commands::Night { mode } => {
            if let Some(m) = mode {
                send_sys_cmd(&client, &cli.daemon_url, SystemCommand::SetNightMode(m)).await?;
            } else {
                send_sys_cmd(&client, &cli.daemon_url, SystemCommand::CycleNightMode).await?;
            }
        }
        Commands::Gpu { profile } => {
            if let Some(p) = profile {
                send_sys_cmd(&client, &cli.daemon_url, SystemCommand::SetGpuProfile(p)).await?;
            } else {
                send_sys_cmd(&client, &cli.daemon_url, SystemCommand::CycleGpuProfile).await?;
            }
        }
        Commands::Game => send_sys_cmd(&client, &cli.daemon_url, SystemCommand::ToggleGamemode).await?,
        Commands::Vsync => send_sys_cmd(&client, &cli.daemon_url, SystemCommand::ToggleVsync).await?,
        Commands::Warp => send_sys_cmd(&client, &cli.daemon_url, SystemCommand::ToggleWarp).await?,
        Commands::MasterVol { level } => send_sys_cmd(&client, &cli.daemon_url, SystemCommand::SetMasterVolume(level)).await?,
        Commands::MasterMute => send_sys_cmd(&client, &cli.daemon_url, SystemCommand::ToggleMasterMute).await?,
        Commands::Bright { level } => send_sys_cmd(&client, &cli.daemon_url, SystemCommand::SetBrightness(level)).await?,
        Commands::Theme { name } => {
            if let Some(t) = name {
                let res = client.post(format!("{}/api/theme", cli.daemon_url)).json(&serde_json::json!({ "theme": t })).send().await?;
                if res.status().is_success() {
                    println!("Theme switched to: {}", t);
                } else {
                    eprintln!("Failed to switch theme: HTTP {}", res.status());
                }
            } else {
                let res = client.get(format!("{}/api/theme", cli.daemon_url)).send().await?;
                if res.status().is_success() {
                    let val: serde_json::Value = res.json().await?;
                    println!("Current Theme: {}", val["current"].as_str().unwrap_or("unknown"));
                    if let Some(arr) = val["available"].as_array() {
                        println!("Available Themes:");
                        for item in arr {
                            println!("  • {:<12} - {}", item["name"].as_str().unwrap_or(""), item["display_name"].as_str().unwrap_or(""));
                        }
                    }
                }
            }
        }
        Commands::App { name } => send_sys_cmd(&client, &cli.daemon_url, SystemCommand::LaunchApp(name)).await?,
        Commands::Dashboard => {
            println!("Launching Arcy in Floating Dashboard Overlay mode...");
            let _ = std::process::Command::new("arcy-gui").arg("--mode").arg("dashboard").spawn()?;
        }
        Commands::Gui => {
            println!("Launching Arcy in Full Window mode...");
            let _ = std::process::Command::new("arcy-gui").arg("--mode").arg("window").spawn()?;
        }
        Commands::Lock => send_sys_cmd(&client, &cli.daemon_url, SystemCommand::SessionLock).await?,
        Commands::Sleep => send_sys_cmd(&client, &cli.daemon_url, SystemCommand::SessionSleep).await?,
        Commands::Reboot => send_sys_cmd(&client, &cli.daemon_url, SystemCommand::SessionReboot).await?,
        Commands::Poweroff => send_sys_cmd(&client, &cli.daemon_url, SystemCommand::SessionPoweroff).await?,
        Commands::Update => {
            println!("=== Checking for Arcy Music & Desktop updates ===");
            let mut script_opt = None;
            if let Some(data_dir) = dirs::data_dir() {
                let repo_file = data_dir.join("arcy-music/repo_path.txt");
                if let Ok(repo_dir) = std::fs::read_to_string(&repo_file) {
                    let script = std::path::PathBuf::from(repo_dir.trim()).join("update.sh");
                    if script.exists() {
                        script_opt = Some(script);
                    }
                }
                if script_opt.is_none() {
                    let script = data_dir.join("arcy-music/update.sh");
                    if script.exists() {
                        script_opt = Some(script);
                    }
                }
            }
            if script_opt.is_none() {
                let candidates = [
                    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("../../update.sh"))),
                    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("../share/arcy-music/update.sh"))),
                    dirs::home_dir().map(|h| h.join("arcy-music/update.sh")),
                    Some(std::path::PathBuf::from("update.sh")),
                ];
                script_opt = candidates.into_iter().flatten().find(|p| p.exists());
            }

            if let Some(script) = script_opt {
                println!("Executing updater: {:?}", script);
                let st = std::process::Command::new("bash")
                    .arg(&script)
                    .status();
                if let Ok(exit) = st {
                    if exit.success() {
                        println!("\nUpdate completed successfully! All playlists, Spotify tokens, and settings are preserved.");
                    } else {
                        eprintln!("\nUpdate process exited with status: {}", exit);
                    }
                } else {
                    eprintln!("Failed to execute update script.");
                }
            } else {
                println!("No local repository update script found. Please run './update.sh' inside your cloned arcy-music directory.");
            }
        }
    }

    Ok(())
}

async fn send_audio_cmd(client: &Client, daemon_url: &str, cmd: CoreCommand) -> Result<(), Box<dyn std::error::Error>> {
    let res = client
        .post(format!("{}/api/command", daemon_url))
        .json(&cmd)
        .send()
        .await?;

    if res.status().is_success() {
        println!("Command sent successfully.");
    } else {
        eprintln!("Command failed with status: {}", res.status());
    }
    Ok(())
}

async fn send_sys_cmd(client: &Client, daemon_url: &str, cmd: SystemCommand) -> Result<(), Box<dyn std::error::Error>> {
    let res = client
        .post(format!("{}/api/system/command", daemon_url))
        .json(&cmd)
        .send()
        .await?;

    if res.status().is_success() {
        println!("System command executed successfully.");
    } else {
        eprintln!("System command failed with status: {}", res.status());
    }
    Ok(())
}
