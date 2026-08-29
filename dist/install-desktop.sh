#!/usr/bin/env bash
set -e

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
chmod +x "$DIR/dist/launch-app.sh"
chmod +x "$DIR/dist/start-wan-mesh.sh"

echo "Installing Community AI Desktop & App Menu launcher..."

# 1. Install high-res icon into standard FreeDesktop icon theme directories
ICON_DIR="$HOME/.local/share/icons/hicolor"
mkdir -p "$ICON_DIR/scalable/apps"
mkdir -p "$ICON_DIR/512x512/apps"
mkdir -p "$ICON_DIR/256x256/apps"
mkdir -p "$ICON_DIR/128x128/apps"
mkdir -p "$ICON_DIR/64x64/apps"
mkdir -p "$ICON_DIR/48x48/apps"
mkdir -p "$ICON_DIR/32x32/apps"
mkdir -p "$HOME/.local/share/pixmaps"

# Copy SVG icons
cp "$DIR/dist/icon.svg" "$ICON_DIR/scalable/apps/community-ai.svg"
cp "$DIR/dist/icon.svg" "$HOME/.local/share/pixmaps/community-ai.svg"
cp "$DIR/dist/icon.png" "$HOME/.local/share/pixmaps/community-ai.png"

# Generate and install sized PNG icons
python3 -c "
from PIL import Image
src_png = '$DIR/dist/icon.png'
img = Image.open(src_png)
for size in [512, 256, 128, 64, 48, 32]:
    resized = img.resize((size, size), Image.LANCZOS)
    resized.save(f'$ICON_DIR/{size}x{size}/apps/community-ai.png', 'PNG')
" 2>/dev/null || cp "$DIR/dist/icon.png" "$ICON_DIR/512x512/apps/community-ai.png"

# 2. Update Linux Icon theme caches
gtk-update-icon-cache -f -t "$ICON_DIR" 2>/dev/null || true

# 3. Create Desktop Entry in Applications Menu
mkdir -p "$HOME/.local/share/applications"

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

echo "$DESKTOP_ENTRY" > "$HOME/.local/share/applications/community-ai.desktop"
chmod +x "$HOME/.local/share/applications/community-ai.desktop"
update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true

# 4. If Desktop folder exists, install trusted desktop shortcut
if [ -d "$HOME/Desktop" ]; then
  rm -f "$HOME/Desktop/Community AI.desktop"
  echo "$DESKTOP_ENTRY" > "$HOME/Desktop/Community AI.desktop"
  chmod +x "$HOME/Desktop/Community AI.desktop"
  gio set "$HOME/Desktop/Community AI.desktop" metadata::trusted true 2>/dev/null || true
  gio set "$HOME/Desktop/Community AI.desktop" metadata::custom-icon "file://$DIR/dist/icon.png" 2>/dev/null || true
fi

# 5. Clear old thumbnail and icon caches so GNOME immediately loads the new icon
rm -rf "$HOME/.cache/thumbnails" 2>/dev/null || true
rm -rf "$HOME/.cache/gnome-shell/icon-theme" 2>/dev/null || true

echo "========================================================"
echo " Community AI Desktop App Successfully Installed!"
echo " You can now launch it directly from:"
echo " 1. Your Ubuntu Application Menu (search 'Community AI')"
echo " 2. Your Desktop shortcut: ~/Desktop/Community AI.desktop"
echo "========================================================"
