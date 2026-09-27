# DECISIONS (Architecture Decision Records)

**Maintainer:** Senior Engineer  
**Updated:** 2026-09-25

Format: Problem → Constraints → Options → Decision → Why → Consequences → Status

**Governing invariant:** ADR-0011 — no mandatory central node.

---

## ADR-0001 — Native application shell

**Status:** ACCEPTED (design) — `community-app` + `apps/desktop` scaffold started (T-060)

**Problem:** Current “apps” require Chrome or Android WebView + Node. Mission forbids browser as core runtime.

**Constraints:** Shared Rust core; desktop + mobile; filesystem/GPU/networking access; maintainability.

**Options:** Chrome PWA; Electron; Tauri 2; Flutter; pure native multi-UI.

**Decision:**
- **Desktop (Linux/Windows/macOS):** Tauri 2 front-end calling Rust core.
- **Android / iOS:** Native Kotlin / SwiftUI shells calling the same Rust `cdylib` via proper JNI / C FFI.
- WebView/PWA retained only as **debug/migration** tooling, not Definition of Done.
- **Production P2P must not use** browser, Chrome app mode, browser WebSocket, browser WebRTC, or WebView as the networking layer.

**Why:** Tauri keeps one Rust core; mobile app stores require native shells anyway.

**Consequences:** New `apps/desktop` work; `dist/launch-app.sh` demoted; Android rewrite beyond WebView.

---

## ADR-0002 — Mesh transport

**Status:** ACCEPTED — amended 2026-09-25 (ADR-0011)

**Problem:** `P2PSwarm` uses `mpsc` channels; marketing claims QUIC/libp2p/WebRTC. Nothing real is wired.

**Constraints:** LAN offline with **zero extra hosts**; NAT is real; embed in mobile; encrypt; multiplex; no hub.

**Options:** libp2p full stack; quinn QUIC; WebRTC-primary; keep WS hub.

**Decision:**
- **Data plane:** Direct QUIC (`quinn`) between peers. All mesh frames travel A↔B, never through a coordinator.
- **LAN discovery:** optional mDNS / DNS-SD / platform NSD. **Not** the WAN foundation (ADR-0012).
- **Mesh growth:** signed **peer-hint gossip** (untrusted until handshake). TTL, hop limit, dedup, rate limits, multi-endpoint (listen / reflexive / relay).
- **Explicit dial:** `host:port` is an operator/bootstrap **hint**, not a coordinator.
- **STUN:** optional reflexive mapping of the QUIC socket. Not an authority.
- **WebRTC / browser stacks:** not production networking.
- **Bootstrap/rendezvous server:** **not required**. If present, first-packet help only — never membership, auth, scheduling, or models.

**Why:** Real sockets, native embed, honest P2P.

**Consequences:** Rewrite `community-network`; multi-process tests without TS coordinator.

---

## ADR-0003 — TypeScript coordinator (LEGACY)

**Status:** SUPERSEDED in role by ADR-0011; artifact disposition ACCEPTED

**Problem:** TS coordinator is a central WebSocket hub. It must not be the mesh.

**Decision:**
- **Not required** for discovery, auth, connections, models, tasks, inference, health, or scheduling.
- **Do not route** mesh traffic through it.
- Keep in-tree as **legacy** for the old worker-node path until Rust inference parity; then **REMOVE** or isolate so it cannot be mistaken for production.
- Mesh tests **must not** start it.

**Why:** Hub-and-spoke violates the invariant.

**Consequences:** Dual code until removal; documentation must say LEGACY.

---

## ADR-0004 — Inference runtime

**Status:** ACCEPTED (llama.cpp via `llama-server` b10632 — T-040)

**Problem:** Production chat uses `generateModelResponse` templates. Rust has only `SimulatedAIBackend`. Worker-node already proves llama.cpp works.

**Decision:**
- Production backend: **llama.cpp** via Rust FFI (`LlamaCppBackend`).
- `SimulatedAIBackend` allowed only under `sim` / test features — **forbidden in release app binaries**.
- Chat READY gate: hash + load + ≥1 smoke token.
- v1 distributed strategy: **remote full-model execution** coordinated **ephemerally** by the requesting peer (not a network master).

**Why:** Reliability first; existing worker-node is evidence llama.cpp is viable.

**Consequences:** Delete/disable template chat path (T-042); port GGUF download/verify; mobile may use smaller models or remote peers.

---

## ADR-0005 — Model readiness honesty

**Status:** ACCEPTED

**Problem:** `contributor.ts` marks models ready via `setTimeout` in 1.5s.

**Decision:** Any timer-based READY is a **defect**. READY requires verified artifacts + successful load smoke test. UI must show blocked/unavailable otherwise.

**Consequences:** Chat may stay disabled longer; UX must explain real download/load state.

---

## ADR-0006 — Metrics honesty

**Status:** ACCEPTED

**Problem:** DeviceResourcesPanel invents CPU via `Math.random`; NetworkPanel invents token balances.

**Decision:** Production UI shows only measured metrics or explicit “unavailable”. Token ledger postponed — remove fake balances from UI (show placeholder “Not implemented”).

**Consequences:** Less flashy UI; truthful product.

---

## ADR-0007 — Legacy disposition summary

| Asset | Disposition |
|-------|-------------|
| TS protocol / worker-node (llama) | **LEGACY** — binary layout reference only; no new production features (`packages/worker-node/LEGACY.md`) |
| TS **coordinator** | **LEGACY** — do not use for mesh; schedule REMOVE |
| Web PWA + WebLLM | KEEP for debug; not final runtime or mesh |
| `trystero` | REMOVE |
| `inferenceEngine.ts` prod path | REPLACE |
| Rust crypto/scheduler/governor | KEEP / extend (scheduler is **local** to a peer) |
| In-memory P2PSwarm | Test utility only; not production transport |
| SimulatedAIBackend | KEEP under `sim` only |
| Android WebView APK | MIGRATE to native+JNI |
| iOS Swift stubs | KEEP as seed; build real app |
| Chrome launch scripts | **LEGACY / DEPRECATED** (`dist/launch-app.sh`, `dist/start-wan-mesh.sh`) |

---

## ADR-0008 — QUIC TLS vs Ed25519

**Status:** ACCEPTED — `ADR-0008-quic-identity-binding.md`

Ephemeral self-signed TLS for confidentiality; Ed25519 handshake for identity.

---

## ADR-0011 — No central node

**Status:** ACCEPTED — `ADR-0011-no-central-node.md`

Governs all networking and scheduling work.

---

## ADR-0012 — WAN discovery, NAT, optional dumb relay

**Status:** ACCEPTED — `ADR-0012-wan-discovery-nat-relay.md`

WAN-first addressing, STUN, gossip endpoints, optional forward-only relay, identity ≠ endpoints. Closes ADR-0010.

---

## ADR-0013 — Peer-local storage (no central database)

**Status:** ACCEPTED — 2026-09-27

**Problem:** Conversations, tasks, and objects must survive restart without a cloud DB or a storage master node.

**Decision:**
- Each peer runs embedded SQLite (`community-storage`) plus a BLAKE3 object directory under the platform data dir.
- Chat/history is **private by default** and is not auto-replicated.
- Shared/public data uses signed append-only events (Ed25519 identity already in `community-security`). Event metadata is in the signed bytes.
- Future wallet/training events reuse the log; they are **not** implemented here. Resource sharing emits `ResourceSharingEnabled` / `Paused` / `ResourceCapabilityUpdated` only.
- Replication over QUIC is deferred; do not add HTTP/WebSocket storage.

**Why:** Matches ADR-0011 (no central authority). WAL SQLite is local, not a server.

**Consequences:** `CommunityApp` is the only production API into storage. Tauri must not open the `.db` file. TOKEN_STREAM UI is still incomplete; storage commits completed assistant text.

---

## Pending decisions

| ID | Topic | Status |
|----|-------|--------|
| ADR-0009 | Default on-device model for mobile | Open |
| ADR-0010 | Optional NAT relay | **SUPERSEDED by ADR-0012** |

Implementation agents must escalate architecture conflicts to the Senior Engineer. Violating ADR-0011 is an automatic stop.
