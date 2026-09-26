# Mission control documents

Phase artifacts live under `docs/`:

| Document | Path |
|----------|------|
| Project status | [docs/PROJECT_STATUS.md](docs/PROJECT_STATUS.md) |
| Implementation matrix | [docs/IMPLEMENTATION_MATRIX.md](docs/IMPLEMENTATION_MATRIX.md) |
| Task board | [docs/TASK_BOARD.md](docs/TASK_BOARD.md) |
| Architecture | [docs/architecture/ARCHITECTURE.md](docs/architecture/ARCHITECTURE.md) |
| Storage | [docs/architecture/STORAGE.md](docs/architecture/STORAGE.md) |
| Decisions (ADRs) | [docs/decisions/DECISIONS.md](docs/decisions/DECISIONS.md) |
| Blockers | [docs/BLOCKERS.md](docs/BLOCKERS.md) |
| WAN validation | [docs/testing/WAN_VALIDATION.md](docs/testing/WAN_VALIDATION.md) |
| Native UI state (T-070) | [docs/testing/NATIVE_UI_STATE.md](docs/testing/NATIVE_UI_STATE.md) |

## Stage 1 gate

```text
PHYSICAL WAN VERIFIED — NOT TESTED
```

**Track A:** B-010 blocked by test environment (two ISPs). Use existing WAN harness — no network redesign.  
**Track B:** T-070 / T-060 / T-043 native product over Rust core.

T-021 (real QUIC mesh) has landed. Native UI work is allowed. UI must never fabricate state.

Network architecture is **frozen** unless a real WAN test exposes a concrete defect. Layer-split remains deferred.

`dist/start-wan-mesh.sh` and `packages/worker-node` are **LEGACY / NON-PRODUCTION**.
