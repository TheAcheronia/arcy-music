#!/usr/bin/env bash
# ==============================================================================
# Arcy Music Player - Installer
# Builds and installs binaries, web assets, desktop entries, and systemd units.
# User playlists, Spotify login, and personal settings are ALWAYS preserved.
# ==============================================================================
set -e

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"
BIN_DIR="${HOME}/.local/bin"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/arcy-music"
APP_DIR="${HOME}/.local/share/applications"
ICON_DIR_SVG="${HOME}/.local/share/icons/hicolor/scalable/apps"
ICON_DIR_PNG="${HOME}/.local/share/icons/hicolor/256x256/apps"
SYSTEMD_USER_DIR="${HOME}/.config/systemd/user"

echo "=== Installing / Updating Arcy Music ==="

# 1. Compile stable release binaries
echo "-> Building optimized release binaries..."
cargo build --release --manifest-path "$DIR/Cargo.toml"

# 2. Ensure directories exist
mkdir -p "$BIN_DIR" "$DATA_DIR/web" "$DATA_DIR/scripts" "$APP_DIR" "$ICON_DIR_SVG" "$ICON_DIR_PNG" "$SYSTEMD_USER_DIR"

# 3. Install release binaries
echo "-> Installing executable binaries to $BIN_DIR..."
install -m 755 "$DIR/target/release/arcy-music-cli" "$BIN_DIR/arcy"
install -m 755 "$DIR/target/release/arcy-music-daemon" "$BIN_DIR/arcy-daemon"
install -m 755 "$DIR/target/release/arcy-music-gui-native" "$BIN_DIR/arcy-gui"
install -m 755 "$DIR/target/release/arcy-music-gui-web" "$BIN_DIR/arcy-web"

# Also symlink canonical names for compatibility
ln -sf "$BIN_DIR/arcy-daemon" "$BIN_DIR/arcy-music-daemon"
ln -sf "$BIN_DIR/arcy-gui" "$BIN_DIR/arcy-music-gui-native"
ln -sf "$BIN_DIR/arcy-web" "$BIN_DIR/arcy-music-gui-web"

# 4. Install assets (Web interface and helper scripts)
echo "-> Synchronizing web client and scripts to $DATA_DIR..."
if [ -d "$DIR/arcy-music-gui-web/web" ]; then
    cp -r "$DIR/arcy-music-gui-web/web/"* "$DATA_DIR/web/"
fi
if [ -d "$DIR/arcy-music-daemon/scripts" ]; then
    cp -r "$DIR/arcy-music-daemon/scripts/"* "$DATA_DIR/scripts/"
fi

# Save updater and repo location for 'arcy update'
echo "$DIR" > "$DATA_DIR/repo_path.txt"
cp -f "$DIR/update.sh" "$DATA_DIR/update.sh"
chmod +x "$DATA_DIR/update.sh"

# 5. Install Desktop Launcher
echo "-> Installing desktop launcher to $APP_DIR..."
sed "s|Icon=.*|Icon=arcy-music|g" "$DIR/assets/arcy.desktop" > "$APP_DIR/arcy.desktop"
chmod +x "$APP_DIR/arcy.desktop"

# 6. Install Icons
echo "-> Installing application icons..."
cp -f "$DIR/assets/arcy-music.svg" "$ICON_DIR_SVG/arcy-music.svg" 2>/dev/null || true
cp -f "$DIR/assets/arcy-music.png" "$ICON_DIR_PNG/arcy-music.png" 2>/dev/null || true

# 7. Install systemd user service
echo "-> Installing systemd user unit to $SYSTEMD_USER_DIR..."
cp -f "$DIR/assets/arcy-daemon.service" "$SYSTEMD_USER_DIR/arcy-daemon.service"

echo "-> Updating desktop database..."
update-desktop-database "$APP_DIR" 2>/dev/null || true

echo ""
echo "=== Installation & Update Successful! ==="
echo "Note: Your user database, playlists, Spotify credentials, and configuration"
echo "at $DATA_DIR/library.db have been safely preserved."
echo ""
echo "Commands available:"
echo "  arcy               # CLI music playback controls"
echo "  arcy-gui           # Native Slint Music Player"
echo "  arcy-web           # Modern Web GUI Window"
echo "  arcy update        # Easily update to future releases"
echo ""
echo "To update anytime in the future, just run:"
echo "  ./update.sh   (or 'arcy update')"
