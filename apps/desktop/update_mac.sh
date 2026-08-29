#!/bin/bash
# Tunante macOS Updater
# Downloads the latest version, replaces the current app, and clears quarantine.

REPO="jjolmo/tunante"
APP_NAME="Tunante.app"
INSTALL_DIR="/Applications"

echo "=== Tunante Updater ==="
echo ""

# Get latest release info from GitHub (use python3 for reliable JSON parsing)
echo "Checking latest version..."
RELEASE_JSON=$(curl -sL "https://api.github.com/repos/$REPO/releases/latest")

if [ -z "$RELEASE_JSON" ]; then
    echo "Error: Could not reach GitHub API."
    echo "Press Enter to close." ; read ; exit 1
fi

# Parse JSON with python3 (always available on macOS)
PARSED=$(echo "$RELEASE_JSON" | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    tag = data.get('tag_name', '')
    dmg_url = ''
    for asset in data.get('assets', []):
        if asset['name'].endswith('.dmg'):
            dmg_url = asset['browser_download_url']
            break
    print(f'{tag}|{dmg_url}')
except Exception as e:
    print(f'|', file=sys.stderr)
    sys.exit(1)
" 2>&1)

TAG=$(echo "$PARSED" | cut -d'|' -f1)
DMG_URL=$(echo "$PARSED" | cut -d'|' -f2)

if [ -z "$TAG" ]; then
    echo "Error: Could not parse release info."
    echo "Press Enter to close." ; read ; exit 1
fi

echo "Latest version: $TAG"

if [ -z "$DMG_URL" ]; then
    echo "Error: No DMG found in release $TAG."
    echo "Press Enter to close." ; read ; exit 1
fi

DMG_FILE=$(basename "$DMG_URL")
TMP_DIR=$(mktemp -d)
TMP_DMG="$TMP_DIR/$DMG_FILE"
MOUNT_POINT="$TMP_DIR/mount"

# Download
echo "Downloading $DMG_FILE..."
echo "URL: $DMG_URL"
echo ""
if ! curl -L --progress-bar --fail -o "$TMP_DMG" "$DMG_URL"; then
    echo ""
    echo "Error: Download failed (curl returned error)."
    rm -rf "$TMP_DIR"
    echo "Press Enter to close." ; read ; exit 1
fi

# Verify the downloaded file is not empty
FILE_SIZE=$(stat -f%z "$TMP_DMG" 2>/dev/null || echo "0")
if [ "$FILE_SIZE" -lt 1000 ]; then
    echo "Error: Downloaded file is too small (${FILE_SIZE} bytes). Likely not a valid DMG."
    rm -rf "$TMP_DIR"
    echo "Press Enter to close." ; read ; exit 1
fi

echo ""
echo "Downloaded $(( FILE_SIZE / 1024 / 1024 )) MB"

# Mount DMG
echo "Mounting disk image..."
mkdir -p "$MOUNT_POINT"
if ! hdiutil attach "$TMP_DMG" -mountpoint "$MOUNT_POINT" -nobrowse -quiet; then
    echo "Error: Could not mount DMG."
    rm -rf "$TMP_DIR"
    echo "Press Enter to close." ; read ; exit 1
fi

# Check the .app exists in the DMG
if [ ! -d "$MOUNT_POINT/$APP_NAME" ]; then
    echo "Error: $APP_NAME not found in disk image."
    echo "Contents: $(ls "$MOUNT_POINT")"
    hdiutil detach "$MOUNT_POINT" 2>/dev/null || true
    rm -rf "$TMP_DIR"
    echo "Press Enter to close." ; read ; exit 1
fi

# Close Tunante if running
if pgrep -f "Tunante" > /dev/null 2>&1; then
    echo "Closing Tunante..."
    osascript -e 'quit app "Tunante"' 2>/dev/null || true
    for i in $(seq 1 10); do
        pgrep -f "Tunante" > /dev/null 2>&1 || break
        sleep 1
    done
    # Force kill if still running
    if pgrep -f "Tunante" > /dev/null 2>&1; then
        echo "Force closing..."
        pkill -f "Tunante" 2>/dev/null || true
        sleep 1
    fi
fi

# Replace the app
echo "Installing to $INSTALL_DIR/$APP_NAME..."
rm -rf "$INSTALL_DIR/$APP_NAME"
cp -R "$MOUNT_POINT/$APP_NAME" "$INSTALL_DIR/$APP_NAME"

if [ ! -d "$INSTALL_DIR/$APP_NAME" ]; then
    echo "Error: Failed to copy app to $INSTALL_DIR."
    echo "You may need to run this script with sudo."
    hdiutil detach "$MOUNT_POINT" 2>/dev/null || true
    rm -rf "$TMP_DIR"
    echo "Press Enter to close." ; read ; exit 1
fi

# Clear quarantine
echo "Clearing quarantine attributes..."
xattr -cr "$INSTALL_DIR/$APP_NAME"

# Cleanup
echo "Cleaning up..."
hdiutil detach "$MOUNT_POINT" 2>/dev/null || true
rm -rf "$TMP_DIR"

echo ""
echo "Done! Tunante $TAG installed."
echo "Launching Tunante..."
open "$INSTALL_DIR/$APP_NAME"
