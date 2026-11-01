use clap::{Parser, Subcommand};
use arcy_music_core::{Command as CoreCommand, PlaybackState};
use reqwest::Client;

#[derive(Parser)]
#[command(author, version, about = "Fast CLI controller for Arcy Music Player", long_about = None)]
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
    /// Get or switch Arcy theme (mocha, nord, tokyo, cyberpunk, dracula, gruvbox)
    Theme { name: Option<String> },
    /// Launch Arcy Music in Native GUI mode
    Gui,
    /// Update Arcy Music to the latest version while preserving all logins and playlists
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
        Commands::Gui => {
            println!("Launching Arcy Music GUI...");
            let _ = std::process::Command::new("arcy-gui").spawn()?;
        }
        Commands::Update => {
            println!("=== Checking for Arcy Music updates ===");
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
