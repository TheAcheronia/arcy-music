use std::path::PathBuf;
use std::process::Command;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting Arcy Music Web GUI launcher...");

    // Record our PID
    let pid = std::process::id();
    let pid_file = PathBuf::from("/tmp/arcy-music-gui-web.pid");
    let _ = std::fs::write(&pid_file, pid.to_string());

    let port: u16 = std::env::var("ARCY_MUSIC_PORT")
        .or_else(|_| std::env::var("ARCY_PORT"))
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(13338);

    let daemon_url = format!("http://127.0.0.1:{}", port);
    let web_url = format!("{}/web/index.html", daemon_url);

    // Check if daemon is responding
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(800))
        .build()?;

    let daemon_up = client.get(format!("{}/api/state", daemon_url)).send().await.is_ok();
    if !daemon_up {
        println!("Arcy daemon not responding. Attempting to start daemon...");
        let candidates = [
            std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("arcy-music-daemon"))),
            std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("arcy-daemon"))),
            dirs::home_dir().map(|h| h.join(".local/bin/arcy-music-daemon")),
            dirs::home_dir().map(|h| h.join(".local/bin/arcy-daemon")),
            Some(PathBuf::from("/usr/bin/arcy-music-daemon")),
            Some(PathBuf::from("/usr/local/bin/arcy-music-daemon")),
            Some(PathBuf::from("target/release/arcy-music-daemon")),
            Some(PathBuf::from("target/debug/arcy-music-daemon")),
        ];
        for c in candidates.into_iter().flatten() {
            if c.exists() {
                let _ = Command::new(&c).spawn();
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                break;
            }
        }
    }

    let profile_dir = dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap().join(".config"))
        .join("arcy-music/web-gui-profile");
    let _ = std::fs::create_dir_all(&profile_dir);

    println!("Launching Web GUI window for URL: {}", web_url);

    // Browser candidates in order of preference
    let mut child = None;

    if std::path::Path::new("/opt/brave-bin/brave").exists() {
        if let Ok(c) = Command::new("/opt/brave-bin/brave")
            .arg(format!("--app={}", web_url))
            .arg(format!("--user-data-dir={}", profile_dir.display()))
            .arg("--class=arcy-music-gui-web")
            .arg("--window-size=1120,740")
            .spawn()
        {
            child = Some(c);
        }
    }

    if child.is_none() {
        for bin in &["google-chrome", "chromium", "brave"] {
            if let Ok(c) = Command::new(bin)
                .arg(format!("--app={}", web_url))
                .arg(format!("--user-data-dir={}", profile_dir.display()))
                .arg("--class=arcy-music-gui-web")
                .arg("--window-size=1120,740")
                .spawn()
            {
                child = Some(c);
                break;
            }
        }
    }

    if child.is_none() {
        if let Ok(c) = Command::new("firefox")
            .args(["--new-window", &web_url])
            .spawn()
        {
            child = Some(c);
        }
    }

    if let Some(mut c) = child {
        // Record child browser PID as well
        let browser_pid = c.id();
        let _ = std::fs::write("/tmp/arcy-music-gui-browser.pid", browser_pid.to_string());

        // Background task to ensure window is not sticky across workspaces in Awesome WM
        tokio::spawn(async {
            use std::io::Write;
            for _ in 0..10 {
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                if let Ok(mut child) = std::process::Command::new("awesome-client")
                    .stdin(std::process::Stdio::piped())
                    .spawn()
                {
                    if let Some(mut stdin) = child.stdin.take() {
                        let _ = stdin.write_all(b"for _, c in ipairs(client.get()) do if c.class == 'arcy-music-gui-web' or c.instance == 'arcy-music-gui-web' or (c.name and c.name:match('Arcy Music %(Web GUI%)')) then c.sticky = false c.ontop = false end end\n");
                    }
                    let _ = child.wait();
                }
            }
        });

        // Wait for browser process
        let _ = c.wait();
    } else {
        eprintln!("Could not find a suitable browser to launch Web GUI");
    }

    let _ = std::fs::remove_file(&pid_file);
    let _ = std::fs::remove_file("/tmp/arcy-music-gui-browser.pid");

    Ok(())
}
