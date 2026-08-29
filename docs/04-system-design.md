# 04. System Design & Crate Architecture

The core architecture is organized as a Cargo workspace with clean boundary separation:

- `community-core`: IDs (`NodeId`, `JobId`, `TaskId`, `ShardId`), errors, and common traits.
- `community-protocol`: Wire data structures for node capabilities, workload requests, and pipeline definitions.
- `community-security`: Cryptographic node identity (Ed25519) and hash verification (BLAKE3).
- `community-governor`: Hardware monitor, sub-second idle detection, and UEPS calculation.
- `community-model-manager`: GGUF layer sharder, LRU disk cache, and dynamic placement scoring.
- `community-runtime`: Hardware-agnostic `AIBackend` abstraction for headless layer execution.
- `community-scheduler`: Workload analyzer, pipeline cluster planner, and fault handler.
- `community-network`: Peer registry, heartbeat tracking, and wire framing.
- `community-daemon`: Cross-platform background worker service.
- `community-simulator`: Discrete event cluster simulator for 10–100k nodes.
