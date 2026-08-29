# Architecture

The thesis being tested: **can a changing collection of ordinary personal
computers behave as one useful AI machine?**

Everything below exists to make that question answerable with measurements
rather than opinion.

## The one boundary that matters

> The coordinator decides **what** a node should do.
> The node decides **how much** of its owner's machine it will give.

Neither side may cross it. The coordinator cannot raise a node's capacity, and a
node cannot invent work for itself. Every other design choice follows from
keeping that line clean.

## System layout

```
                    +-------------------------------+
                    |          COORDINATOR          |
                    |                               |
   client WS -----> |  API + authentication         |
                    |  Device Registry              |
                    |  Model Registry (catalog)     |
                    |  Workload Analyzer            |
                    |  Cluster Scheduler            |
                    |  Pipeline Planner             |
                    |  Job Manager                  |
                    +---------------+---------------+
                                    | worker WS
                +-------------------+-------------------+
                |                                       |
    +-----------v-----------+             +-------------v-----------+
    |   DESKTOP WORKER      |             |  BROWSER CONTRIBUTOR    |
    |                       |             |  (optional, opt-in)     |
    |  Hardware Agent       |             |  Frame monitor          |
    |  Resource Governor    |             |  Resource Governor      |
    |  Network Agent        |             |  Web Worker             |
    |  RPC Server Agent     |             |  WebLLM / WebGPU        |
    |  Worker Scheduler     |             +-------------------------+
    |  AI Runtime           |
    |    -> llama.cpp RPC   |
    |  Model Cache (GGUF)   |
    +-----------------------+
```

Both node types speak the *same* worker protocol. The coordinator does not
special-case browsers; it only knows that one tier is less reliable and weights
it accordingly.

## Capability profiles, not hardware names

A node advertises what it can contribute right now:

```jsonc
{
  "nodeId": "worker-gaming-pc",
  "kind": "desktop-worker",
  "cpu":     { "model": "Ryzen 7 5800X", "cores": 16, "available": 0.72 },
  "gpu":     { "vendor": "NVIDIA", "model": "RTX 3060", "vram": 12288,
               "available": 0.81, "backend": "cuda" },
  "memory":  { "total": 32768, "available": 21400 },
  "network": { "latency": 18, "bandwidthMbps": 640, "jitter": 2.4 },
  "userState": { "activity": "idle", "thermalState": "normal",
                 "onBattery": false, "batteryPct": null },
  "runtime": { "engine": "llama.cpp-rpc", "ready": true,
               "loadedModels": ["qwen2.5-1.5b"],
               "rpc": { "endpoint": "100.x.y.z:50052", "offeredMemoryMB": 8192,
                        "canHead": true, "build": "b10632" } }
}
```

Nothing in the scheduler matches on `"RTX 3060"`. Two derived numbers do all the
work, both in `packages/protocol/src/capability.ts`:

- `usableMemoryMB` — governor-approved VRAM, plus system RAM discounted to 12%
  when a GPU is present and 35% when it is not. Host-side offload is slow enough
  that treating a 32 GB CPU box as a peer to a 12 GB GPU produces bad placements.
- `throughputPrior` — a vendor-neutral estimate from VRAM size, backend and free
  compute, used only until the node has produced real tokens. After that,
  measured tokens/second replaces it via an exponentially weighted average.

The registry marks which of the two a node's current figure is
(`throughputIsMeasured`), because a plan built on estimates deserves less trust
than one built on measurements.

`latency` and `jitter` in the registry are overwritten with values the
coordinator measures itself from ping/pong round-trips. A node's self-report is
a hint; the coordinator's own clock is the record.

## Workload Analyzer

```
                    REQUEST
                       |
                Workload Analyzer
                       |
          +------------+------------+
          |                         |
   independent units          tightly coupled
          |                         |
    task-parallel        single-node / model-parallel
```

The strategy is derived from the request, not selected by the user:

- **Chat turn, model fits one node** → `single-node`. Token *N+1* depends on
  token *N*, so splitting the decode would only add a network hop per token.
- **Chat turn, model fits no single node** → `model-parallel`. The Pipeline
  Planner assembles the fewest members whose pooled memory holds the weights,
  then orders them by measured RTT. That is the research case: a model the
  community can run that no member could run alone.
- **Batch of independent items** → `task-parallel`, provided there are at least
  two items and two nodes that can hold the model.

Showing the rejected strategies is deliberate. The interesting research output
is *when each approach is appropriate*, and that only becomes visible if the
system explains its reasoning on every request.

## Cluster Scheduler

Six policies are implemented side by side so they can be compared on the same
live network within minutes of each other:

| Policy | Ranks by | Purpose |
| --- | --- | --- |
| `adaptive` | throughput, discounted by RTT, warm models, node tier | The proposal |
| `best-node` | single strongest node | Baseline to beat |
| `compute-only` | raw throughput | Isolates the value of network awareness |
| `network-aware` | throughput with a heavy RTT penalty | Isolates over-correction |
| `resource-aware` | governor-approved free memory | Isolates memory-driven placement |
| `round-robin` | nothing | Control condition |

`adaptive` adds a marginal-benefit cut-off: a node contributing under 8% of the
cluster's current aggregate score is dropped rather than included. Adding a slow
node to a fan-out does not help if it becomes the straggler everyone waits for.

Work allocation is two-phase:

1. **Initial split** by capability share, using largest-remainder rounding so
   fractional shares become whole tasks.
2. **Work stealing.** Whichever node finishes first pulls from the longest
   remaining queue. A node that turns out slower than its profile suggested
   cannot hold up the job, and the system self-corrects without needing an
   accurate performance model up front.

The reduce step goes to the fastest *native* worker when one exists. Losing the
reduce loses the whole job, so it does not go to a browser tab that might be
backgrounded.

## Resource Governor

Lives inside the worker (`packages/worker-node/src/agents/governor.ts`), never on
the coordinator.

```
owner idle           -> capacity climbs toward the configured ceiling
owner opens an IDE   -> capacity steps down
owner starts a game  -> capacity to zero, running work abandoned
owner stops          -> capacity restored gradually, never in one jump
```

Capacity rises at 0.12 per tick and falls at 0.5. Being slow to *reclaim* spare
cycles is invisible to the owner; being slow to *release* them is not.

Inputs: owner CPU load, free RAM, thermal state, battery state, manual pause.

Owner CPU load is computed as total system busy **minus this worker's own CPU
time**. Without that subtraction the worker's own contribution would look like
owner activity and it would throttle itself into oscillation.

Every limit is enforced locally and every task can be refused. The worker sends
`task:rejected` with a human-readable reason, and the coordinator reassigns.

The browser governor (`packages/web/src/lib/governor.ts`) implements the same
contract with the signals a browser actually has. Inference runs in a Web
Worker, so main-thread frame jank is a clean proxy for *the owner doing
something* — measured via `requestAnimationFrame` deltas.

## AI Runtime boundary

```
Worker Scheduler
       |
   AI Runtime                 <- interface: prepare / generate / configure / dispose
       |
 llama.cpp RPC                <- the only implementation
       |
 ggml-rpc-server (this node)  <- members hold layers
 llama-server    (head)       <- drives the pipeline, holds almost no weights
```

The ggml RPC protocol has no authentication. The server therefore binds to a
private/overlay address, never `0.0.0.0` on a public host. That is why the
testnet lives on Tailscale.

A node advertises two different numbers on purpose:

- `usableMemoryMB` — what it can devote to a *whole* model.
- `runtime.rpc.offeredMemoryMB` — what it can devote to a *slice* of a model.

Those are not the same contribution. A 6 GB laptop can host part of a 40 GB
model even though it could never load one alone.

## Pipeline Planner

Lives in `packages/coordinator/src/pipeline.ts` and is a different optimiser
from the fan-out scheduler:

| | Task-parallel | Model-parallel |
| --- | --- | --- |
| Objective | maximise nodes that help | minimise nodes that still fit |
| Cost of an extra node | coordination | a round-trip on *every token* |
| Ranked by | throughput, discounted by RTT | free memory, then RTT |
| Failure of one node | reassign that task | rebuild the whole pipeline |

The latency floor is structural:

```
time_per_token  ≥  Σ RTT between consecutive members
```

The UI reports both the measured tokens/second and this ceiling, so a slow
result cannot be mistaken for a slow GPU. The hypothesis worth testing is not
"is this faster" — it is not — but "does this make a model runnable that no
member could run at all, at a tolerable speed?"

When a pipeline member pauses or disappears, the job manager does not retry the
task elsewhere. It asks the planner to re-form the chain from whoever is left.
That is dynamic model placement in its simplest form.

The llama.cpp build tag is pinned (`b10632`). The RPC wire format carries a
version, and peers that disagree fail at connect time, so every node in a
pipeline must run the same build.

## Failure handling

| Failure | Response |
| --- | --- |
| Pipeline member disconnects | Whole pipeline re-planned from remaining nodes |
| Node disconnects mid-task | Tasks requeued, preferring a node that has not already failed them |
| Node refuses a task | Reassigned immediately with the refusal reason recorded |
| Task exceeds the timeout | Reclaimed by the sweeper and retried |
| Every node busy | Task queues on a capable node rather than failing |
| Node reconnects | Reclaims its identity and its learned performance history |
| Coordinator restarts | Workers reconnect with exponential backoff |

Retries are capped at three attempts, and a job with partial results still runs
the reduce over whatever succeeded.

## Deliberately not yet

- **Sharded *storage*.** GGUF is already a multi-file format. The missing piece
  was a runtime that executes a layer range, which is what this version adds.
  Serving shards from other workers instead of Hugging Face is next.
- **True pairwise RTT.** Hops are estimated from each node's RTT to the
  coordinator. Measuring neighbour-to-neighbour latency will change the numbers.
- **Peer-to-peer worker communication.** Workers still only talk to the
  coordinator; the pipeline itself is llama.cpp talking RPC between members.
- **Blockchain, tokens, reputation.** None of it helps answer the core question.
- **Phones as required compute.** They can contribute; they cannot host a slice.

The roadmap is: v0.1 task distribution → v0.2 layer-split pipelines (this) →
v0.3 dynamic shard placement and worker-to-worker model cache → v0.4 P2P
discovery → v0.5 reputation.
