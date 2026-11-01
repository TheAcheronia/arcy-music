# Arcy Desktop & Music Package (Rust Core)

A blazing-fast, privacy-first native Desktop Control Center and Music Player written in Rust. Designed to replace resource-heavy webview and interpreted wrappers with pure compiled native binaries consuming near-zero CPU and memory in the background.

---

## Architecture Overview

Arcy is split into modular, focused components:

1. **`arcy-daemon` (`arcy-music-daemon`)**:
   - Headless background service consuming **< 30 MB RAM** and **~0.0% CPU** when idle.
   - Live hardware telemetry engine: reads `/proc/stat`, `/proc/meminfo`, thermals, and disk usage with zero shell overhead.
   - Safe system controller: manages Power Profiles (`powerprofilesctl`), Night Mode (`xrandr`), GPU overclocking (`nvidia-settings`), VSync (`picom`), GameMode, and Cloudflare WARP (`warp-cli`).
   - Audiophile audio playback engine with gapless playback, direct streaming cache, LRCLIB synchronized lyrics parser, and real-time 5-band biquad equalizer (RBJ cookbook).
   - Instant YouTube & Spotify resolution with automatic background prefetching for zero-latency playlist playback.
   - Standard MPRIS2 D-Bus service (`org.mpris.MediaPlayer2.ArcyMusic`) for full Linux media key and lockscreen integration.
   - Secure local REST API (`http://127.0.0.1:13338`).

2. **`arcy-gui` (`arcy-music-gui-native`)**:
   - Native desktop application rendered with Slint (hardware-accelerated OpenGL or software fallback).
   - Zero WebKit / Chromium overhead (< 45 MB RAM vs 600+ MB for Electron/Tauri).
   - Catppuccin Mocha aesthetic with smooth 60fps animations.
   - Comprehensive tabs:
     - **Dashboard**: Live telemetry gauges (CPU, RAM, Disk, Temp) & Quick Toggles (Power, Night, VSync, GameMode, GPU OC, Session).
     - **Diagnostics**: Detailed hardware specs (Kernel, CPU, GPU, WM, Host, Uptime).
     - **Network & VPN**: Active network details, Cloudflare WARP one-click toggle.
     - **Library**: Fast local music library browser (lossless tracks indexed in milliseconds).
     - **Search Online**: YouTube music search with instantaneous thumbnail rendering.
     - **Queue**: Real-time reorderable playlist queue with auto-advance.
     - **DSP Equalizer**: Interactive 5-band graphic sliders (60Hz, 250Hz, 1kHz, 4kHz, 12kHz) with instant presets.
     - **Lyrics**: Synced LRC karaoke display with smooth scrolling and manual sync correction.

3. **`arcy-web` (`arcy-music-gui-web`)**:
   - Modern, responsive web frontend running on top of the daemon.
   - Accessible via browser at `http://127.0.0.1:13338/web/index.html` or launched in an isolated lightweight app window via `arcy-web`.

4. **`arcy` (`arcy-music-cli`)**:
   - Ultra-fast command-line interface for terminal scripting, keybindings, automation, and one-command system updates.

---

## Installation

Clone the repository and run the installer:

```bash
git clone https://github.com/TheAcheronia/arcy-music.git
cd arcy-music
./install.sh
```

The installer builds optimized release binaries and installs:
- Executables: `~/.local/bin/` (`arcy`, `arcy-daemon`, `arcy-gui`, `arcy-web`)
- Assets & Web UI: `~/.local/share/arcy-music/`
- Desktop Launcher: `~/.local/share/applications/arcy.desktop`
- Icons: `~/.local/share/icons/`
- User Systemd Service: `~/.config/systemd/user/arcy-daemon.service`

### Running the Background Daemon on Startup

To enable the headless daemon to launch automatically on user login:

```bash
systemctl --user daemon-reload
systemctl --user enable --now arcy-daemon
```

---

## Updating

Updating Arcy is as simple as running a single command:

```bash
arcy update
```
or from the cloned directory:
```bash
./update.sh
```

### 🔒 User Data & Privacy Preservation Guarantee
All personal data—including:
- Spotify login session & `sp_dc` cookie
- Created playlists & saved tracks
- Local music folder paths & downloaded songs
- Equalizer profiles and custom volume settings

are stored strictly in your standard XDG user data directory (`~/.local/share/arcy-music/library.db`). 

**Updating the software never touches, overwrites, or deletes this database.** You will never lose your playlists or need to log into Spotify again after updating.

---

## Command Reference

### Hardware & System Management

```bash
# Print full system telemetry and quick toggles snapshot
arcy sys

# Cycle or set power profile (power-saver, balanced, performance)
arcy power
arcy power performance

# Cycle or set night mode brightness (off, evening, night)
arcy night
arcy night evening

# Cycle or set GPU profile (default, oc, turbo)
arcy gpu
arcy gpu turbo

# Toggle Game Mode (power profile boost + lightweight picom + feral gamemode)
arcy game

# Toggle compositor VSync
arcy vsync

# Toggle Cloudflare WARP VPN
arcy warp

# System Session Controls
arcy lock       # Lock desktop session
arcy sleep      # Suspend system
arcy reboot     # Reboot computer
arcy poweroff   # Power off computer
```

### Music & Audio Playback

```bash
# Playback controls
arcy toggle     # Play / Pause toggle
arcy play
arcy pause
arcy next       # Next track in queue
arcy prev       # Previous track in queue
arcy stop       # Stop playback

# Volume & Seeking
arcy volume 0.50      # Set music volume to 50%
arcy master-vol 80    # Set master system volume to 80%
arcy master-mute      # Toggle master mute
arcy seek 45          # Seek to 45 seconds

# Queue, Lyrics & Status
arcy shuffle    # Toggle shuffle mode
arcy repeat     # Toggle repeat mode (Off -> All -> One)
arcy lyrics     # View synced or plain lyrics
arcy status     # Print now-playing track metadata
arcy update     # Update Arcy to the latest version
```

### MPRIS2 Integration

Arcy exposes the standard `org.mpris.MediaPlayer2.ArcyMusic` interface:

```bash
playerctl -p ArcyMusic status
playerctl -p ArcyMusic play-pause
playerctl -p ArcyMusic next
playerctl -p ArcyMusic metadata
```

---

## Security & Privacy Directives

1. **Privacy First**: Zero tracking, zero analytics, zero external telemetry. All requests communicate solely between the local daemon and the user's selected music sources.
2. **Credential Isolation**: Spotify cookies and personal tokens are stored encrypted/safely in the local SQLite database and are masked by default in the UI.
3. **Memory Safety**: 100% safe Rust core with zero undefined behavior.
4. **Command Injection Immunization**: All external helper tools (`powerprofilesctl`, `xrandr`, `nvidia-settings`, `yt-dlp`) are executed via explicit argument slices with input whitelisting. Zero shell interpolation (`sh -c`).
