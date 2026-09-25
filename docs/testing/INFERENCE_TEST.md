# Remote full-model inference test

No coordinator. No browser. No `SimulatedAIBackend`.

> Community AI is designed for decentralized Internet-wide peer-to-peer operation. LAN discovery is an optimization, not the architectural foundation.

## Commands

```bash
cargo test -p community-protocol --lib task
cargo test -p community-network --lib template_engine_result_is_rejected
cargo test -p community-network --lib remote_full_model_llama_tokens -- --nocapture
cargo test -p community-network --lib worker_disappear_fails_task -- --nocapture
cargo test -p community-network --lib reassign_skip_rejecting_peer_then_llama -- --nocapture
cargo test -p community-network --lib loopback_connection_is_direct_and_process_verified
cargo test -p community-network --lib two_peers_quic_through_opaque_relay
./scripts/wan-inference-harness.sh
```

`remote_full_model_llama_tokens` / harness require `llama-server` (b10632) and a GGUF under `community-ai/models/`.

A fake/template result **must fail**.

## Labels

| Case | Status |
|------|--------|
| A→B llama tokens (loopback) | **PROCESS VERIFIED** |
| DIRECT vs RELAY distinguished | **PROCESS VERIFIED** |
| Originator/worker CLI harness | **PROCESS VERIFIED** (`scripts/wan-inference-harness.sh`) |
| B dies mid-task | **PROCESS VERIFIED** |
| Reassign A→C | **PROCESS VERIFIED** |
| **WAN A→Internet→B llama tokens** | **NOT TESTED** — see `docs/testing/WAN_VALIDATION.md` |

Do not treat the harness as PHYSICAL WAN VERIFIED. Same host / loopback is PROCESS VERIFIED only.
