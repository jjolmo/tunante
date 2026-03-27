#!/bin/bash
# Tunante macOS Updater
# Downloads the latest version, replaces the current app, and clears quarantine.

REPO="jjolmo/tunante"
APP_NAME="Tunante.app"
INSTALL_DIR="/Applications"

echo "=== Tunante Updater ==="
echo ""

# Get latest release info from GitHub
echo "Checking latest version..."
RELEASE_JSON=$(curl -sL "https://api.github.com/repos/$REPO/releases/latest")

if [ -z "$RELEASE_JSON" ]; then
    echo "Error: Could not reach GitHub API."
    echo "Press Enter to close."
    read
    exit 1
fi

TAG=$(echo "$RELEASE_JSON" | grep '"tag_name"' | head -1 | sed 's/.*: "\(.*\)".*/\1/')

if [ -z "$TAG" ]; then
    echo "Error: Could not parse release info."
    echo "Press Enter to close."
    read
    exit 1
fi

echo "Latest version: $TAG"

# Find the DMG download URL
DMG_URL=$(echo "$RELEASE_JSON" | grep '"browser_download_url"' | grep '\.dmg"' | head -1 | sed 's/.*: "\(.*\)".*/\1/')

if [ -z "$DMG_URL" ]; then
    echo "Error: No DMG found in latest release."
    echo "Press Enter to close."
    read
    exit 1
fi

DMG_FILE=$(basename "$DMG_URL")
TMP_DIR=$(mktemp -d)
TMP_DMG="$TMP_DIR/$DMG_FILE"
MOUNT_POINT="$TMP_DIR/mount"

# Download
echo "Downloading $DMG_FILE..."
curl -L --progress-bar -o "$TMP_DMG" "$DMG_URL"

if [ ! -f "$TMP_DMG" ]; then
    echo "Error: Download failed."
    echo "Press Enter to close."
    read
    exit 1
fi

# Mount DMG
echo "Mounting disk image..."
mkdir -p "$MOUNT_POINT"
if ! hdiutil attach "$TMP_DMG" -mountpoint "$MOUNT_POINT" -nobrowse -quiet; then
    echo "Error: Could not mount DMG."
    rm -rf "$TMP_DIR"
    echo "Press Enter to close."
    read
    exit 1
fi

# Check the .app exists in the DMG
if [ ! -d "$MOUNT_POINT/$APP_NAME" ]; then
    echo "Error: $APP_NAME not found in disk image."
    hdiutil detach "$MOUNT_POINT" 2>/dev/null || true
    rm -rf "$TMP_DIR"
    echo "Press Enter to close."
    read
    exit 1
fi

# Close Tunante if running
if pgrep -f "Tunante" > /dev/null 2>&1; then
    echo "Closing Tunante..."
    osascript -e 'quit app "Tunante"' 2>/dev/null || true
    # Wait up to 10 seconds for it to close
    for i in $(seq 1 10); do
        if ! pgrep -f "Tunante" > /dev/null 2>&1; then
            break
        fi
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
    echo "Press Enter to close."
    read
    exit 1
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
