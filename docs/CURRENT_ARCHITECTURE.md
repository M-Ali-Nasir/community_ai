# Current Architecture Audit & Technical Assessment

**Document Version:** 1.0.0  
**Date:** August 2026  
**Auditor:** Principal Distributed Systems & AI Infrastructure Architect  
**Repository State:** Dual-stack (Legacy Python simulation + Node.js/TypeScript monorepo v0.2)

---

## 1. Executive Summary & Repository Overview

The repository currently contains two distinct generations of prototypes developed to explore decentralized, community-powered AI compute:

1. **Generation 1 (Legacy Python Prototype in `/community_ai`):**
   - In-memory event simulator and HTTP coordinator/worker demonstration.
   - Computes analytical TFLOPS / PCIe / link latency equations (`community_ai/cluster.py`) without running real local neural tensor operations.
   - Useful mathematical baseline for theoretical cluster modeling and simulation.

2. **Generation 2 (TypeScript Monorepo in `/community-ai`):**
   - Monorepo using npm workspaces (`@community-ai/protocol`, `@community-ai/coordinator`, `@community-ai/worker-node`, `@community-ai/web`).
   - Implements **Level 1 (Task-parallel map/reduce)** and **Level 2 (Model-parallel layer-splitting)** using pre-built `llama.cpp` binaries (`ggml-rpc-server` and `llama-server --rpc`).
   - Web PWA client with WebGPU capability via WebLLM for optional browser-based contribution.
   - Centralized WebSocket coordinator managing node registration, capability advertisements, workload analysis, and pipeline planning.

---

## 2. Deep Component Audit & Execution Flows

### 2.1 Protocol Layer (`packages/protocol`)
- **Schema Engine:** Zod (`z.object`) for runtime schema enforcement and TypeScript inference.
- **Key Schemas:**
  - `CapabilityProfile`: Node kind (`desktop-worker`, `browser-contributor`, `client`), CPU/GPU hardware descriptors, available memory, RTT/bandwidth network profile, user state, and `RpcProfile`.
  - `RpcProfile`: Endpoint (`host:port`), `offeredMemoryMB`, device list, `canHead` boolean, and binary `build` tag.
  - `WorkloadAnalysis` & `TaskSpec`: Distinguishes between `single-node`, `task-parallel`, and `model-parallel` execution.
  - `PipelinePlan`: Defines a multi-stage execution pipeline specifying member nodes, layer shares, pooled memory, and latency ceilings.
- **Model Catalog (`models.ts`):**
  - Explicit Apache-2.0 and MIT licensing whitelist (`smollm2-360m`, `qwen2.5-0.5b`, `qwen2.5-1.5b`, `phi-3.5-mini`, `mistral-7b`, `qwen2.5-7b`).
  - Gated models (Llama 3.1, Gemma 2) explicitly excluded to protect peer redistribution.

### 2.2 Coordinator (`packages/coordinator`)
- **Server Core:** Express HTTP server + `ws` WebSocket server with optional self-signed TLS (`tls.ts`) for enabling remote WebGPU contexts.
- **Registry (`registry.ts`):** In-memory thread-safe node map tracking heartbeat, last-seen timestamps, governor limits, and historical throughput.
- **Scheduler & Analyzer (`analyzer.ts`, `scheduler.ts`, `pipeline.ts`):**
  - `analyzeWorkload()`: Evaluates model memory footprint vs. available nodes. If model fits a single machine, single-node execution is enforced to avoid latency overhead. If model exceeds all single nodes but fits pooled memory, `model-parallel` is triggered.
  - `planPipeline()`: Greedily selects the minimal number of RPC workers whose pooled memory satisfies the model weight + KV cache requirement ($1.15\times$ factor), sorted by lowest round-trip time.

### 2.3 Worker Node Daemon (`packages/worker-node`)
- **Process Orchestration:** Node.js CLI spawning child processes:
  - `ggml-rpc-server`: Binds to local IP (LAN/Tailscale), exposing GPU/CPU compute over ggml-rpc protocol.
  - `llama-server`: Spawned on the designated "Head Node" with `--rpc <endpoints>` and `--tensor-split <fractions>`, exposing an OpenAI-compatible streaming API.
- **Binary & Model Fetching:**
  - `llamaBinaries.ts`: Fetches pinned `llama.cpp` release binaries (`b10632`) from GitHub Releases with automatic OS/architecture detection.
  - `modelFiles.ts`: Resumable Hugging Face GGUF downloader with `.part` staging files.
- **Local Agents:**
  - `HardwareAgent`: Probes OS, CPU cores, system RAM, NVIDIA/AMD/Apple GPU status.
  - `ResourceGovernor`: Dynamically calculates capacity factors based on owner activity, battery state, thermal condition, and manual pause commands.
  - `NetworkAgent`: Probes RTT, download bandwidth, and jitter to the coordinator.

### 2.4 Web Client & PWA (`packages/web`)
- **Frontend Framework:** React 18 with Vite, modular CSS.
- **WebLLM Web Worker (`webllm.worker.ts`):** In-browser inference engine utilizing `@mlc-ai/web-llm` for WebGPU-accelerated local execution.
- **Diagnostics:** Auto-detects WebGPU support and provides clear remediation paths for non-secure HTTP origins.

---

## 3. Critical Architectural & Technical Flaws

| Category | Finding / Vulnerability | Impact | Long-Term Architectural Conflict |
| :--- | :--- | :--- | :--- |
| **Centralization** | Single coordinator WebSocket hub for signaling, job dispatch, and node discovery. | Single point of failure (SPOF) and bottleneck under large node counts. | Violates the decentralized P2P computing vision. |
| **Model Distribution** | Full GGUF file must reside on the Head Node disk; head streams tensors to RPC workers. | Workers without disk space cannot act as head; head node bears severe download & disk penalty. | Contradicts true model sharding where nodes only hold discrete layer files. |
| **P2P Latency Matrix** | Latency is only measured between Worker $\leftrightarrow$ Coordinator, not Worker $A \leftrightarrow$ Worker $B$. | Inaccurate pipeline hop latency estimation over WAN. | Sub-optimal pipeline clustering across disparate networks. |
| **Security & Trust** | Plain unauthenticated RPC protocol (`ggml-rpc`); arbitrary coordinator commands accepted without cryptographic verification. | Malicious node can poison tokens, execute arbitrary RPC calls, or inject spoofed metrics. | Violates zero-trust principle for community devices. |
| **Runtime Coupling** | Runtime tightly coupled to Node.js wrapper around external `llama.cpp` CLI binaries. | Heavy Node.js runtime footprint; fragile sub-process management; no native memory control. | Hampers cross-platform embedding (iOS, Android, background daemons). |
| **Resource Isolation** | Background execution relies on polling system metrics via Node.js standard libraries. | High jitter; lacks native OS low-level idle hooks (e.g. Windows `GetLastInputInfo`, macOS IOKit). | Degrades user experience preservation score (UEPS). |

---

## 4. Reusable Assets vs. Code to Replace

### 4.1 Reusable Assets (To be adapted/ported)
- **Mathematical Formulations:** Pipeline latency estimations, TFLOP scaling, and memory overhead equations from `community_ai/cluster.py` and `packages/coordinator/src/pipeline.ts`.
- **Licensing and Model Manifest Schemas:** Apache-2.0 gating verification rules and model specs in `packages/protocol/src/models.ts`.
- **UI & Dashboard Components:** React components in `packages/web` (chat streams, node telemetry grids, pipeline visualizer cards).
- **Test Invariants:** End-to-end smoke verification logic in `scripts/smoke.mjs`.

### 4.2 Components to Replace / Redesign
- **Node.js Worker Runtime:** Replace with a unified, native **Rust Core Engine** compiled to native binaries and C/FFI libraries.
- **Centralized WebSocket Coordinator:** Evolve toward a hybrid bootstrap/DHT discovery model with libp2p and direct QUIC streams.
- **Full-GGUF Head Node Streaming:** Replace with discrete **Model Shard Packages** where nodes only store, verify, and execute their assigned layer shards.
