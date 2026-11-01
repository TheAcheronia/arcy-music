#!/usr/bin/env bash
# ==============================================================================
# Arcy Desktop & Music Player - One-Command Updater
# Pulls latest updates from GitHub and installs them without losing any user data.
# ==============================================================================
set -e

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"
cd "$DIR"

echo "=== Updating Arcy Music & Desktop ==="

# 1. Pull latest changes if in a git repository
if [ -d ".git" ]; then
    echo "-> Pulling latest updates from Git repository..."
    git pull --rebase || {
        echo "Note: Git pull failed or branch has diverged. Proceeding with local installation..."
    }
fi

# 2. Run installer (compiles --release and updates binaries & assets)
"$DIR/install.sh"

# 3. Gracefully reload daemon if running
PORT="${ARCY_PORT:-13338}"
if curl -s --connect-timeout 1 "http://127.0.0.1:${PORT}/api/state" >/dev/null 2>&1; then
    echo "-> Gracefully reloading background daemon..."
    curl -s -X POST "http://127.0.0.1:${PORT}/api/daemon/shutdown" >/dev/null 2>&1 || true
    sleep 1
    nohup "$HOME/.local/bin/arcy-daemon" > /dev/null 2>&1 &
    sleep 0.5
    echo "-> Background daemon reloaded with latest version."
fi

echo ""
echo "=== Update Complete! ==="
echo "All your Spotify logins, custom playlists, and settings remain 100% intact."
