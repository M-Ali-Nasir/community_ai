# IMPLEMENTATION MATRIX

**Audit date:** 2026-09-26  
**Branch / workspace:** `community-ai-stage1-tauri-wan`  
**Auditor role:** Manager Agent (Stage 1B)  
**Method:** Source inspection + `community-app` process tests. Tauri window BUILD/RUNTIME **NOT TESTED** this gate.

Classification legend:

| Tag | Meaning |
|-----|---------|
| REAL | Production-capable behavior verified in source |
| PARTIAL | Real pieces exist; critical paths incomplete |
| SIMULATED | Timers, templates, random, or in-memory stand-ins presented as real |
| STUB | Declared API / UI / config with no meaningful body |
| UNUSED | Code or dependency present but not wired into runtime |
| BROKEN | Present but incorrect / inconsistent / non-runnable |
| MISSING | Required for target product; not in tree |
| UNKNOWN | Needs runtime measurement to confirm |

---

## A. TypeScript monorepo (`community-ai/`)

| Component | Class | Evidence |
|-----------|-------|----------|
| `@community-ai/protocol` Zod schemas | REAL | `packages/protocol/src/*` |
| Model catalog (`qwen3-14b` only) | PARTIAL | Catalog exists; `largestModelFitting` ignores RAM; disk models / smoke still reference other IDs |
| `@community-ai/coordinator` Express+WS hub | LEGACY | Hub topology; **not** required for the mesh (ADR-0011) |
| Analyzer / scheduler / pipeline | REAL | `analyzer.ts`, `scheduler.ts`, `pipeline.ts`, `jobs.ts` |
| `@community-ai/worker-node` + llama.cpp / GGUF | LEGACY | Node CLI + coordinator WS. Target worker is Rust daemon. See `packages/worker-node/LEGACY.md`. No new production features. |
| Optional `node-llama-cpp` | PARTIAL | Dynamic import; worker may be not-ready if missing |
| Web PWA shell (React/Vite) | REAL | `packages/web` builds and serves UI |
| Chat answers (`inferenceEngine.ts`) | DISABLED | `generateModelResponse` throws; not production |
| Chat submit path (`useCoordinator.submit`) | DISABLED | Does not stream templates; tells user to use native mesh |
| Fabricated local peer (`buildInitialLocalNode`) | SIMULATED | Hardcoded tok/s, `ready: true`, fake RPC endpoint |
| WS disconnect UX | SIMULATED | Forces `connected: true`, `p2p-mesh-active` |
| Contributor → `/ws/worker` | REAL | `contributor.ts` registers and heartbeats |
| WebLLM worker | REAL | `contribute/webllm.worker.ts` uses `@mlc-ai/web-llm` |
| Model-ready gate for chat | PARTIAL | PWA timer READY removed; WebLLM `loaded` still marks ready |
| Browser governor (rAF/battery) | REAL | `lib/governor.ts` |
| DeviceResourcesPanel CPU/RAM | SIMULATED | Explicit random jitter comment |
| NetworkPanel token balance | SIMULATED | Formula / floor 1000; no ledger |
| Ed25519 / ZK marketing copy | STUB | Strings in `NetworkPanel.tsx` only |
| `trystero` dependency | UNUSED | In `package.json`; zero imports in `src/` |
| BroadcastChannel mesh | MISSING | No usage in app source |
| True WebRTC P2P mesh | REJECTED | Production mesh is native QUIC, not browser WebRTC |
| Unit tests (TS packages) | MISSING | No `*.test.ts` / vitest / jest in packages |
| `scripts/smoke.mjs` | REAL | Live coordinator e2e; default model id BROKEN vs catalog |

---

## B. Rust workspace (`crates/`)

| Component | Class | Evidence |
|-----------|-------|----------|
| `community-core` | REAL | IDs, errors |
| `community-protocol` | REAL | Serde `MeshFrame` + handshake/gossip/echo; tests |
| `community-security` | REAL | Ed25519 persist + BLAKE3 + tests |
| `community-governor` | REAL / thermal PARTIAL | `sysinfo` CPU/RAM; thermal always `Normal` |
| `community-scheduler` | REAL | Local pipeline planner + tests (not a network master) |
| `community-model-manager` | PARTIAL | Scoring/manifest; `insert_shard` does not write bytes |
| `community-runtime` llama.cpp | REAL | `LlamaServerEngine` + GGUF smoke token; same b10632 `llama-server` as worker-node |
| `community-runtime` `SimulatedAIBackend` | SIMULATED | `sim` feature only; not linked by daemon |
| `community-network` `MeshSwarm` | REAL | QUIC + handshake + WAN endpoints + gossip + tasks + originator reassign |
| `community-network` STUN | PARTIAL | RFC 5389 client; live public STUN **NOT TESTED** |
| `community-network` relay | PARTIAL | Opaque UDP forward **PROCESS VERIFIED**; WAN hole-punch **NOT TESTED** |
| `community-app` | REAL (API) | Native mesh session; peers/network/models/tasks/chat views; CPU/memory graphs **removed** from UI |
| `community-network` `InMemorySwarm` | SIMULATED | Test utility only |
| `community-daemon` | REAL (mesh + optional llama) | `--model` loads GGUF; no coordinator |
| `community-simulator` | SIMULATED | Intentional cluster CLI |
| `community-ffi` C ABI | PARTIAL | Identity/governor/planner; async mesh bind not in FFI |
| Daemon `--coordinator` flag | REMOVED | systemd/launchd updated to peer-only CLI |

---

## C. Platforms (`platform/`)

| Component | Class | Evidence |
|-----------|-------|----------|
| Android `MainActivity` | PARTIAL | WebView → bundled PWA only |
| Android `CommunityAINative` | UNUSED + BROKEN | Never called; expects JNI; C ABI only; no `.so` in tree |
| Android `build_android.sh` | STUB | Would emit jniLibs; directory absent |
| iOS Swift bridge | STUB | Header + Swift; no Xcode app / XCFramework |
| Linux systemd unit | PARTIAL | Peer daemon; no coordinator flag |
| macOS launchd plist | PARTIAL | Same |
| Windows service | STUB + BROKEN | JSON metadata only |

---

## D. Packaging (`dist/`)

| Component | Class | Evidence |
|-----------|-------|----------|
| `CommunityAI.apk` | REAL (WebView shell) | Installable; not native inference |
| `launch-app.sh` | REAL + browser-dependent | `npm run dev` + Chrome `--app=` |
| `install-desktop.sh` | REAL | Desktop entry + icons |
| `start-wan-mesh.sh` | LEGACY / DEPRECATED | cloudflared + TypeScript coordinator; **not** production WAN. Use `scripts/wan-inference-harness.sh` |
| Native desktop binary (Tauri/Electron-free) | PARTIAL | UI+IPC+Rust IMPLEMENTED; Linux/Windows/macOS BUILD/RUNTIME/PHYSICAL all **NOT TESTED** |
| Non-browser worker today | LEGACY (TS) | `worker-node` — not the production worker; Rust `community-daemon` is |

---

## E. Documentation honesty

| Doc | Class vs reality |
|-----|------------------|
| Root `README.md` P2P WebRTC badge | BROKEN | No WebRTC mesh in app |
| `community-ai/README.md` “no simulated workers” | BROKEN | Chat + ready timers simulated |
| `docs/TARGET_ARCHITECTURE.md` | ASPIRATIONAL | Correct north star; not implemented |
| `docs/CURRENT_ARCHITECTURE.md` | PARTIAL | Accurate on TS hub; outdated Python mention |
| `docs/MIGRATION_PLAN.md` | PARTIAL | Phases 1 mostly done; 2–6 incomplete |

---

## F. Dead / duplicate / abandoned

| Item | Action candidate |
|------|------------------|
| `trystero` unused | REMOVE or REWIRE after ADR |
| `inferenceEngine.ts` production chat path | DONE — throws; PWA submit disabled |
| `buildInitialLocalNode` synthetic peer | REMOVE from production path |
| Fake ready `setTimeout` in `contributor.ts` | DONE — timers removed |
| DeviceResourcesPanel random CPU | REPLACE with real sensors |
| Duplicate `CommunityAINative.kt` paths | DEDUPE |
| In-memory Rust swarm presented as QUIC | DONE — production type is `MeshSwarm`; `InMemorySwarm` is tests-only |
| Chrome/WebView as “the app” | MIGRATE to native shell (Tauri + mobile natives) |

---

## G. Security weaknesses (observed)

1. Coordinator join token often empty; no mutual auth on worker RPC.
2. `ggml-rpc` path is unauthenticated (known in CURRENT_ARCHITECTURE).
3. UI claims end-to-end P2P encryption without implementing it.
4. Replay window + dial/task/peer limits exist on the mesh; hostile-Internet review still open (T-080).
5. Model integrity: TS worker may verify downloads; Rust cache does not persist/verify weights.

---

## H. What is genuinely reusable

KEEP / MIGRATE:

- TS protocol schemas → map into Rust `community-protocol` (already parallel).
- Coordinator analyzer/scheduler ideas → port into Rust scheduler (already started).
- Worker-node llama.cpp + GGUF download logic → become reference for Rust `LlamaCppBackend`.
- `community-security` Ed25519/BLAKE3.
- Branding / icons / install scripts (adapt to native packages).

REPLACE:

- Web chat simulation stack.
- In-memory P2PSwarm.
- SimulatedAIBackend as production runtime.
- WebView/Chrome as final product runtime.
