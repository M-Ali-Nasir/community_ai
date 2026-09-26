# Native UI state binding (T-070)

**Updated:** 2026-09-27  
**Rule:** A missing metric is preferable to invented data. UI values come from `community-app` views over Tauri IPC.

Evidence class for these checks: **PROCESS VERIFIED** (in-process / two local `CommunityApp` sessions). **PHYSICAL WAN VERIFIED — NOT TESTED.**

## Source matrix

| UI value | Source | Real runtime? | Verified? |
|----------|--------|---------------|-----------|
| Local peer ID | `NodeIdentity` / `session_view` | YES | YES (`app_starts_without_browser`) |
| Remote peer ID | `MeshSwarm::snapshots` | YES | YES (`peers_view_tracks_connect_and_disconnect`) |
| Peer count / ready peers | `ready_count` / snapshots | YES | YES (same) |
| Connection state | `PeerState` from mesh | YES | YES (READY → DISCONNECTED/CONNECTING) |
| Endpoints | `NetEndpoint` advertised + session addr | YES | YES (process) |
| DIRECT / RELAY | `ConnectionMode` from mesh | YES | YES (mode from mesh; WAN still NOT TESTED) |
| RTT | ping/pong `latency_ms` on snapshot | YES when measured, else omitted (`—`) | YES (omission allowed) |
| Evidence class | `evidence_class_for` | YES | YES (loopback = PROCESS_VERIFIED) |
| WAN banner | constant `WAN_STATUS_NOT_TESTED` | YES (honest constant) | YES (never auto-promoted) |
| Model id / state | `ModelAdvertisement` on local + peer profiles | YES | YES (empty until advertised; no timer READY) |
| Model available | `state.can_serve()` | YES | YES (shown as yes/no, not a fake READY badge) |
| Tasks | originator `tasks` list in `CommunityApp` | YES | YES (`chat_without_ready_worker_is_task_error`) |
| Chat result / tokens | `collect_inference_report` → llama.cpp | YES when a READY worker exists | PARTIAL — failure path PROCESS VERIFIED; live GGUF through **this** UI API **NOT RUN** in this gate |
| TOKEN_STREAM in UI | QUIC events exist in core; IPC returns completed text | Core YES / UI live stream NO | **NOT VERIFIED** (T-043 remaining) |
| Conversation list / titles | `community-storage` conversations | YES | YES (`chat_persists_locally_and_survives_restart`, `conversations_are_isolated_between_peers`) |
| Chat messages after restart | SQLite messages table | YES | YES (same persist test) |
| Generating indicator | UI wait state while IPC `chat` runs; no invented tokens | YES (honest wait) | Source-only; Tauri window RUNTIME **NOT TESTED** |
| Resource sharing ACTIVE/PAUSED | `ResourceSharingConfig` + mesh advertisement | YES | YES (`resource_sharing_defaults_paused_and_survives_restart`, swarm pause/accept tests) |
| CPU/RAM/GPU snapshot | `HardwareSnapshot::detect` (`sysinfo`; GPU fields None unless detected) | YES (CPU/RAM); GPU none unless detected | YES (GPU stays None in process tests) |
| Wallet / credits | — | NO | **NOT IMPLEMENTED** (label only) |
| CPU % graph | — | NO | **REMOVED** from native UI |
| Memory graph | — | NO | **REMOVED** from native UI |
| Fake peers | — | NO | **ABSENT** on native path |
| `inferenceEngine.ts` / WebRTC / coordinator | — | NO | Native app does not import them |

## Chat path (production native)

```text
Tauri UI → Tauri IPC `chat` → community-app::chat
  → MeshSwarm::collect_inference_report → peer QUIC task
  → llama.cpp TOKEN_STREAM on the worker
  → completed text + attempts back to UI
```

Missing integration (do not fake as done): **live token append in the Tauri window** (T-043 / T-041). Failure shows `TASK_ERROR` or `TASK_TIMEOUT` instead of spinning forever.

## Desktop / mobile claims

See `docs/PROJECT_STATUS.md`. Linux/Windows/macOS are not “desktop supported” until each of BUILD / RUNTIME / PHYSICAL is recorded. Android/iOS remain **NOT PHYSICALLY TESTED**.
