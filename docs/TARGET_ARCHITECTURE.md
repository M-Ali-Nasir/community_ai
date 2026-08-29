# Target Architecture: Community-Powered Distributed AI Network

**Document Version:** 2.0.0  
**Date:** August 2026  
**Status:** Approved Architectural Target  

---

## 1. System Philosophy & North Star

The system transforms an arbitrary set of heterogeneous consumer devices (GPUs, CPUs, Apple Silicon, mobile phones) into a **single coherent distributed AI inference and learning machine**.

Key Pillars:
1. **Shared Rust Core:** All networking, scheduling, model sharding, resource governance, security, and runtime abstractions live in a single unified Rust workspace compiled across desktop and mobile targets.
2. **Decentralized Layer Sharding:** Models are split into standalone, cryptographically signed layer shards. Workers store and execute only the shards assigned to them by dynamic replication algorithms.
3. **Zero-Trust Security:** Mutual TLS, cryptographic node identities (Ed25519), and signed computation proofs prevent malicious actors from compromising or poisoning the cluster.
4. **Deterministic & Adaptive Scheduling:** Scheduling decisions prioritize minimizing pipeline network hops and maximizing user experience preservation (UEPS).

---

## 2. High-Level Architectural Diagram

```
                              ┌──────────────────────────────────────────────────┐
                              │            P2P NETWORK OVERLAY (QUIC)            │
                              │       libp2p / Kademlia DHT / Hole Punching      │
                              └──────────────────────┬───────────────────────────┘
                                                     │
                 ┌───────────────────────────────────┼───────────────────────────────────┐
                 │                                   │                                   │
                 ▼                                   ▼                                   ▼
   ┌───────────────────────────┐       ┌───────────────────────────┐       ┌───────────────────────────┐
   │    WORKER NODE A (GPU)    │       │    WORKER NODE B (CPU)    │       │     CLIENT / CONSUMER     │
   │  ┌─────────────────────┐  │       │  ┌─────────────────────┐  │       │  ┌─────────────────────┐  │
   │  │   Shared Rust Core  │  │◄─────►│  │   Shared Rust Core  │  │◄─────►│  │   Shared Rust Core  │  │
   │  ├─────────────────────┤  │ QUIC  │  ├─────────────────────┤  │ QUIC  │  ├─────────────────────┤  │
   │  │ Model Shards: 0..14 │  │ Stream│  │ Model Shards: 15..28│  │ Stream│  │ Local Client Query   │  │
   │  ├─────────────────────┤  │       │  ├─────────────────────┤  │       │  ├─────────────────────┤  │
   │  │ Resource Governor   │  │       │  │ Resource Governor   │  │       │  │ Token Stream Display │  │
   │  └─────────────────────┘  │       │  └─────────────────────┘  │       │  └─────────────────────┘  │
   │             │             │       │             │             │       │             │             │
   │             ▼             │       │             ▼             │       │             ▼             │
   │  [Native OS Adapter: Win] │       │  [Native OS Adapter: Lin] │       │  [Desktop UI / Mobile App]│
   └───────────────────────────┘       └───────────────────────────┘       └───────────────────────────┘
```

---

## 3. Modular Crate Decomposition (Rust Workspace)

The core architecture is organized into clean, isolated crates with strict dependency boundaries:

```
crates/
├── community-core          # Common data primitives, IDs, configuration, errors
├── community-protocol      # Wire message definitions, Serde schemas, versioning
├── community-security      # Ed25519 node identities, token signatures, TLS/noise encryption
├── community-network       # libp2p / QUIC transport, peer discovery, NAT hole-punching, RTT matrix
├── community-governor      # Resource monitoring, UEPS scoring, dynamic throttling, OS idle hooks
├── community-model-manager # Model manifests, layer sharder, integrity verification, cache eviction
├── community-scheduler     # Workload analyzer, single-node vs. pipeline planner, replication scoring
├── community-runtime       # Abstract AIBackend trait (llama.cpp C/FFI, Vulkan, Metal, CPU)
├── community-daemon        # Native background worker service orchestrating all crates
└── community-simulator     # Large-scale discrete event cluster simulator (10 to 100k nodes)
```

---

## 4. Subsystem Deep-Dive

### 4.1 Networking & Peer Discovery Layer (`community-network`)
- **Transport:** QUIC via `libp2p-quic` or `quinn` over UDP, providing 0-RTT handshakes, connection multiplexing, and built-in TLS 1.3 encryption.
- **Discovery:**
  - *Phase 1 (MVP/Testnet):* Lightweight rendezvous bootstrap server with direct peer signaling.
  - *Phase 2 (Decentralized):* Kademlia DHT (`libp2p-kad`) with STUN/TURN/AutoNAT and hole punching (`libp2p-autonat`, `libp2p-identify`).
- **Mesh RTT Matrix:** Every active peer maintains rolling pairwise ping/bandwidth estimates with known neighbors to inform pipeline formation.

### 4.2 Resource Governor & UEPS Engine (`community-governor`)
- **User Experience Preservation Score (UEPS):**
  $$\text{UEPS} = 1.0 - \left( w_1 \cdot \Delta\text{Latency}_{\text{system}} + w_2 \cdot \text{Contention}_{\text{CPU}} + w_3 \cdot \text{Contention}_{\text{VRAM}} + w_4 \cdot \text{ThermalPenalty} \right)$$
- **OS-Level Hooks:**
  - **Windows:** `GetLastInputInfo` for sub-second user idle detection; DXGI/NVML for GPU memory and thermal throttling.
  - **Linux:** `/proc/stat`, `/sys/class/power_supply`, and X11/Wayland idle protocol monitors.
  - **macOS:** IOKit HID idle time, Metal device memory allocators.
- **State Machine:** `IDLE` (70–90% capacity) $\rightarrow$ `LIGHT` (30–50%) $\rightarrow$ `ACTIVE` (10–20%) $\rightarrow$ `BUSY/GAMING` (0% - Immediate release of VRAM).

### 4.3 Model Sharding & Distributed Cache (`community-model-manager`)
- **Model Shard Specification:**
  - Models are partitioned at transformer block boundaries:
    - `Shard 0`: Token Embeddings & Pre-attention norm.
    - `Shard 1..N`: Contiguous transformer layer blocks (e.g. 4–8 layers per shard).
    - `Shard N+1`: Final RMSNorm & LM Head.
- **Replication Score on Node Join:**
  $$\text{PlacementScore}(S_k, N_j) = \frac{\text{Demand}(S_k)}{\text{Replicas}(S_k) + 1} \times \frac{\text{AvailableMemory}(N_j)}{\text{Size}(S_k)} \times \frac{1}{\overline{\text{RTT}}(N_j, \text{Neighbors})}$$
- **Integrity:** SHA-256 / BLAKE3 cryptographic hashes verified before loading into runtime.

### 4.4 AI Runtime Abstraction (`community-runtime`)
- **Trait Definition:**
  ```rust
  #[async_trait]
  pub trait AIBackend: Send + Sync {
      async fn load_shard(&mut self, shard_path: &Path) -> Result<ShardHandle, RuntimeError>;
      async fn forward_stage(&self, handle: &ShardHandle, input: TensorActivation) -> Result<TensorActivation, RuntimeError>;
      async fn sample_token(&self, logits: TensorActivation, params: &SamplingParams) -> Result<u32, RuntimeError>;
      fn available_vram(&self) -> usize;
  }
  ```
- **Implementations:**
  - `LlamaCppBackend`: Direct C/C++ bindings via `llama-cpp-sys` avoiding external subprocess overhead.
  - `WgpuBackend`: WebGPU compute shaders for cross-platform portability.

### 4.5 Dynamic Cluster Scheduler (`community-scheduler`)
- **Scheduling Policies:**
  1. `SingleNodeOptimal`: Evaluated first. If a single node has memory and compute capacity, co-locate execution to eliminate network latency.
  2. `AdaptivePipeline`: Assembles a minimal-hop pipeline chain sorted by physical network proximity ($N_0 \rightarrow N_1 \rightarrow \dots \rightarrow N_k$).
  3. `TaskParallelMapReduce`: Independent batch requests fanned out across all ready nodes.
- **Fault Recovery:** If a pipeline member drops mid-generation, the head node queries the local neighbor cache for hot-standby replicas holding the same shard index and hot-swaps the QUIC stream.
