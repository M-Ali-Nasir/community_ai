# TASK BOARD

**Updated:** 2026-09-25  
**Owner:** Manager Agent  
**Rule:** Do not start UI/P2P cosmetics before protocol + transport exist. Do not mark DONE without tests on real behavior.

Status values: `BACKLOG` | `READY` | `IN_PROGRESS` | `BLOCKED` | `DONE` | `CANCELLED`

---

## Phase 0 — Audit

| ID | Description | Priority | Deps | Owner | Status | Acceptance | Tests | Affected |
|----|-------------|----------|------|-------|--------|------------|-------|----------|
| T-000 | Full repository audit + matrix | P0 | — | Manager | **DONE** | Matrix classifies all major components | N/A (doc) | `docs/IMPLEMENTATION_MATRIX.md` |
| T-001 | Project status + task board | P0 | T-000 | Manager | **DONE** | Status reflects reality | N/A | `docs/PROJECT_STATUS.md`, this file |
| T-002 | Senior architecture + ADRs | P0 | T-000 | Senior Eng | **DONE** | ADRs cover native shell, transport, **no central node**, inference | N/A | `docs/architecture/ARCHITECTURE.md`, `docs/decisions/DECISIONS.md`, ADR-0011 |

---

## Phase 1 — Architecture lock (no feature coding beyond scaffolds)

| ID | Description | Priority | Deps | Owner | Status | Acceptance | Tests | Affected |
|----|-------------|----------|------|-------|--------|------------|-------|----------|
| T-010 | Freeze protocol v1 message set | P0 | T-002 | Senior + P2P | **DONE** | Versioned MeshFrame + handshake/gossip/echo | `community-protocol` tests | `crates/community-protocol`, `docs/architecture/PROTOCOL.md` |
| T-011 | Define peer state machine | P0 | T-002 | P2P | **DONE** | Documented states + timeouts | Unit tests for Ready/disconnect | `community-network` state + PROTOCOL.md |
| T-012 | Define runtime trait + readiness contract | P0 | T-002 | AI Eng | **DONE** | READY iff llama.cpp load + smoke token | `LlamaServerEngine::start` | runtime |
| T-013 | Define platform capability interface | P0 | T-002 | Native | READY | CPU/RAM/GPU/battery/thermal collectors | Fake-sensor banned in prod | governor, platforms |

---

## Phase 2 — Real P2P (first implementation wave)

| ID | Description | Priority | Deps | Owner | Status | Acceptance | Tests | Affected |
|----|-------------|----------|------|-------|--------|------------|-------|----------|
| T-020 | Persistent Ed25519 identity on disk | P0 | T-010 | P2P | **DONE** | Same key across restarts | Roundtrip load/save | security, daemon |
| T-021 | Replace in-memory swarm with real QUIC transport | P0 | T-010, T-011 | P2P | **DONE** | Two/three processes bind/dial; signed HELLO; gossip; failure | `community-network` tests + `scripts/mesh-three-peer-test.sh` | `community-network`, daemon |
| T-022 | LAN discovery (mDNS/DNS-SD) | P1 | T-021 | P2P | **PARTIAL** | Optional LAN optimization; not a WAN gate | Desktop mDNS adapter; physical LAN **NOT TESTED** | network, daemon |
| T-023 | Optional NAT relay (forward-only, never authority) | P0 | T-021, ADR-0012 | P2P | **PARTIAL** | Opaque UDP forwarder; mesh works without it | `std_relay_forwards_opaque_bytes` **PROCESS VERIFIED**; WAN relay **NOT TESTED** | `community-network` relay + `community-relay` bin |
| T-024 | Heartbeat, stale drop, reconnect/backoff | P0 | T-021 | P2P | **DONE** | Peer leaves within timeout; mesh survives | `peer_disappear_mesh_survives` + three-daemon script | network |
| T-025 | Capability + resource_report gossip | P0 | T-021, T-013 | P2P | **IN_PROGRESS** | Capabilities exchanged at handshake; resource-report on wire | Handshake tests | network, governor |
| T-026 | Remove synthetic peers from UI path | P0 | T-021 | Native/UI | BACKLOG | Network panel shows only verified peers | Manual + e2e | web/Tauri UI |

---

## Phase 3 — Model system

| ID | Description | Priority | Deps | Owner | Status | Acceptance | Tests | Affected |
|----|-------------|----------|------|-------|--------|------------|-------|----------|
| T-030 | Manifest format + hash list | P0 | T-012 | AI Eng | BACKLOG | Manifest schema documented | Unit | model-manager |
| T-031 | Chunk download + verify + resume | P0 | T-030, T-021 | AI Eng | BACKLOG | Corrupt chunk rejected; resume works | Integration | model-manager, network |
| T-032 | Disk cache + eviction + space checks | P1 | T-031 | AI Eng | BACKLOG | Cache survives restart | Unit/integration | model-manager |
| T-033 | READY only after load smoke test | P0 | T-031, T-040 | AI Eng | **DONE** (native) | No timer-based READY on mesh path; PWA timers removed | llama smoke + contributor.ts | runtime, web |

---

## Phase 4 — Real inference + chat

| ID | Description | Priority | Deps | Owner | Status | Acceptance | Tests | Affected |
|----|-------------|----------|------|-------|--------|------------|-------|----------|
| T-040 | Llama.cpp via existing `llama-server` (worker-node b10632) | P0 | T-012 | AI Eng | **DONE** | Real tokens from GGUF | `remote_full_model_llama_tokens` | `community-runtime` |
| T-041 | Stream + cancel API | P0 | T-040 | AI Eng | **PARTIAL** | Tokens stream on QUIC; cancel/worker-gone | `worker_disappear_fails_task` | runtime, network |
| T-042 | Delete/disable `generateModelResponse` production path | P0 | T-040 | AI Eng | **DONE** | Function throws; PWA submit does not stream templates | Source | `inferenceEngine.ts` |
| T-043 | Wire native UI chat → mesh task → llama.cpp | P0 | T-040, T-042 | AI + Native | BACKLOG | Prompt → real tokens in native UI | E2E | UI, daemon |
| T-044 | Reuse worker-node llama.cpp binaries | P1 | T-040 | AI Eng | **DONE** | Same `~/.community-ai/llama/b10632-*` layout | Engine start | worker-node → Rust |

---

## Phase 5 — Distributed execution

| ID | Description | Priority | Deps | Owner | Status | Acceptance | Tests | Affected |
|----|-------------|----------|------|-------|--------|------------|-------|----------|
| T-050 | Task offer/accept/reject/stream/result/error | P0 | T-021, T-040 | P2P + AI | **DONE** | Remote peer executes full-model task | `remote_full_model_llama_tokens` | protocol, network |
| T-051 | Reliable first: remote full-model (no layer-split) | P0 | T-050 | Senior | **DONE** | Originating peer picks a READY worker | Same | scheduler/network |
| T-052 | Layer-split only when proven | P2 | T-051 | AI Eng | BACKLOG | Documented as experimental until stable | Benchmarks | runtime |
| T-053 | Failure: worker gone → originator reassigns A→C | P0 | T-050 | P2P | **PARTIAL** | Detect death; retry next eligible peer (no global scheduler) | `worker_disappear_fails_task`, `reassign_tries_next_peer_on_failed_result`, `reassign_skip_rejecting_peer_then_llama` | network |

---

## Phase 6 — Native platforms

| ID | Description | Priority | Deps | Owner | Status | Acceptance | Tests | Affected |
|----|-------------|----------|------|-------|--------|------------|-------|----------|
| T-060 | Tauri 2 Linux desktop shell (no Chrome dep) | P0 | T-021 | Native | **IN_PROGRESS** | API in `community-app`; Tauri scaffold in `apps/desktop` | `app_starts_without_browser`; Tauri window **NOT BUILT** | `community-app`, `apps/desktop` |
| T-061 | Windows + macOS Tauri packages | P1 | T-060 | Native | BACKLOG | Installers build in CI | Build | desktop |
| T-062 | Android: Rust `.so` + real JNI + foreground service | P0 | T-021, T-040 | Native | BACKLOG | Native activity uses core; WebView optional | Emulator test | `platform/android` |
| T-063 | iOS: XCFramework + minimal SwiftUI shell | P1 | T-021 | Native | BACKLOG | Builds; discovery constrained documented | Device/sim | `platform/ios` |
| T-064 | Fix systemd/launchd CLI flags | P0 | T-021 | Native | BACKLOG | Units start daemon successfully | Install test | linux/macos |
| T-065 | Retire Chrome `launch-app.sh` as primary | P1 | T-060 | Manager | BACKLOG | README points to native app | Doc | dist/, README |

---

## Phase 7–10 (summarized backlog)

| ID | Description | Priority | Status |
|----|-------------|----------|--------|
| T-070 | UI binds only to real peer/resource/model state | P0 | BACKLOG |
| T-080 | Adversarial security review | P1 | BACKLOG |
| T-090 | Three-node reproducible mesh + inference lab | P0 | BACKLOG |
| T-100 | Production installers + signed APK | P1 | BACKLOG |

---

## Phase 3 — WAN-first P2P (current)

> Community AI is designed for decentralized Internet-wide peer-to-peer operation. LAN discovery is an optimization, not the architectural foundation.

| ID | Description | Priority | Deps | Owner | Status | Acceptance | Tests | Affected |
|----|-------------|----------|------|-------|--------|------------|-------|----------|
| T-110 | ADR WAN discovery / NAT / relay (no authority) | P0 | T-002 | Senior | **DONE** | ADR-0012 accepted | N/A (doc) | `docs/decisions/ADR-0012-*.md` |
| T-111 | Identity ≠ endpoints (`NetEndpoint`) | P0 | T-110 | P2P | **DONE** | Hello/gossip carry listen/reflexive/relay | `prefers_direct_before_relay`, `identity_stable_when_listen_port_changes` | protocol, network |
| T-112 | STUN reflexive discovery | P0 | T-111 | P2P | **PARTIAL** | Same UDP socket; failure non-fatal | XOR decode **PROCESS VERIFIED**; live STUN **NOT TESTED** (optional) | `stun.rs` |
| T-113 | WAN gossip + dial rate limits | P0 | T-111 | P2P | **DONE** (process) | Multi-endpoint hints; untrusted until handshake | existing gossip tests + limits in swarm | network |
| T-114 | Originator task reassignment | P0 | T-050 | P2P | **DONE** (process) | A→B fail → A→C; no global scheduler | `reassign_*` | network |
| T-115 | Task protocol origin/job/attempt/timeout | P0 | T-050 | P2P | **DONE** | Duplicate attempt rejected | unit + mesh tests | protocol, network |
| T-116 | Hostile-Internet limits (skew, replay, size, rate, peers, tasks) | P0 | T-021 | P2P | **PARTIAL** | Documented + enforced locally | `stale_timestamp_is_rejected` | protocol, network |
| T-117 | RTT / local worker preference | P1 | T-024 | P2P | **PARTIAL** | Ping/pong RTT on snapshots | process | network |
| T-118 | Native `community-app` API | P0 | T-021 | Native | **DONE** (process) | Start mesh without browser | `app_starts_without_browser` | `community-app` |
| T-119 | Physical WAN A↔B tokens | P0 | T-112, T-040 | QA | **NOT TESTED** | Two public networks, real GGUF tokens | — | — |
| T-120 | WAN originator/worker harness + connection_mode | P0 | T-040 | P2P | **DONE** (process) | `--mode originator\|worker`; DIRECT vs RELAY visible; never self-stamps WAN | `wan-inference-harness.sh`, `two_peers_quic_through_opaque_relay` | daemon, network |
| T-121 | Hostile-Internet task/origin checks | P0 | T-116 | P2P | **DONE** (process) | Spoof origin, oversized prompt, malformed task | `spoofed_origin_id_is_rejected`, `oversized_and_malformed_tasks_rejected` | network |

Do **not** mark T-119 DONE without Internet evidence.

---

## Assignment gate

**No agent may start T-026 / T-070 (UI cosmetics) before T-021 lands.**  
**Layer-split (T-052) is forbidden until remote full-model is reliable (T-050).**  
**Do not claim “P2P complete” without T-021 + process-level tests. Physical LAN is still NOT TESTED. WAN PHYSICAL TEST — NOT TESTED.**
