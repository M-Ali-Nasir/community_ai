#!/usr/bin/env bash
# WAN inference harness.
#
# Default: two native processes on THIS host → PROCESS VERIFIED only.
# That is NEVER PHYSICAL WAN VERIFIED.
#
# Physical WAN (two ISPs) is documented in docs/testing/WAN_VALIDATION.md
# and must be run by an operator with two Internet connections.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

CLASSIFICATION_OVERRIDE="${WAN_EVIDENCE_CLASS:-}"
USE_RELAY="${WAN_USE_RELAY:-0}"
GGUF="${COMMUNITY_AI_TEST_GGUF:-}"
if [[ -z "$GGUF" ]]; then
  for c in \
    "$ROOT/community-ai/models/hf_Qwen_qwen2.5-0.5b-instruct-q4_k_m.gguf" \
    "$ROOT/community-ai/models/hf_HuggingFaceTB_smollm2-360m-instruct-q8_0.gguf"
  do
    if [[ -f "$c" ]]; then GGUF="$c"; break; fi
  done
fi

echo "==> building community-daemon (+ relay)"
cargo build -p community-daemon -p community-network --bin community-relay --quiet
BIN="$ROOT/target/debug/community-daemon"
RELAY_BIN="$ROOT/target/debug/community-relay"
TMP="$(mktemp -d)"
W_PID=""
O_PID=""
R_PID=""

cleanup() {
  kill "$O_PID" "$W_PID" "$R_PID" 2>/dev/null || true
  wait "$O_PID" "$W_PID" "$R_PID" 2>/dev/null || true
  rm -rf "$TMP"
}
trap cleanup EXIT

if [[ -z "$GGUF" ]]; then
  echo "FAIL: no GGUF found (set COMMUNITY_AI_TEST_GGUF)"
  exit 1
fi

RELAY_ARGS=()
if [[ "$USE_RELAY" == "1" ]]; then
  "$RELAY_BIN" --bind 127.0.0.1:3478 >"$TMP/relay.log" 2>&1 &
  R_PID=$!
  sleep 0.3
  RELAY_ARGS=(--relay 127.0.0.1:3478)
  echo "==> local opaque relay on 127.0.0.1:3478 (PROCESS VERIFIED path, not WAN)"
fi

W_PORT=50251
O_PORT=50252

echo "==> worker llama.cpp GGUF=$GGUF"
export RUST_LOG=info
stdbuf -oL -eL "$BIN" \
  --mode worker \
  --name wan-worker \
  --bind 127.0.0.1 \
  --port "$W_PORT" \
  --no-mdns \
  --no-stun \
  --identity "$TMP/worker.key" \
  --cache-dir "$TMP/worker-cache" \
  --model "$GGUF" \
  --model-id wan-gguf \
  "${RELAY_ARGS[@]+"${RELAY_ARGS[@]}"}" \
  >"$TMP/worker.log" 2>&1 &
W_PID=$!

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

if ! wait_log "$TMP/worker.log" "llama.cpp READY" 120; then
  echo "FAIL: worker model never READY"
  tail -n 80 "$TMP/worker.log" || true
  exit 1
fi

echo "==> originator (this host — PROCESS VERIFIED unless you overrode)"
RUST_LOG=info "$BIN" \
  --mode originator \
  --name wan-originator \
  --bind 127.0.0.1 \
  --port "$O_PORT" \
  --no-mdns \
  --no-stun \
  --identity "$TMP/orig.key" \
  --cache-dir "$TMP/orig-cache" \
  --peer "127.0.0.1:${W_PORT}" \
  --model-id wan-gguf \
  --prompt "Reply with one short sentence about rivers." \
  --max-tokens 32 \
  --wait-secs 60 \
  --report-json "$TMP/report.json" \
  "${RELAY_ARGS[@]+"${RELAY_ARGS[@]}"}" \
  >"$TMP/originator.log" 2>&1
O_RC=$?
cat "$TMP/originator.log"
if [[ "$O_RC" -ne 0 ]]; then
  echo "FAIL: originator exit $O_RC"
  tail -n 80 "$TMP/worker.log" || true
  exit 1
fi

if ! grep -q "CLASSIFICATION: PROCESS VERIFIED" "$TMP/originator.log"; then
  echo "WARN: expected PROCESS VERIFIED on loopback"
fi
if grep -q "PHYSICAL WAN VERIFIED" "$TMP/originator.log"; then
  echo "FAIL: harness must never self-stamp PHYSICAL WAN VERIFIED"
  exit 1
fi
if ! grep -q "completion=ok" "$TMP/originator.log"; then
  echo "FAIL: missing completion=ok"
  exit 1
fi
if grep -qi "Why do programmers prefer dark mode" "$TMP/originator.log"; then
  echo "FAIL: template output"
  exit 1
fi

echo
echo "HARNESS RESULT: PROCESS VERIFIED (loopback originator→worker llama.cpp)"
echo "PHYSICAL WAN VERIFIED: NO (same machine). See docs/testing/WAN_VALIDATION.md"
echo "PASS"
