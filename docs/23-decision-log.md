# 23. Architectural Decision Records (ADRs)

### ADR-001: Shared Rust Core
- **Context:** Need maximum hardware efficiency, memory safety, cross-platform compilation (Windows, Linux, macOS, iOS, Android), and native C/FFI AI backend bindings.
- **Decision:** Implement all core logic in a unified Rust workspace.

### ADR-002: Transport Layer (QUIC / UDP)
- **Context:** TCP causes head-of-line blocking and connection latency across WAN peers.
- **Decision:** Adopt QUIC with TLS 1.3 encryption for multiplexed tensor activation streaming.

### ADR-003: Model Partitioning vs. Full Replication
- **Context:** Requiring every consumer node to download 10GB+ models limits adoption and prevents large model execution.
- **Decision:** Models are split into discrete layer shards; nodes download and execute only assigned shards.

### ADR-004: Single Flagship Model Default
- **Context:** Eliminating configuration friction for community volunteer testers.
- **Decision:** Default network model set to `Qwen3-14B` (Apache-2.0).

### ADR-005: Resource Priority & UEPS
- **Context:** Volunteer devices must not experience slowdowns or battery drain while used by their owners.
- **Decision:** Implement sub-second hardware governor with automatic throttle and battery pause.

### ADR-006: Postponement of Blockchain
- **Context:** Blockchain adds premature consensus and gas overhead for 10–20 testnet MVP nodes.
- **Decision:** Postpone blockchain settlement; focus on proving distributed AI execution first.
