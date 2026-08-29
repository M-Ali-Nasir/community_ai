#!/usr/bin/env bash
set -euo pipefail

echo "========================================================"
echo "    Community AI: Linux Worker Installation Script      "
echo "========================================================"

# Check root privileges for systemd setup
IS_ROOT=0
if [ "$EUID" -eq 0 ]; then
    IS_ROOT=1
fi

echo "1. Building release binary (community-daemon)..."
cargo build --release --bin community-daemon

BINARY_SRC="target/release/community-daemon"
BINARY_DEST="/usr/local/bin/community-daemon"
SERVICE_SRC="platform/linux/community-ai.service"
SERVICE_DEST="/etc/systemd/system/community-ai.service"

if [ "$IS_ROOT" -eq 1 ]; then
    echo "2. Installing binary to $BINARY_DEST..."
    install -m 755 "$BINARY_SRC" "$BINARY_DEST"

    echo "3. Creating system user and cache directory..."
    id -u community-ai &>/dev/null || useradd -r -s /bin/false community-ai
    mkdir -p /var/lib/community-ai/cache
    chown -R community-ai:community-ai /var/lib/community-ai

    echo "4. Installing systemd background service..."
    install -m 644 "$SERVICE_SRC" "$SERVICE_DEST"
    systemctl daemon-reload
    systemctl enable community-ai
    systemctl restart community-ai

    echo "========================================================"
    echo "✅ Installation Complete!"
    echo "Service Status: sudo systemctl status community-ai"
    echo "View Logs:      sudo journalctl -u community-ai -f"
    echo "========================================================"
else
    echo "2. Non-root user detected. You can run the binary directly:"
    echo "   $BINARY_SRC --name my-worker-$(hostname) --coordinator 127.0.0.1:8080"
    echo ""
    echo "To install as a 24/7 background system service, re-run with sudo:"
    echo "   sudo ./platform/linux/install.sh"
fi
