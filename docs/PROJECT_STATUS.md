# PROJECT STATUS

**Updated:** 2026-09-26  
**Owner:** Manager Agent  
**Phase:** Stage 1B — native application verification; network **frozen**  
**WAN claim:** `PHYSICAL WAN VERIFIED — NOT TESTED` (B-010 open)

---

## Product statement

> Community AI is designed for decentralized Internet-wide peer-to-peer operation. LAN discovery is an optimization, not the architectural foundation.

There is no permanent coordinator. A peer coordinates only a task it originated.

---

## Evidence classes (mandatory)

| Class | Meaning |
|-------|---------|
| **VERIFIED** | Actual evidence exists for that exact claim |
| **PROCESS VERIFIED** | Multiple local/native processes (same host / loopback) |
| **NETWORK EMULATED** | Simulated network conditions (`tc` / netem); not physical WAN |
| **PHYSICAL WAN VERIFIED** | Two independent public Internet connections |
| **NOT TESTED** | No evidence |

Do **not** promote WAN from localhost, loopback, multi-process same host, same LAN, compile success, or STUN/relay unit tests.

---

## Stage 1 gate

| Class | Status |
|-------|--------|
| PROCESS VERIFIED | QUIC mesh, STUN client, opaque relay, DIRECT/RELAY classify, llama.cpp GGUF tokens, reassignment (one host); native `community-app` peer views + chat failure path |
| NETWORK EMULATED | Scripts exist; do not invent results |
| PHYSICAL WAN VERIFIED | **NOT TESTED** |

**Network feature freeze:** no new discovery/relay/coordinator/protocol/WebRTC/WebSocket control plane unless a real WAN test exposes a concrete defect.

---

## Two tracks

| Track | Focus | Status |
|-------|--------|--------|
| **A — WAN validation** | B-010 / T-119 physical two-ISP test | **BLOCKED BY TEST ENVIRONMENT** |
| **B — Product** | T-070 / T-060 / T-043 native app over Rust core | **IN PROGRESS** |

Track B is **not** blocked on B-010. Layer-split remains **DEFERRED**.

---

## Desktop platform status

Do not collapse this into “desktop supported.”

| OS | BUILD VERIFIED | RUNTIME VERIFIED | PHYSICAL TESTED |
|----|----------------|------------------|-----------------|
| Linux | **NOT TESTED** (Tauri CLI/webkit not confirmed this gate) | **NOT TESTED** | **NOT TESTED** |
| Windows | **NOT TESTED** | **NOT TESTED** | **NOT TESTED** |
| macOS | **NOT TESTED** | **NOT TESTED** | **NOT TESTED** |

Native UI + IPC + Rust API exist in source. That is **IMPLEMENTED**, not BUILD/RUNTIME verified.

## Mobile status

| OS | Status |
|----|--------|
| Android | **NOT PHYSICALLY TESTED** — do not claim support |
| iOS | **NOT PHYSICALLY TESTED** — do not claim support |

Eventually each mobile OS needs BUILD / RUNTIME / P2P / INFERENCE verified. Shared Rust core stays reusable.

---

## Executive summary

1. **Legacy TypeScript hub / worker-node / `start-wan-mesh.sh`** — LEGACY / DEPRECATED. Not the production mesh.
2. **Rust core:** Ed25519, `quinn` QUIC, optional mDNS/STUN/relay — **frozen** pending WAN evidence.
3. **Native API:** `community-app` views for peers/network/models/tasks/chat. No CPU/memory graphs (no honest UI source yet).
4. **Chat:** Tauri `chat` → mesh → llama.cpp. Live window GGUF **NOT VERIFIED**. Live UI token stream **missing** (T-043). Failure → `TASK_ERROR` / `TASK_TIMEOUT`.
5. **WAN:** `PHYSICAL WAN VERIFIED — NOT TESTED`.

---

## Progress by mission phase

| Phase | Name | Status |
|-------|------|--------|
| 0 | Audit | **DONE** |
| 1 | Architecture | **LOCKED** — ADR-0011 + ADR-0012 |
| 2 | Real P2P | **PROCESS VERIFIED** — physical LAN **NOT TESTED** |
| 3 | WAN / Stage 1 | Architecture **IMPLEMENTED**; physical WAN **NOT TESTED**; **network frozen** |
| 4 | Real inference | **PROCESS VERIFIED** on mesh; native chat **IN PROGRESS** |
| 5 | Distributed execution | Remote full-model + reassign; **no layer-split** |
| 6 | Native platforms | **IN PROGRESS** — see platform tables above |
| 7–10 | Packaging / security | Open |

---

## Blockers (open)

1. **B-010 WAN PHYSICAL** — NOT TESTED (Track A).
2. **B-003 Tauri window** — IMPLEMENTED in source; BUILD/RUNTIME **NOT TESTED** this gate.
3. **B-004 Mobile** — **NOT PHYSICALLY TESTED**.
4. Physical multi-machine LAN — NOT TESTED (not a Stage 1 gate).

---

## Evidence artifacts

| Artifact | Path |
|----------|------|
| Native UI state matrix | `docs/testing/NATIVE_UI_STATE.md` |
| WAN validation | `docs/testing/WAN_VALIDATION.md` |
| WAN harness | `scripts/wan-inference-harness.sh` |
| Task board | `docs/TASK_BOARD.md` |
| Blockers | `docs/BLOCKERS.md` |
