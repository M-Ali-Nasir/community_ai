# Community AI native desktop (Tauri 2)

Production shell for **Track B**. Calls the Rust core (`community-app` → `MeshSwarm`). Does **not** use Chrome app mode, WebRTC, WebSocket coordinator, or `inferenceEngine.ts`.

## Status

| Item | Status |
|------|--------|
| Application API (`crates/community-app`) | **PROCESS VERIFIED** |
| UI binds to Rust state (T-070) | **IN_PROGRESS** — matrix in `docs/testing/NATIVE_UI_STATE.md` |
| UI panels | **IMPLEMENTED** (no synthetic peers/CPU graphs) |
| Linux BUILD VERIFIED | **NOT TESTED** until `npx tauri build`/`dev` succeeds here |
| Linux RUNTIME VERIFIED | **NOT TESTED** until the window is exercised |
| Windows / macOS | **NOT TESTED** |
| PHYSICAL WAN | **NOT TESTED** |
| Android / iOS | **NOT PHYSICALLY TESTED** |

## Panels

| Panel | Source of truth |
|-------|-----------------|
| Chat | `community-app::chat` → `run_inference_with_reassign` |
| Peers | `peers_view` (state, endpoints, DIRECT/RELAY, RTT, models) |
| Models | `models_view` (ABSENT…READY…FAILED — no timers) |
| Tasks | `tasks_view` (attempts / reassignment visible) |
| Network | `network_view` + `PHYSICAL WAN VERIFIED — NOT TESTED` |
| Settings | Dial bootstrap address into the mesh |

## Build (when Tauri CLI + webkit are installed)

```bash
cd apps/desktop
npm install
npx tauri dev
```

`src-tauri` is **not** a workspace member so `cargo test --workspace` does not require GTK.

## WAN (Track A)

When two ISPs are available, use `scripts/wan-inference-harness.sh` / daemon `--mode originator|worker`. Do not invent a new stack for the test.
