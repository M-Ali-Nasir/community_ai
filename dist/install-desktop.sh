#!/usr/bin/env bash
set -e

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
chmod +x "$DIR/dist/launch-app.sh"

DESKTOP_ENTRY="[Desktop Entry]
Version=1.0
Type=Application
Name=Community AI
Comment=Decentralized Heterogeneous AI Cluster
Exec=\"$DIR/dist/launch-app.sh\"
Icon=$DIR/dist/icon.png
Terminal=false
Categories=Utility;Development;Network;
StartupWMClass=Community AI
"

mkdir -p "$HOME/.local/share/applications"
echo "$DESKTOP_ENTRY" > "$HOME/.local/share/applications/community-ai.desktop"
chmod +x "$HOME/.local/share/applications/community-ai.desktop"

# If Desktop folder exists, place a direct 1-click icon there
if [ -d "$HOME/Desktop" ]; then
  echo "$DESKTOP_ENTRY" > "$HOME/Desktop/Community AI.desktop"
  chmod +x "$HOME/Desktop/Community AI.desktop"
  gio set "$HOME/Desktop/Community AI.desktop" metadata::trusted true 2>/dev/null || true
fi

echo "========================================================"
echo " Community AI Desktop App Successfully Installed!"
echo " You can now launch it directly from:"
echo " 1. Your Ubuntu Application Menu (search 'Community AI')"
echo " 2. Your Desktop shortcut: ~/Desktop/Community AI.desktop"
echo "========================================================"
