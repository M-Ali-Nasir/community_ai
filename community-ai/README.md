# Community AI

A distributed compute network built from ordinary personal computers, whose
first client happens to be a web app.

Friends run a small worker on their machine. A coordinator keeps track of what
each machine can spare right now and forms a temporary cluster for every
request. Everything here runs real models and produces real tokens — there are
no simulated workers.

```
                          USER
                            |
                     PWA (chat + optional contributor)
                            |
                  +---------------------+
                  |     COORDINATOR     |
                  |  API / auth         |
                  |  Device Registry    |
                  |  Workload Analyzer  |
                  |  Cluster Scheduler  |
                  |  Job Manager        |
                  +----------+----------+
                             |
                   private overlay network
                             |
        +--------------------+--------------------+
        |                    |                    |
   Worker (RTX 4090)   Worker (RTX 3060)    Worker (MacBook)
   Resource Governor   Resource Governor    Resource Governor
   RPC server          RPC server           RPC server
   Model Cache         Model Cache          Model Cache
```

## What is actually running

| Piece | What it does |
| --- | --- |
| `packages/protocol` | Capability profile, governor report, WebSocket contracts, model catalogue |
| `packages/coordinator` | Device registry, workload analyzer, cluster scheduler, job manager, HTTP + WS API |
| `packages/worker-node` | Native desktop worker: hardware agent, resource governor, RPC server, pipeline runtime over llama.cpp |
| `packages/web` | PWA: chat client, live network view, optional WebGPU contributor |

## Quick start

```bash
npm install
npm run build

# terminal 1 — coordinator, also serves the built PWA
npm start

# terminal 2 — a worker on this machine
# first run downloads llama.cpp (~32 MB) into ~/.community-ai/llama/
node packages/worker-node/dist/index.js --model qwen2.5-0.5b
```

Open <http://localhost:8787>. The first run downloads the model weights once and
caches them in `./models`.

For development with hot reload, `npm run dev` starts the coordinator and a Vite
dev server that proxies to it.

### End-to-end check from the command line

```bash
node scripts/smoke.mjs chat  "Why is distributed inference hard?"
node scripts/smoke.mjs batch          # fans four items out across the network
```

It prints the analyzer's decision, the cluster it formed, which node ran which
task, and the measured tokens per second.

## Running it with friends

The coordinator listens on `0.0.0.0`, but do not put it straight on the public
internet. Put everyone on a private overlay network first — see
[docs/DEPLOY.md](docs/DEPLOY.md) for the Tailscale walkthrough. Then each friend
runs:

```bash
node packages/worker-node/dist/index.js \
  --coordinator http://<coordinator-tailscale-ip>:8787 \
  --token <join token> \
  --model qwen2.5-1.5b
```

They get a live panel showing what their machine is giving, and `p` pauses
contribution instantly.

```
  Community AI Worker   gaming-pc

  Status        Connected   http://100.x.y.z:8787
  Engine        llama.cpp b10632 (vulkan)
  Device        NVIDIA GeForce RTX 3060 (cuda)
  VRAM          12.0 GB total, 9.7 GB free

  Owner CPU     ███░░░░░░░░░░░░░░░  18%
  GPU load      █░░░░░░░░░░░░░░░░░   4%
  Capacity      ██████████████░░░░  78%   available

  Contribution  READY
  Model         qwen2.5-1.5b ready
  Serving       100.x.y.z:50052   RTX 3060 10 GB
  Pipeline      idle
  Tasks         0 running, 12 done   31.4 tok/s avg

  p = pause/resume    q = quit
```

Phones open the same URL and install the PWA. They are clients by default;
contributing from a browser is opt-in and explicitly experimental.

## Design decisions worth knowing

**The coordinator never looks at GPU model names.** Every node publishes a
capability profile — free compute, spare memory, measured round-trip time, owner
activity, thermal state — and the scheduler ranks on that. A 4090, an M2 Air and
a CPU-only laptop are the same kind of object to it.

**The Resource Governor lives inside the worker.** The coordinator decides *what*
a node should do. The node decides *how much* of the owner's machine it will
give, and can refuse any task. Capacity ramps up slowly and drops fast, so
starting a game reclaims the machine immediately.

**Distribution strategy is derived, not configured.** The Workload Analyzer looks
at the request and picks: a chat turn that fits one node stays there; a chat
turn that fits none is split across the fewest members whose pooled memory holds
it; a batch of independent items fans out. The reasoning, including strategies
it rejected and why, is shown in the UI.

**Workers don't own the model. They cache parts of it.** Each node runs
`ggml-rpc-server` and offers a slice of memory. The coordinator's pipeline
planner places layer ranges so a model larger than any one machine can still
run. Every extra hop costs a round-trip on every token, so the planner
minimises members, the opposite of the fan-out scheduler.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the full picture.

## Models and licences

The default catalogue is Apache-2.0 and MIT only, and every entry is ungated on
Hugging Face. This matters because weights get copied onto other people's
machines: anything requiring individual licence acceptance would make each
volunteer a licensee. [docs/LICENSES.md](docs/LICENSES.md) records what was
included, what was excluded, and why.

## Status

v0.2. Task distribution and layer-split pipelines both work end to end on real
hardware. A model that no single node can hold is assembled into a pipeline and
runs; the dashboard shows pooled memory against the largest member, and
measured tokens/second against the network's latency ceiling.
