#!/usr/bin/env bash
# ==============================================================================
# Arcy Music Launcher Script
# Automatically manages the daemon and native GUI.
# ==============================================================================

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/arcy-music"
mkdir -p "$DATA_DIR"
DAEMON_LOG="$DATA_DIR/daemon.log"

export ARCY_PORT="${ARCY_PORT:-13338}"
DAEMON_URL="http://127.0.0.1:${ARCY_PORT}/api/state"

find_binary() {
    local bin_name="$1"
    if [ -x "$SCRIPT_DIR/target/release/$bin_name" ]; then
        echo "$SCRIPT_DIR/target/release/$bin_name"
    elif [ -x "$SCRIPT_DIR/target/debug/$bin_name" ]; then
        echo "$SCRIPT_DIR/target/debug/$bin_name"
    else
        echo ""
    fi
}

is_daemon_running() {
    curl -s --fail --connect-timeout 1 "$DAEMON_URL" > /dev/null 2>&1
}

stop_daemon() {
    echo "Stopping Arcy Music Daemon..."
    pkill -f "arcy-music-daemon" 2>/dev/null || true
    echo "Daemon stopped."
}

status_daemon() {
    if is_daemon_running; then
        echo "Arcy Music Daemon is running (API accessible at 127.0.0.1:${ARCY_PORT})."
    else
        echo "Arcy Music Daemon is not running."
    fi
}

# Parse CLI flags (default KEEP_DAEMON=1 so closing GUI does NOT kill music playback)
KEEP_DAEMON=1
DAEMON_ONLY=0
USE_WEB=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --stop)
            stop_daemon
            exit 0
            ;;
        --status)
            status_daemon
            exit 0
            ;;
        --logs)
            tail -f "$DAEMON_LOG"
            exit 0
            ;;
        --kill-on-exit)
            KEEP_DAEMON=0
            shift
            ;;
        --keep-daemon)
            KEEP_DAEMON=1
            shift
            ;;
        --daemon-only)
            DAEMON_ONLY=1
            shift
            ;;
        --web)
            USE_WEB=1
            shift
            ;;
        *)
            break
            ;;
    esac
done

DAEMON_BIN="$(find_binary arcy-music-daemon)"
GUI_BIN="$(find_binary arcy-music-gui-native)"
WEB_BIN="$(find_binary arcy-music-gui-web)"

# If either binary is missing, build them
if [ -z "$DAEMON_BIN" ] || [ -z "$GUI_BIN" ] || [ -z "$WEB_BIN" ]; then
    echo "Building Arcy Music binaries..."
    cargo build --bin arcy-music-daemon --bin arcy-music-gui-native --bin arcy-music-gui-web
    DAEMON_BIN="$(find_binary arcy-music-daemon)"
    GUI_BIN="$(find_binary arcy-music-gui-native)"
    WEB_BIN="$(find_binary arcy-music-gui-web)"
fi

STARTED_DAEMON=0
DAEMON_PID=""

if is_daemon_running; then
    echo "Arcy Music Daemon is already running."
else
    echo "Starting Arcy Music Daemon in background..."
    nohup "$DAEMON_BIN" > "$DAEMON_LOG" 2>&1 &
    DAEMON_PID=$!
    disown "$DAEMON_PID" 2>/dev/null || true
    STARTED_DAEMON=1

    # Wait for daemon to become ready (up to 5 seconds)
    READY=0
    for _ in {1..50}; do
        if is_daemon_running; then
            READY=1
            break
        fi
        sleep 0.1
    done

    if [ "$READY" -eq 1 ]; then
        echo "Daemon is ready."
    else
        echo "Warning: Daemon did not report ready within 5 seconds. Check $DAEMON_LOG"
    fi
fi

if [ "$DAEMON_ONLY" -eq 1 ]; then
    echo "Daemon running in background. Logs: $DAEMON_LOG"
    exit 0
fi

# Cleanup daemon if we started it and KEEP_DAEMON is not set
cleanup() {
    if [ "$STARTED_DAEMON" -eq 1 ] && [ "$KEEP_DAEMON" -eq 0 ]; then
        echo "Closing Arcy Music Daemon..."
        if [ -n "$DAEMON_PID" ] && kill -0 "$DAEMON_PID" 2>/dev/null; then
            kill "$DAEMON_PID" 2>/dev/null || true
        else
            pkill -f "arcy-music-daemon" 2>/dev/null || true
        fi
    fi
}
trap cleanup EXIT INT TERM

if [ "$USE_WEB" -eq 1 ]; then
    echo "Launching Arcy Music Web GUI..."
    "$WEB_BIN" "$@"
else
    echo "Launching Arcy Music GUI..."
    "$GUI_BIN" "$@"
fi
