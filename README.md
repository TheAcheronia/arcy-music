# Arcy Music Player (Rust Core)

A blazing-fast, privacy-first native music player and audiophile streaming client for Linux, written in 100% Rust. Designed to replace resource-heavy Electron/Tauri players with pure compiled native binaries consuming near-zero CPU and memory in the background.

---

## Architecture Overview

Arcy Music is split into modular, focused components:

1. **`arcy-daemon` (`arcy-music-daemon`)**:
   - Headless background audio service consuming **< 25 MB RAM** and **~0.0% CPU** when idle.
   - Audiophile playback engine with gapless playback, direct streaming cache, and background prefetching for zero-latency playlist playback.
   - Real-time 5-band biquad equalizer (RBJ cookbook) with instantaneous filter response.
   - LRCLIB synchronized lyrics parser with timecode interpolation.
   - Standard MPRIS2 D-Bus service (`org.mpris.MediaPlayer2.ArcyMusic`) for full Linux media key and lockscreen integration.
   - Secure local REST API (`http://127.0.0.1:13338`).

2. **`arcy-gui` (`arcy-music-gui-native`)**:
   - Native desktop application rendered with Slint (hardware-accelerated OpenGL or software fallback).
   - Zero WebKit / Chromium overhead (< 45 MB RAM vs 600+ MB for Electron/Spotube).
   - Catppuccin Mocha aesthetic with smooth 60fps animations.
   - Comprehensive tabs:
     - **Library**: Fast local music browser (thousands of lossless tracks indexed in milliseconds).
     - **Search Online**: YouTube music search with instantaneous thumbnail rendering and playback.
     - **Queue**: Real-time reorderable playlist queue with auto-advance and shuffle.
     - **DSP Equalizer**: Interactive 5-band graphic sliders (60Hz, 250Hz, 1kHz, 4kHz, 12kHz) with instant presets.
     - **Lyrics**: Synced LRC karaoke display with smooth scrolling and manual sync correction.
     - **Settings**: Local music directories, Spotify sync with masked cookie security, audio sink selection.

3. **`arcy-web` (`arcy-music-gui-web`)**:
   - Modern, responsive web frontend running on top of the daemon.
   - Accessible via browser at `http://127.0.0.1:13338/web/index.html` or launched as an isolated lightweight window via `arcy-web`.

4. **`arcy` (`arcy-music-cli`)**:
   - Ultra-fast command-line interface for terminal scripting, keybindings, and single-command updates.

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

Updating Arcy Music is as simple as running a single command:

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

## Command Reference (CLI)

```bash
# Playback Controls
arcy toggle              # Play / Pause toggle
arcy play                # Resume playback
arcy pause               # Pause playback
arcy stop                # Stop playback entirely
arcy next                # Next track in queue
arcy prev                # Previous track in queue

# Volume & Seeking
arcy volume 0.70         # Set music volume to 70% (0.0 - 1.0)
arcy seek 45             # Seek to 45 seconds

# Queue, Mode & Lyrics
arcy shuffle             # Toggle shuffle mode
arcy repeat              # Toggle repeat mode (Off -> All -> One)
arcy lyrics              # View synced or plain lyrics in terminal
arcy status              # Print now-playing track metadata & progress

# Themes & Interface
arcy theme               # Print active theme & list available themes
arcy theme nord          # Switch theme (mocha, nord, tokyo, cyberpunk, dracula, gruvbox)
arcy gui                 # Launch Native GUI window

# System Maintenance
arcy update              # Update Arcy Music to the latest release
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
4. **Command Injection Immunization**: All external helper tools (`yt-dlp`) are executed via explicit argument slices with input whitelisting. Zero shell interpolation (`sh -c`).
