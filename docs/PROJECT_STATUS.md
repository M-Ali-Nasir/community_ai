# PROJECT STATUS

**Updated:** 2026-09-25  
**Owner:** Manager Agent  
**Phase:** Stage 1 WAN inference validation — **PROCESS VERIFIED**, **PHYSICAL WAN NOT TESTED**  
**Product claim vs reality:** Native originator can get **real llama.cpp tokens** from a worker over QUIC on **one machine**, with DIRECT vs RELAY labeled. Two different public ISPs have **not** been used.

---

## Product statement

> Community AI is designed for decentralized Internet-wide peer-to-peer operation. LAN discovery is an optimization, not the architectural foundation.

There is no permanent coordinator. A peer coordinates only a task it originated.

---

## Executive summary

1. **Legacy TypeScript hub** — LEGACY. Not the production mesh.
2. **Rust core:** Ed25519 identity, `quinn` QUIC, optional mDNS, WAN endpoints (listen / STUN / relay), gossip, originator reassignment, `llama-server` b10632.
3. **Native API:** `crates/community-app`. Daemon `--mode originator|worker`. Tauri window **not** CI-built.
4. **Relay:** QUIC via local opaque relay is **PROCESS VERIFIED** (`two_peers_quic_through_opaque_relay`). Not a coordinator. WAN relay **NOT TESTED**.

---

## Progress by mission phase

| Phase | Name | Status |
|-------|------|--------|
| 0 | Audit | **DONE** |
| 1 | Architecture | **LOCKED** — ADR-0011 + ADR-0012 WAN-first |
| 2 | Real P2P | **PROCESS-LEVEL VERIFIED** — physical LAN **NOT TESTED** |
| 3 | WAN-first P2P / Stage 1 validation | **IN PROGRESS** — harness + process evidence; **PHYSICAL WAN NOT TESTED** |
| 3 (old) | Model system | **PARTIAL** — READY = load + smoke; no P2P model download |
| 4 | Real inference | **PARTIAL** — llama.cpp tokens over mesh; no native chat UI |
| 5 | Distributed execution | **PARTIAL** — remote full-model + originator reassign; **no layer-split** |
| 6 | Native platforms | **STARTED** (`community-app`); Tauri window / mobile **NOT TESTED** |
| 7–10 | UI / security / packaging | Open |

---

## Mesh validation (honest)

See `docs/testing/MESH_VALIDATION.md`.

| Class | Status |
|-------|--------|
| PROCESS VERIFIED (loopback / one host) | QUIC mesh, gossip, llama tokens, reassign, relay opaque forward, identity≠port |
| NETWORK EMULATED | Script present; **NOT RUN** unless `tc` + privileges |
| PHYSICAL WAN VERIFIED | **NOT TESTED** |
| Physical LAN two machines | **NOT TESTED** |

---

## Blockers (open)

1. **WAN PHYSICAL TEST — NOT TESTED** — do not fabricate (T-119).
2. **Tauri window** not built in this environment (webkit/CLI).
3. **Mobile** JNI/Swift mesh bind not physically tested.
4. Physical multi-machine LAN still unused (not a Phase 3 gate).

---

## Evidence artifacts

| Artifact | Path |
|----------|------|
| WAN validation | `docs/testing/WAN_VALIDATION.md` |
| WAN harness | `scripts/wan-inference-harness.sh` |
| Mesh tests | `docs/testing/MESH_TEST.md` |
| Mesh validation | `docs/testing/MESH_VALIDATION.md` |
| Inference tests | `docs/testing/INFERENCE_TEST.md` |
| Implementation matrix | `docs/IMPLEMENTATION_MATRIX.md` |
| Task board | `docs/TASK_BOARD.md` |
