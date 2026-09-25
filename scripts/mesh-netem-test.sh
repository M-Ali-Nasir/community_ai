#!/usr/bin/env bash
# NETWORK EMULATED helper — does not claim PHYSICAL WAN.
# Requires root + `tc`. If unavailable, exit 0 with an honest skip.
set -euo pipefail
if [[ "${EUID}" -ne 0 ]] || ! command -v tc >/dev/null 2>&1; then
  echo "NETWORK EMULATED — NOT RUN (need root and tc). This is not a WAN physical test."
  exit 0
fi
DEV="${1:-lo}"
echo "Applying netem delay 80ms loss 2% on ${DEV} (loopback emulation only)"
tc qdisc replace dev "${DEV}" root netem delay 80ms loss 2%
echo "Run: cargo test -p community-network --lib two_peers_quic_handshake_ready -- --nocapture"
echo "Then: tc qdisc del dev ${DEV} root"
echo "Label any pass as NETWORK EMULATED, not PHYSICAL WAN VERIFIED."
