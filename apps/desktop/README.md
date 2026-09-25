# Community AI native desktop (Tauri 2)

This shell is the **start** of T-060. It must call the Rust core (`community-app` → `MeshSwarm`). It must **not** use Chrome app mode, WebRTC, or the TypeScript coordinator for networking.

## Status

| Item | Status |
|------|--------|
| Application API (`crates/community-app`) | **PROCESS VERIFIED** (`app_starts_without_browser`) |
| Tauri window + IPC | **SCAFFOLDED — NOT BUILT IN CI** (requires webkit2gtk / Tauri CLI) |
| Fancy UI | Intentionally deferred |

## Build (when Tauri CLI + webkit are installed)

```bash
cd apps/desktop
npm install
npx tauri dev
```

The `src-tauri` crate is **not** a workspace member so `cargo test --workspace` does not require GTK.

## Mobile

The same `community-app` / `community-ffi` core is the intended Android/iOS path.

**Android / iOS physical WAN: NOT TESTED.**
