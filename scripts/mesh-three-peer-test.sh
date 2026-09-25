#!/usr/bin/env bash
# Three native daemons. No coordinator. No browser.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "==> building community-daemon"
cargo build -p community-daemon --quiet

BIN="$ROOT/target/debug/community-daemon"
TMP="$(mktemp -d)"
A_PID=""
B_PID=""
C_PID=""

cleanup() {
  kill "$A_PID" "$B_PID" "$C_PID" 2>/dev/null || true
  wait "$A_PID" "$B_PID" "$C_PID" 2>/dev/null || true
  rm -rf "$TMP"
}
trap cleanup EXIT

A_PORT=50151
B_PORT=50152
C_PORT=50153

start_peer() {
  local name="$1" port="$2"
  shift 2
  RUST_LOG=info "$BIN" \
    --name "$name" \
    --bind 127.0.0.1 \
    --port "$port" \
    --no-mdns \
    --identity "$TMP/${name}.key" \
    --cache-dir "$TMP/${name}-cache" \
    "$@" \
    >"$TMP/${name}.log" 2>&1 &
  echo $!
}

A_PID="$(start_peer peer-a "$A_PORT")"
sleep 0.4
B_PID="$(start_peer peer-b "$B_PORT" --peer "127.0.0.1:${A_PORT}")"
C_PID="$(start_peer peer-c "$C_PORT" --peer "127.0.0.1:${A_PORT}")"

wait_log() {
  local file="$1" pattern="$2" seconds="$3"
  local i=0
  local n=$((seconds * 2))
  while [[ "$i" -lt "$n" ]]; do
    if grep -q "$pattern" "$file" 2>/dev/null; then
      return 0
    fi
    sleep 0.5
    i=$((i + 1))
  done
  return 1
}

echo "==> waiting for A to see 2 ready peers (B and C)"
if ! wait_log "$TMP/peer-a.log" "ready_peers=2" 25; then
  echo "FAIL: A never reached 2 ready peers"
  tail -n 50 "$TMP/peer-a.log" "$TMP/peer-b.log" "$TMP/peer-c.log" || true
  exit 1
fi

echo "==> waiting for gossip so B and C each see 2 peers"
if ! wait_log "$TMP/peer-b.log" "ready_peers=2" 25; then
  echo "FAIL: B did not reach 2 peers via gossip"
  tail -n 50 "$TMP/peer-b.log" || true
  exit 1
fi
if ! wait_log "$TMP/peer-c.log" "ready_peers=2" 25; then
  echo "FAIL: C did not reach 2 peers via gossip"
  tail -n 50 "$TMP/peer-c.log" || true
  exit 1
fi

echo "==> killing B; A must detect failure; A and C must keep running"
kill "$B_PID"
wait "$B_PID" 2>/dev/null || true
B_PID=""

if ! wait_log "$TMP/peer-a.log" "disconnected" 20; then
  echo "FAIL: A did not detect B's failure"
  tail -n 50 "$TMP/peer-a.log" || true
  exit 1
fi

if ! kill -0 "$A_PID" 2>/dev/null; then
  echo "FAIL: A died after B was killed"
  exit 1
fi
if ! kill -0 "$C_PID" 2>/dev/null; then
  echo "FAIL: C died after B was killed"
  exit 1
fi

echo "PASS: three native daemons, no coordinator, no browser"
trap 'kill "$A_PID" "$C_PID" 2>/dev/null || true' EXIT
