import { z } from "zod";

/**
 * A node advertises what it can contribute *right now*, not what hardware it owns.
 *
 * The coordinator never branches on "is this an RTX 4090". It asks the profile
 * for available capacity, memory headroom and link quality. That keeps NVIDIA,
 * AMD, Intel, Apple Silicon and CPU-only nodes on one abstraction.
 */

export const NodeKind = z.enum([
  /** Primary compute. Native worker process on a desktop/laptop. */
  "desktop-worker",
  /** Optional, experimental compute. Browser tab with WebGPU. May vanish at any time. */
  "browser-contributor",
  /** Submits work, contributes nothing. */
  "client",
]);
export type NodeKind = z.infer<typeof NodeKind>;

export const UserActivity = z.enum(["idle", "light", "active", "busy"]);
export type UserActivity = z.infer<typeof UserActivity>;

export const ThermalState = z.enum(["normal", "warm", "hot", "critical", "unknown"]);
export type ThermalState = z.infer<typeof ThermalState>;

export const AcceleratorBackend = z.enum([
  "cuda",
  "vulkan",
  "metal",
  "webgpu",
  "cpu",
  "none",
]);
export type AcceleratorBackend = z.infer<typeof AcceleratorBackend>;

export const CpuProfile = z.object({
  model: z.string(),
  cores: z.number().int().nonnegative(),
  /** Fraction of the CPU currently free, 0..1 */
  available: z.number().min(0).max(1),
});
export type CpuProfile = z.infer<typeof CpuProfile>;

export const GpuProfile = z.object({
  vendor: z.string(),
  model: z.string(),
  /** Megabytes of dedicated video memory, 0 when unknown or shared. */
  vram: z.number().nonnegative(),
  /** Fraction of the accelerator currently free, 0..1 */
  available: z.number().min(0).max(1),
  backend: AcceleratorBackend,
});
export type GpuProfile = z.infer<typeof GpuProfile>;

export const MemoryProfile = z.object({
  total: z.number().nonnegative(),
  available: z.number().nonnegative(),
});
export type MemoryProfile = z.infer<typeof MemoryProfile>;

export const NetworkProfile = z.object({
  /** Round-trip to the coordinator in milliseconds. */
  latency: z.number().nonnegative(),
  bandwidthMbps: z.number().nonnegative(),
  jitter: z.number().nonnegative(),
});
export type NetworkProfile = z.infer<typeof NetworkProfile>;

export const UserState = z.object({
  activity: UserActivity,
  thermalState: ThermalState,
  onBattery: z.boolean(),
  batteryPct: z.number().min(0).max(100).nullable(),
});
export type UserState = z.infer<typeof UserState>;

/**
 * What a node offers to a *layer-split* pipeline, which is a different
 * contribution from running a whole model. A node with 6 GB free can host a
 * slice of a 40 GB model even though it could never load one alone, so this is
 * advertised separately from the whole-model memory in `usableMemoryMB`.
 */
export const RpcProfile = z.object({
  /** `host:port` peers dial to place layers here. Null when not serving. */
  endpoint: z.string().nullable(),
  /** Free memory across every device this node exposes, as llama.cpp sees it. */
  offeredMemoryMB: z.number().nonnegative().default(0),
  devices: z
    .array(
      z.object({
        name: z.string(),
        description: z.string(),
        totalMB: z.number().nonnegative(),
        freeMB: z.number().nonnegative(),
      })
    )
    .default([]),
  /**
   * Whether this node can act as pipeline head. The head needs the full weight
   * file on disk (not in memory) plus a good link to every member, so a laptop
   * on a slow uplink should stay a member even when it has spare compute.
   */
  canHead: z.boolean().default(false),
  /** Build tag; peers must match exactly or the RPC handshake fails. */
  build: z.string().default(""),
});
export type RpcProfile = z.infer<typeof RpcProfile>;

export const RuntimeProfile = z.object({
  /** Which execution engine sits behind the AI Runtime interface. */
  engine: z.enum(["node-llama-cpp", "llama.cpp-rpc", "webllm", "none"]),
  ready: z.boolean(),
  /** Models already resident on this node, so the scheduler can prefer warm nodes. */
  loadedModels: z.array(z.string()).default([]),
  /** Model ids this node is willing and able to load. */
  supportedModels: z.array(z.string()).default([]),
  /** Absent on browser contributors, which cannot expose devices over RPC. */
  rpc: RpcProfile.nullable().default(null),
});
export type RuntimeProfile = z.infer<typeof RuntimeProfile>;

export const CapabilityProfile = z.object({
  nodeId: z.string().min(1),
  label: z.string().min(1),
  kind: NodeKind,
  platform: z.object({
    os: z.string(),
    arch: z.string(),
    version: z.string().default(""),
  }),
  cpu: CpuProfile,
  gpu: GpuProfile.nullable(),
  memory: MemoryProfile,
  network: NetworkProfile,
  userState: UserState,
  runtime: RuntimeProfile,
});
export type CapabilityProfile = z.infer<typeof CapabilityProfile>;

/**
 * The Resource Governor lives inside the worker and is the single authority on
 * how much of the owner's machine may be used. The coordinator decides WHAT a
 * node should do; this report states HOW MUCH the node will give.
 */
export const GovernorState = z.enum([
  "idle",
  "available",
  "contributing",
  "throttling",
  "paused",
  "resuming",
]);
export type GovernorState = z.infer<typeof GovernorState>;

export const GovernorReport = z.object({
  state: GovernorState,
  /** Fraction of spare resources the owner's machine is offering, 0..1 */
  capacity: z.number().min(0).max(1),
  maxConcurrentTasks: z.number().int().nonnegative(),
  /** Hard ceiling on model weights this node will hold, in MB. */
  maxModelMemoryMB: z.number().nonnegative(),
  /** Human-readable causes, surfaced in the worker CLI and the web dashboard. */
  reasons: z.array(z.string()).default([]),
  /** Owner pressed pause. Never overridden by the coordinator. */
  manualPause: z.boolean().default(false),
});
export type GovernorReport = z.infer<typeof GovernorReport>;

/** Rolling, measured performance. Not a spec sheet estimate. */
export const NodeMetrics = z.object({
  tokensPerSecond: z.number().nullable(),
  ttftMs: z.number().nullable(),
  tasksCompleted: z.number().int().nonnegative(),
  tasksFailed: z.number().int().nonnegative(),
  samples: z.number().int().nonnegative(),
});
export type NodeMetrics = z.infer<typeof NodeMetrics>;

export function emptyMetrics(): NodeMetrics {
  return {
    tokensPerSecond: null,
    ttftMs: null,
    tasksCompleted: 0,
    tasksFailed: 0,
    samples: 0,
  };
}

/**
 * Memory a node may actually devote to weights right now.
 * VRAM is first class; system RAM is discounted because host-side offload is slow.
 */
export function usableMemoryMB(
  profile: CapabilityProfile,
  governor: GovernorReport
): number {
  if (governor.state === "paused" || governor.capacity <= 0) return 0;
  const vram = profile.gpu ? profile.gpu.vram * profile.gpu.available * governor.capacity : 0;
  // Always leave the owner headroom so their session never swaps.
  const ramFloor = 1024;
  const ram = Math.max(0, profile.memory.available * governor.capacity - ramFloor);
  const usable = vram > 0 ? vram + 0.12 * ram : 0.35 * ram;
  return Math.min(usable, governor.maxModelMemoryMB || usable);
}

/**
 * Capability-relative throughput prior, used before a node has produced any
 * measured tokens/second. Deliberately vendor-neutral: it reads the profile,
 * not a table of GPU names.
 */
export function throughputPrior(
  profile: CapabilityProfile,
  governor: GovernorReport
): number {
  if (governor.state === "paused" || governor.capacity <= 0) return 0;
  let score = 0;
  if (profile.gpu && profile.gpu.backend !== "cpu" && profile.gpu.backend !== "none") {
    // VRAM size correlates with class of accelerator far better than any name match.
    const memFactor = Math.sqrt(Math.max(profile.gpu.vram, 512) / 1024);
    const backendFactor =
      profile.gpu.backend === "cuda"
        ? 1.0
        : profile.gpu.backend === "metal"
          ? 0.85
          : profile.gpu.backend === "vulkan"
            ? 0.7
            : 0.4; // webgpu in a browser tab is the least predictable
    score += 12 * memFactor * backendFactor * profile.gpu.available;
  }
  score += 0.25 * profile.cpu.cores * profile.cpu.available;
  return score * governor.capacity;
}
