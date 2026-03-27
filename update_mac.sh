#!/bin/bash
# Tunante macOS Updater
# Downloads the latest version, replaces the current app, and clears quarantine.

set -euo pipefail

REPO="jjolmo/tunante"
APP_NAME="Tunante.app"
INSTALL_DIR="/Applications"

echo "=== Tunante Updater ==="
echo ""

# Get latest release info from GitHub
echo "Checking latest version..."
RELEASE_JSON=$(curl -sL "https://api.github.com/repos/$REPO/releases/latest")
TAG=$(echo "$RELEASE_JSON" | grep '"tag_name"' | head -1 | sed 's/.*: "\(.*\)".*/\1/')

if [ -z "$TAG" ]; then
    echo "Error: Could not fetch latest release info."
    exit 1
fi

echo "Latest version: $TAG"

# Find the DMG download URL
DMG_URL=$(echo "$RELEASE_JSON" | grep '"browser_download_url"' | grep '\.dmg"' | head -1 | sed 's/.*: "\(.*\)".*/\1/')

if [ -z "$DMG_URL" ]; then
    echo "Error: No DMG found in latest release."
    exit 1
fi

DMG_FILE=$(basename "$DMG_URL")
TMP_DIR=$(mktemp -d)
TMP_DMG="$TMP_DIR/$DMG_FILE"
MOUNT_POINT="$TMP_DIR/mount"

cleanup() {
    echo "Cleaning up..."
    hdiutil detach "$MOUNT_POINT" 2>/dev/null || true
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT

# Download
echo "Downloading $DMG_FILE..."
curl -L --progress-bar -o "$TMP_DMG" "$DMG_URL"

# Mount DMG
echo "Mounting disk image..."
mkdir -p "$MOUNT_POINT"
hdiutil attach "$TMP_DMG" -mountpoint "$MOUNT_POINT" -nobrowse -quiet

# Check the .app exists in the DMG
if [ ! -d "$MOUNT_POINT/$APP_NAME" ]; then
    echo "Error: $APP_NAME not found in disk image."
    exit 1
fi

# Close Tunante if running
if pgrep -x "Tunante" > /dev/null 2>&1; then
    echo "Closing Tunante..."
    osascript -e 'quit app "Tunante"' 2>/dev/null || true
    sleep 2
fi

# Replace the app
echo "Installing to $INSTALL_DIR/$APP_NAME..."
rm -rf "$INSTALL_DIR/$APP_NAME"
cp -R "$MOUNT_POINT/$APP_NAME" "$INSTALL_DIR/$APP_NAME"

# Clear quarantine
echo "Clearing quarantine attributes..."
xattr -cr "$INSTALL_DIR/$APP_NAME"

echo ""
echo "Done! Tunante $TAG installed."
echo "You can now open Tunante from Applications."

# Optionally relaunch
read -p "Launch Tunante now? [Y/n] " answer
case "${answer:-Y}" in
    [Yy]*|"") open "$INSTALL_DIR/$APP_NAME" ;;
esac
