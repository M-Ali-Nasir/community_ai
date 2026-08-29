#!/usr/bin/env bash
set -e

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export PATH=$PATH:$HOME/.local/bin:$HOME/.nvm/versions/node/$(ls $HOME/.nvm/versions/node 2>/dev/null | tail -n 1)/bin:/usr/local/bin:/usr/bin

echo "=========================================================="
echo " 🌐 Starting Community AI Global WAN Decentralized Mesh"
echo "=========================================================="

# Clean up any lingering processes
pkill -f "node.*community-ai" 2>/dev/null || true
pkill -f "cloudflared" 2>/dev/null || true

# 1. Start local coordinator & web server
cd "$DIR/community-ai"
nohup npm run dev > /tmp/community-ai.log 2>&1 &
APP_PID=$!

echo "Waiting for coordinator to initialize on port 8787..."
for i in {1..30}; do
  if curl -s http://localhost:8787/api/state > /dev/null 2>&1; then
    break
  fi
  sleep 0.5
done

echo "✓ Local coordinator online at http://localhost:8787 and http://192.168.1.9:8787"

# 2. Start Cloudflare WAN Tunnel for trusted global Internet access
echo ""
echo "Creating zero-config Public WAN Mesh Gateway..."
nohup cloudflared tunnel --url http://localhost:8787 > /tmp/cloudflared.log 2>&1 &
TUNNEL_PID=$!

WAN_URL=""
for i in {1..40}; do
  WAN_URL=$(grep -o 'https://[-a-zA-Z0-9@:%._\+~#=]*\.trycloudflare\.com' /tmp/cloudflared.log | head -n 1 || true)
  if [ -n "$WAN_URL" ]; then
    break
  fi
  sleep 0.5
done

echo ""
echo "=========================================================="
if [ -n "$WAN_URL" ]; then
  echo " 🌍 GLOBAL WAN PUBLIC ENDPOINT (ANY PHONE / ANY DEVICE):"
  echo "    $WAN_URL"
  echo ""
  echo " Open the mobile app on your phone (or any browser worldwide),"
  echo " tap the top Mesh Status pill (⚙️), and set Coordinator URL to:"
  echo "    $WAN_URL"
else
  echo " LAN Endpoint (Same Wi-Fi):"
  echo "    http://192.168.1.9:8787"
fi
echo "=========================================================="
echo ""

# Launch desktop app interface
"$DIR/dist/launch-app.sh" &

wait $APP_PID
