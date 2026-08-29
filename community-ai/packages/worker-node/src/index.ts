#!/usr/bin/env node
import { hostname } from "node:os";
import {
  type CapabilityProfile,
  type NodeMetrics,
  emptyMetrics,
  getModel,
} from "@community-ai/protocol";
import { HardwareAgent, platformInfo, type HardwareSnapshot } from "./agents/hardware.js";
import { NetworkAgent } from "./agents/network.js";
import { ResourceGovernor, classifyActivity } from "./agents/governor.js";
import { RpcServerAgent } from "./agents/rpc.js";
import { LLAMA_BUILD, ensureLlamaBinaries, type BinaryPaths } from "./runtime/llamaBinaries.js";
import { PipelineRuntime, type PipelineConfig } from "./runtime/llamaServer.js";
import { WorkerScheduler } from "./scheduler.js";
import { WorkerClient } from "./client.js";
import { StatusPanel, parseArgs, toWebSocketUrl, type StatusModel } from "./cli.js";

const parsed = parseArgs(process.argv.slice(2));
if (!parsed) process.exit(0);
const args = parsed as NonNullable<typeof parsed>;

// The LAN coordinator serves a self-signed cert so browser WebGPU works from
// other PCs. Node's fetch (bandwidth probe) must accept that same cert.
if (args.coordinator.startsWith("https://")) {
  process.env.NODE_TLS_REJECT_UNAUTHORIZED = "0";
}

// Derived from the label (which defaults to the hostname) so the same machine
// reconnecting reclaims its identity and its learned performance history, while
// two workers started with different --name values stay distinct.
const nodeId = `worker-${args.label.toLowerCase().replace(/[^a-z0-9-]+/g, "-").replace(/^-|-$/g, "")}`;
void hostname;

const hardware = new HardwareAgent();
const network = new NetworkAgent(args.coordinator);
const governor = new ResourceGovernor(args.governor);

// The runtime exists from the start so the worker can register and appear in
// the dashboard while the llama.cpp release is still downloading. It refuses
// work until `attach` supplies the binaries.
const runtime = new PipelineRuntime({
  modelsDir: args.modelsDir,
  modelUri: args.modelUri ?? undefined,
  contextSize: args.contextSize,
  port: args.serverPort,
});
let binaries: BinaryPaths | null = null;
let rpc: RpcServerAgent | null = null;
let binaryStatus = "fetching llama.cpp runtime";

const panel = new StatusPanel(args.plain);
const metrics: NodeMetrics = emptyMetrics();

let lastHardware: HardwareSnapshot | null = null;
let lastGovernor = governor.update(
  {
    cpuModel: "",
    cpuCores: 1,
    totalMemoryMB: 0,
    availableMemoryMB: 0,
    gpu: null,
    thermalState: "unknown",
    onBattery: false,
    batteryPct: null,
  },
  0,
  false
);
let connection = "starting";
let connected = false;
let modelPhase = "not loaded";
let activeModel = args.modelId ?? "none";
let lastEvent = "starting up";
let throughputSum = 0;
let throughputSamples = 0;

async function buildProfile(): Promise<CapabilityProfile> {
  const snapshot = await hardware.snapshot(runtime.vram());
  lastHardware = snapshot;
  const ownerBusy = hardware.ownerBusyPct;
  lastGovernor = governor.update(snapshot, ownerBusy, runtime.isReady());

  return {
    nodeId,
    label: args.label,
    kind: "desktop-worker",
    platform: platformInfo(),
    cpu: {
      model: snapshot.cpuModel,
      cores: snapshot.cpuCores,
      available: Math.max(0, Math.min(1, 1 - ownerBusy / 100)),
    },
    gpu: snapshot.gpu,
    memory: { total: snapshot.totalMemoryMB, available: snapshot.availableMemoryMB },
    network: network.profile,
    userState: {
      activity: classifyActivity(ownerBusy),
      thermalState: snapshot.thermalState,
      onBattery: snapshot.onBattery,
      batteryPct: snapshot.batteryPct,
    },
    runtime: {
      engine: runtime.unavailableReason ? "none" : "llama.cpp-rpc",
      ready: runtime.isReady(),
      loadedModels: runtime.loadedModels(),
      supportedModels: runtime.supportedModels(),
      rpc: rpc
        ? {
            endpoint: rpc.endpoint,
            // The governor's ceiling applies to a slice of a model exactly as
            // it does to a whole one: it is a limit on weights held in memory,
            // regardless of whether those weights are the entire network.
            offeredMemoryMB: Math.round(
              Math.min(
                rpc.offeredMemoryMB * lastGovernor.capacity,
                lastGovernor.maxModelMemoryMB || Number.POSITIVE_INFINITY
              )
            ),
            devices: rpc.devices.map((d) => ({
              name: d.name,
              description: d.description,
              totalMB: d.totalMB,
              freeMB: d.freeMB,
            })),
            canHead: !args.noHead,
            build: LLAMA_BUILD,
          }
        : null,
    },
  };
}

const scheduler = new WorkerScheduler(runtime, governor, {
  onAccepted(spec) {
    client.send({ type: "task:accepted", jobId: spec.jobId, taskId: spec.taskId });
    lastEvent = `running ${spec.phase} task #${spec.index + 1}`;
  },
  onRejected(spec, reason) {
    client.send({ type: "task:rejected", jobId: spec.jobId, taskId: spec.taskId, reason });
    lastEvent = `declined a task: ${reason}`;
  },
  onWarmupNeeded(requestedModelId) {
    void preloadModel(requestedModelId);
  },
  async onPrepareTask(spec) {
    // Either drive a multi-node pipeline the coordinator planned, or stand up
    // a one-member pipeline pointing at this machine. Same code path both ways,
    // which is what makes the split-versus-solo timings comparable.
    const config = spec.pipeline
      ? {
          modelId: spec.pipeline.modelId,
          members: spec.pipeline.members.map((m) => ({
            nodeId: m.nodeId,
            label: m.label,
            endpoint: m.endpoint,
            share: m.share,
          })),
        }
      : soloConfig(spec.modelId);

    if (spec.pipeline) {
      lastEvent =
        `heading a pipeline for ${spec.pipeline.modelId} across ` +
        `${spec.pipeline.members.length} nodes (${spec.pipeline.members.map((m) => m.label).join(" → ")})`;
    }
    await runtime.configure(config, reportModelProgress(config.modelId));
  },
  onToken(spec, token) {
    client.send({ type: "task:token", jobId: spec.jobId, taskId: spec.taskId, token });
  },
  onCompleted(spec, output, taskMetrics) {
    client.send({
      type: "task:completed",
      jobId: spec.jobId,
      taskId: spec.taskId,
      output,
      metrics: taskMetrics,
    });
    metrics.tasksCompleted += 1;
    metrics.samples += 1;
    metrics.ttftMs = taskMetrics.ttftMs;
    throughputSum += taskMetrics.tokensPerSecond;
    throughputSamples += 1;
    metrics.tokensPerSecond = throughputSum / throughputSamples;
    lastEvent = `finished task #${spec.index + 1} — ${taskMetrics.tokens} tokens at ${taskMetrics.tokensPerSecond.toFixed(1)} tok/s`;
  },
  onFailed(spec, error) {
    client.send({ type: "task:failed", jobId: spec.jobId, taskId: spec.taskId, error });
    metrics.tasksFailed += 1;
    lastEvent = `task #${spec.index + 1} failed: ${error}`;
  },
});

const client = new WorkerClient(
  {
    wsUrl: toWebSocketUrl(args.coordinator),
    token: args.token,
    buildProfile,
    buildGovernor: () => lastGovernor,
    buildMetrics: () => metrics,
    activeTasks: () => scheduler.activeTasks,
  },
  {
    onRegistered(id) {
      lastEvent = `registered with the coordinator as ${id}`;
      if (args.modelId) void preloadModel(args.modelId);
    },
    onRejected(reason) {
      panel.note(`\n  Coordinator rejected this worker: ${reason}\n`);
      lastEvent = `rejected: ${reason}`;
    },
    onConnectionChange(isConnected, detail) {
      connected = isConnected;
      connection = detail;
    },
    onMessage(message) {
      switch (message.type) {
        case "task:assign": {
          const spec = message.spec;
          void scheduler.accept(
            spec,
            lastGovernor.maxConcurrentTasks,
            lastGovernor.capacity
          );
          break;
        }
        case "task:cancel":
          scheduler.cancel(message.taskId);
          lastEvent = "coordinator cancelled a task";
          break;
        case "model:prepare":
          void preloadModel(message.modelId);
          break;
        default:
          break;
      }
    },
  }
);

/**
 * A pipeline of one. Requires this node's own RPC server, because the head
 * always reaches its members over RPC even when the only member is itself.
 */
function soloConfig(modelId: string): PipelineConfig {
  const endpoint = rpc?.endpoint;
  if (!endpoint) {
    throw new Error("this node is not serving devices over RPC, so it cannot run a model");
  }
  return {
    modelId,
    members: [{ nodeId, label: args.label, endpoint, share: 1 }],
  };
}

function reportModelProgress(modelId: string) {
  return (event: {
    phase: "downloading" | "loading" | "ready" | "error";
    progress: number;
    detail: string;
  }) => {
    modelPhase =
      event.phase === "downloading"
        ? `downloading ${Math.round(event.progress * 100)}% ${event.detail}`
        : event.phase === "loading"
          ? `loading ${Math.round(event.progress * 100)}%`
          : event.phase;
    client.send({
      type: "model:progress",
      modelId,
      phase: event.phase,
      progress: event.progress,
      detail: event.detail,
    });
  };
}

async function preloadModel(modelId: string): Promise<void> {
  const entry = getModel(modelId);
  if (!entry) return;
  if (runtime.loadedModels().includes(modelId)) return;
  activeModel = modelId;
  try {
    // Fetch the weights only. The pipeline itself is built per task, because
    // its membership depends on who is online at that moment.
    await runtime.prepare(modelId, reportModelProgress(modelId));
    modelPhase = "ready";
    lastEvent = `${entry.displayName} is cached and ready to serve`;
  } catch (err) {
    modelPhase = "error";
    lastEvent = `could not fetch ${modelId}: ${err instanceof Error ? err.message : String(err)}`;
  }
}

function statusModel(): StatusModel {
  const gpu = lastHardware?.gpu ?? null;
  const members = runtime.members;
  return {
    coordinator: args.coordinator,
    connection,
    connected,
    label: args.label,
    engine: runtime.unavailableReason
      ? `none — ${runtime.unavailableReason}`
      : `llama.cpp ${LLAMA_BUILD}${binaries ? ` (${binaries.variant})` : ""}`,
    device: gpu ? `${gpu.model} (${gpu.backend})` : `${lastHardware?.cpuModel ?? "CPU"} — CPU only`,
    vram: gpu && gpu.vram > 0
      ? `${(gpu.vram / 1024).toFixed(1)} GB total, ${((gpu.vram * gpu.available) / 1024).toFixed(1)} GB free`
      : "n/a",
    cpuOwnerPct: hardware.ownerBusyPct,
    gpuUsedPct: gpu ? (1 - gpu.available) * 100 : null,
    memory: lastHardware
      ? `${(lastHardware.availableMemoryMB / 1024).toFixed(1)} / ${(lastHardware.totalMemoryMB / 1024).toFixed(1)} GB free`
      : "…",
    governorState: lastGovernor.state,
    capacityPct: Math.round(lastGovernor.capacity * 100),
    reasons: lastGovernor.reasons,
    model: activeModel,
    modelPhase,
    activeTasks: scheduler.activeTasks,
    tasksCompleted: metrics.tasksCompleted,
    tokensPerSecond: metrics.tokensPerSecond,
    paused: governor.isManuallyPaused,
    lastEvent,
    rpcEndpoint: rpc?.endpoint ?? null,
    rpcSummary: rpc
      ? rpc.endpoint
        ? `${rpc.devices.map((d) => `${d.description} ${Math.round(d.freeMB / 1024)} GB`).join(", ") || "no devices"}`
        : (rpc.error ?? binaryStatus)
      : args.rpcEnabled
        ? binaryStatus
        : "disabled with --no-rpc",
    pipeline:
      members.length > 1
        ? `${members.length} nodes — ${members
            .map((m) => `${m.label} ${Math.round(m.share * 100)}%`)
            .join(" → ")}`
        : members.length === 1
          ? "solo (this machine holds the whole model)"
          : null,
  };
}

/**
 * Fetch the llama.cpp release and start serving this machine's devices. Done
 * after the worker is already connected, so a first run shows a download
 * progress bar in the dashboard instead of appearing to hang.
 */
async function startPipelineStack(): Promise<void> {
  try {
    binaries = await ensureLlamaBinaries({
      variant: args.llamaVariant,
      onProgress: (event) => {
        binaryStatus =
          event.phase === "downloading"
            ? `downloading runtime ${Math.round(event.progress * 100)}% — ${event.detail}`
            : event.phase === "extracting"
              ? "unpacking runtime"
              : "runtime ready";
        lastEvent = binaryStatus;
      },
    });
    runtime.attach(binaries);
  } catch (err) {
    binaryStatus = `runtime unavailable: ${err instanceof Error ? err.message : String(err)}`;
    runtime.unavailableReason = binaryStatus;
    lastEvent = binaryStatus;
    return;
  }

  if (!args.rpcEnabled) {
    binaryStatus = "not serving devices (--no-rpc)";
    return;
  }

  rpc = new RpcServerAgent({
    binaries,
    port: args.rpcPort,
    bindHost: args.rpcHost ?? undefined,
    advertiseHost: args.rpcAdvertise ?? undefined,
    threads: Math.max(1, Math.floor((lastHardware?.cpuCores ?? 4) / 2)),
    device: args.rpcDevice ?? undefined,
  });

  try {
    await rpc.start();
    lastEvent = `serving devices to the network on ${rpc.endpoint}`;
  } catch (err) {
    binaryStatus = `could not serve devices: ${err instanceof Error ? err.message : String(err)}`;
    lastEvent = binaryStatus;
  }
}

async function main(): Promise<void> {
  panel.note("\n  Community AI Worker starting…\n");
  await runtime.init();
  await network.maybeMeasureBandwidth();

  client.start();
  void startPipelineStack().then(() => {
    if (args.modelId) void preloadModel(args.modelId);
  });

  setInterval(() => {
    void (async () => {
      await network.maybeMeasureBandwidth();
      panel.render(statusModel());
    })();
  }, 1000);

  // Free memory on the exposed devices moves as the owner uses their machine,
  // so the number the coordinator plans against has to be refreshed.
  setInterval(() => {
    if (rpc?.running && runtime.members.length === 0) void rpc.probeDevices();
  }, 15_000);

  setupKeyboard();
}

function setupKeyboard(): void {
  const stdin = process.stdin;
  if (!stdin.isTTY) return;
  stdin.setRawMode(true);
  stdin.resume();
  stdin.setEncoding("utf8");
  stdin.on("data", (key: string) => {
    if (key === "p" || key === "P") {
      const next = !governor.isManuallyPaused;
      governor.setManualPause(next);
      if (next) {
        scheduler.cancelAll("owner paused contribution");
        // Pausing has to release the memory too, not just decline new work:
        // this node may be holding layers for someone else's pipeline.
        void runtime.teardown();
        void rpc?.stop();
        lastEvent = "contribution paused — layers released, devices withdrawn";
      } else {
        void startPipelineStack();
        lastEvent = "contribution resuming, capacity will ramp back up";
      }
    }
    if (key === "q" || key === "Q" || key === "\u0003") {
      void shutdown();
    }
  });
}

let shuttingDown = false;
async function shutdown(): Promise<void> {
  if (shuttingDown) return;
  shuttingDown = true;
  panel.note("\n  Stopping worker…");
  scheduler.cancelAll("worker shutting down");
  client.stop();
  await rpc?.stop();
  await runtime.dispose();
  process.exit(0);
}

process.on("SIGINT", () => void shutdown());
process.on("SIGTERM", () => void shutdown());

void main();
