# worker-node — LEGACY / NON-PRODUCTION

**Decision (2026-09-26):** `@community-ai/worker-node` is **temporary migration / test infrastructure**, not the long-term inference architecture.

| Question | Answer |
|----------|--------|
| Target production path | Native app → `community-app` → Rust `MeshSwarm` → `llama.cpp` (`community-runtime`) |
| Still required? | Only as a reference for the existing `~/.community-ai/llama/b10632-*` binary layout (T-044) and historical TS tests |
| New production features | **Forbidden** on this package |
| Coordinator WebSocket | LEGACY — not the mesh |

The Rust daemon (`community-daemon --mode worker`) is the production worker.
