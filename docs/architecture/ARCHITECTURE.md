# ARCHITECTURE — Community AI Target (Senior Engineer)

**Status:** LOCKED (decentralization invariant) + WAN-first (ADR-0012)  
**Date:** 2026-09-25  
**Author role:** Senior Engineer / System Architect  
**Based on:** Phase 0 audit + no-central-node + WAN-first product requirement

---

## Architectural invariant (non-negotiable)

> **Community AI is a peer-to-peer system. No central device or server is required for peer discovery, peer communication, model exchange, task execution, or normal network operation. Any peer may temporarily coordinate an individual task, but no peer is permanently authoritative over the network.**

> **Community AI is designed for decentralized Internet-wide peer-to-peer operation. LAN discovery is an optimization, not the architectural foundation.**

If a proposed implementation violates this invariant: **STOP**, update an ADR, do not ship a workaround.

Persistence is **peer-local** (`docs/architecture/STORAGE.md`). There is no central chat history or storage node.

The architecture is:

**Decentralized / peer-to-peer mesh, with optional infrastructure only where technically necessary for connectivity (NAT), never for authority.**

Do **not** describe this as a “hybrid coordinator” system. A hybrid *control plane* is forbidden.

---

## Planes (do not conflate)

| Plane | What it is | What it is not |
|-------|------------|----------------|
| **Discovery** | Operator `--peer`, signed gossip, optional STUN, optional mDNS, optional dumb relay | Not a membership server |
| **Peer communication** | Direct QUIC A↔B (relay only forwards opaque UDP) | Not traffic through a hub |
| **Task coordination** | Ephemeral: the peer that originated a job may coordinate **that job only** | Not a master node |
| **Optional NAT relay** | Dumb encrypted-transport forwarder; network must work without it | Not identity, scheduling, models, or membership |

---

## 1. Problem

The repository currently implements (or markets) a hub-and-spoke TypeScript coordinator plus simulated peers. That topology is **rejected** for production.

Required topology (full mesh of equals):

```text
                 ┌─────────────┐
                 │   PEER A    │
                 └──┬──────┬───┘
                    │      │
              QUIC  │      │ QUIC
                    │      │
             ┌──────▼─┐  ┌─▼──────┐
             │ PEER B │  │ PEER C │
             └────┬───┘  └───┬────┘
                  │           │
                  └─────┬─────┘
                        │
                   ┌────▼─────┐
                   │  PEER D  │
                   └──────────┘
```

Forbidden for normal operation:

```text
                    CENTRAL SERVER
                   /      |       \
                 A        B        C
```

---

## 2. Constraints

| Constraint | Implication |
|------------|-------------|
| No mandatory central node | Every instance is a peer; all listed peer capabilities must be implementable locally |
| LAN without Internet | mDNS **may** be used; not required if `--peer` / gossip exist |
| NAT is real | STUN + simultaneous dial; optional dumb relay; never authority |
| No browser in production network | QUIC stack lives in Rust core (`community-app` / daemon), not Chrome/WebView/WebRTC |
| WAN is the product | Do not gate progress on physical LAN machines |
| No central database | Each peer stores its own view of peers/models/tasks |
| No fake success | READY only after authenticated handshake + capabilities |

Every peer must be able to: discover, authenticate, connect, advertise capabilities/resources/models, request/serve model data, submit/accept tasks, run inference, return results, participate in scheduling, detect and recover from peer failure.

---

## 3. Native shell (unchanged)

| Option | Verdict |
|--------|---------|
| Chrome `--app` / WebView as production runtime | **Rejected** |
| **Tauri 2 + Rust core (desktop)** | **Chosen** |
| **Kotlin / SwiftUI + same Rust cdylib (mobile)** | **Chosen** |

---

## 4. Transport decision

| Option | Verdict |
|--------|---------|
| TypeScript WebSocket coordinator as mesh | **Rejected** — hub topology |
| Browser WebRTC as production mesh | **Rejected** |
| **QUIC (`quinn`) + gossip + optional STUN/relay + optional mDNS** | **Chosen** (ADR-0012) |
| Required bootstrap / rendezvous server | **Rejected** as authority; `--peer` is only a first-dial hint |
| Optional NAT relay (forward-only) | **Specified** — `community-relay`; not required |

---

## 5. System shape (no control-plane hub)

```text
┌─────────────────────────────────────────────────────────────┐
│                     Native UI shell                          │
│   Desktop: Tauri 2    Android: Kotlin    iOS: SwiftUI        │
└───────────────────────────┬─────────────────────────────────┘
                            │ FFI / IPC
┌───────────────────────────▼─────────────────────────────────┐
│                 Shared Rust Core (every peer)                │
│  identity │ WAN discovery │ NAT/STUN │ QUIC │ gossip │ governor │    │
│  models │ local scheduler │ runtime(llama.cpp) │ protocol │ security   │
└───────────────────────────┬─────────────────────────────────┘
                            │ direct QUIC (data plane)
              ┌─────────────┼─────────────┐
              ▼             ▼             ▼
           Peer A        Peer B        Peer C
```

There is **no** bootstrap box on the required path.

**TypeScript coordinator:** LEGACY. Not on the mesh data plane. Not required to start daemons. Candidate for removal after worker-node parity (see ADR-0011 / ADR-0003 superseded).

---

## 6. Discovery (WAN-first)

Primary (Internet):

```text
A starts → persistent Ed25519 identity → QUIC listen (one UDP socket)
Optional STUN → reflexive endpoint advertised in Hello
Operator `--peer B` and/or gossip from an existing peer
A dials B candidates (listen, reflexive, relay) → handshake → READY
B gossips C's signed hints → A dials C
No membership server. No central scheduler.
```

LAN mDNS (`_community-ai._udp.local.`) is **optional**. `--no-mdns` is supported. Do not treat LAN validation as a product gate.

**Peer-hint gossip:** untrusted until handshake. hop ≤ 3, TTL, dedup, dial rate limits, max peer table. Endpoints can change; identity cannot.

**NAT:** cone NATs: simultaneous QUIC dial of reflexive addresses. Symmetric/CGNAT: optional `community-relay` (opaque UDP). Direct always preferred.

---

## 7. Scheduling (ephemeral task coordinator)

A user request on peer A may make **A the coordinator of that task only**. B/C/D may execute work. When the task ends, A has no special network role.

A later request on B is coordinated by B.

There is no central scheduler process and no permanent master.

---

## 8. Resource and model information

Peers send `capabilities` / `resource-report` on the QUIC session. Each peer keeps a **local** table. No global DB.

---

## 9. Failure

If B disappears: QUIC `connection.closed` / stale timeout. A and C remain connected. The **originating** peer fails the task (`TASK_ERROR` / `TASK_TIMEOUT` / worker-gone) and may reassign to another eligible peer. There is no global scheduler.

---

## 10. Core modules

Unchanged crate split. `community-scheduler` runs **in-process on the requesting peer**, not as a network service.

Release builds must not compile `SimulatedAIBackend` (`prod` vs `sim` features).

---

## 11. Protocol v1 (transport freeze + gossip)

See `docs/architecture/PROTOCOL.md`.

Handshake: Hello → version → AuthChallenge/Response → Capabilities → READY.

Plus `peer-gossip` (WAN endpoints), STUN/relay advertisements, and remote full-model task frames (`TASK_OFFER` … `TASK_TIMEOUT`). Layer-split is **not** in this phase.

---

## 12. Peer lifecycle

```text
Discovered → Connecting → Authenticating → Connected → Ready
Ready ↔ Busy
Ready|Busy → Degraded → Disconnected → (backoff) Connecting
```

---

## 13. Inference (later phases; reliability first)

1. Local if model READY (load + smoke; no timers).
2. Else remote **full-model** execution on a READY peer (originating peer coordinates that job; may reassign A→C).
3. Layer-split experimental later (Stage 3). Stage 1 WAN A→B tokens must be robust first.

---

## 14. Security

Ed25519 persistent identity; identity ≠ endpoints; QUIC TLS for confidentiality (ADR-0008); application handshake; signed frames + timestamp skew; gossip/STUN/relay hints untrusted until connect; size/rate/task/connection limits.

---

## 15. Migration

1. Real QUIC mesh + identity + WAN addressing + gossip in `community-daemon` (no coordinator).
2. llama.cpp in Rust; chat without templates.
3. Remote tasks + originator reassignment.
4. Native shells (`community-app` / Tauri); WebView/Chrome demoted.
5. Remove or quarantine TS coordinator so it cannot be mistaken for production networking.

---

## 16. Acceptance (decentralized network)

Three independent daemon processes. No coordinator, no browser, no cloud.

A, B, C discover and authenticate; full mesh QUIC; capability exchange; gossip can introduce the third edge; kill B; A–C survive.

---

## 17. Non-goals (90 days)

- Token/blockchain ledger
- Claiming phone-side 14B layer-split
- Browser as production runtime
- Required WAN membership server
- Claiming physical WAN or mobile support without device evidence
- Fancy UI / layer-split before Stage 1 WAN inference is robust

---

## 18. Mobile (same core, unclaimed until tested)

Android and iOS must use **this** identity / discovery / QUIC / NAT / gossip / task / llama stack via `community-ffi` / `community-app` — not a second networking implementation.

Known platform limits (do not hide):

- iOS background execution and UDP may require the optional relay more often.
- Android NSD is not a WAN mechanism; WAN still uses `--peer` / gossip / STUN / relay.
- Cellular CGNAT often blocks inbound hole punching.

**Android / iOS physical WAN: NOT TESTED. Do not claim mobile support.**

