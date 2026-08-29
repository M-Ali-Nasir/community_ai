#!/usr/bin/env bash
set -e

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export PATH=$PATH:$HOME/.local/bin:$HOME/.nvm/versions/node/$(ls $HOME/.nvm/versions/node 2>/dev/null | tail -n 1)/bin:/usr/local/bin:/usr/bin

echo "Starting Community AI Desktop App..."

# Kill any previous instance cleanly
pkill -f "node.*community-ai" 2>/dev/null || true

# Start background services
cd "$DIR/community-ai"
nohup npm run dev > /tmp/community-ai.log 2>&1 &
SERVER_PID=$!

# Wait for server to become ready
echo "Waiting for app to initialize..."
for i in {1..30}; do
  if curl -s http://localhost:5173 > /dev/null 2>&1; then
    break
  fi
  sleep 0.5
done

# Profile directory to keep sessions without triggering Chrome setup wizards
PROFILE_DIR="$HOME/.config/community-ai/app-profile"
mkdir -p "$PROFILE_DIR"
# Touch 'First Run' file to explicitly tell Chromium/Chrome not to show welcome or default browser checks
touch "$PROFILE_DIR/First Run"

URL="http://localhost:5173"

# Flags to suppress all browser chrome, default browser prompts, sync prompts, and toolbars
APP_FLAGS=(
  --app="$URL"
  --user-data-dir="$PROFILE_DIR"
  --no-first-run
  --no-default-browser-check
  --disable-default-apps
  --disable-sync
  --disable-infobars
  --disable-extensions
  --disable-component-update
  --disable-features=Translate,OptimizationHints,MediaRouter
  --window-size=1200,820
)

if command -v google-chrome >/dev/null 2>&1; then
  exec google-chrome "${APP_FLAGS[@]}"
elif command -v chromium-browser >/dev/null 2>&1; then
  exec chromium-browser "${APP_FLAGS[@]}"
elif command -v chromium >/dev/null 2>&1; then
  exec chromium "${APP_FLAGS[@]}"
elif command -v brave-browser >/dev/null 2>&1; then
  exec brave-browser "${APP_FLAGS[@]}"
elif command -v microsoft-edge >/dev/null 2>&1; then
  exec microsoft-edge "${APP_FLAGS[@]}"
elif command -v xdg-open >/dev/null 2>&1; then
  xdg-open "$URL"
else
  firefox --new-window "$URL"
fi
