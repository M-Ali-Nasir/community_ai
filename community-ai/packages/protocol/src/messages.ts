import { z } from "zod";
import { CapabilityProfile, GovernorReport, NodeMetrics } from "./capability.js";
import {
  ClusterPlan,
  JobRequest,
  JobView,
  SchedulingPolicy,
  TaskMetrics,
  TaskSpec,
} from "./workload.js";

export const PROTOCOL_VERSION = 1;

/* ------------------------------------------------------------------ *
 * Worker -> Coordinator
 * ------------------------------------------------------------------ */

export const WorkerToCoordinator = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("register"),
    protocolVersion: z.number().int(),
    token: z.string().default(""),
    profile: CapabilityProfile,
    governor: GovernorReport,
  }),
  z.object({
    type: z.literal("heartbeat"),
    profile: CapabilityProfile,
    governor: GovernorReport,
    metrics: NodeMetrics,
    activeTasks: z.number().int().nonnegative(),
  }),
  /** The worker's Resource Governor may refuse work the coordinator offered. */
  z.object({
    type: z.literal("task:accepted"),
    jobId: z.string(),
    taskId: z.string(),
  }),
  z.object({
    type: z.literal("task:rejected"),
    jobId: z.string(),
    taskId: z.string(),
    reason: z.string(),
  }),
  z.object({
    type: z.literal("task:token"),
    jobId: z.string(),
    taskId: z.string(),
    token: z.string(),
  }),
  z.object({
    type: z.literal("task:completed"),
    jobId: z.string(),
    taskId: z.string(),
    output: z.string(),
    metrics: TaskMetrics,
  }),
  z.object({
    type: z.literal("task:failed"),
    jobId: z.string(),
    taskId: z.string(),
    error: z.string(),
  }),
  z.object({
    type: z.literal("model:progress"),
    modelId: z.string(),
    phase: z.enum(["downloading", "loading", "ready", "error"]),
    progress: z.number().min(0).max(1),
    detail: z.string().default(""),
  }),
  z.object({ type: z.literal("pong"), nonce: z.string(), sentAtMs: z.number() }),
]);
export type WorkerToCoordinator = z.infer<typeof WorkerToCoordinator>;

/* ------------------------------------------------------------------ *
 * Coordinator -> Worker
 * ------------------------------------------------------------------ */

export const CoordinatorToWorker = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("registered"),
    nodeId: z.string(),
    sessionId: z.string(),
    heartbeatMs: z.number(),
    serverTimeMs: z.number(),
  }),
  z.object({ type: z.literal("rejected"), reason: z.string() }),
  z.object({ type: z.literal("task:assign"), spec: TaskSpec }),
  z.object({ type: z.literal("task:cancel"), jobId: z.string(), taskId: z.string() }),
  /** Ask a node to warm a model ahead of time. The node may decline. */
  z.object({ type: z.literal("model:prepare"), modelId: z.string() }),
  z.object({ type: z.literal("ping"), nonce: z.string(), sentAtMs: z.number() }),
]);
export type CoordinatorToWorker = z.infer<typeof CoordinatorToWorker>;

/* ------------------------------------------------------------------ *
 * Client (PWA) -> Coordinator
 * ------------------------------------------------------------------ */

export const ClientToCoordinator = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("subscribe"),
    protocolVersion: z.number().int(),
    token: z.string().default(""),
    label: z.string().default("web client"),
  }),
  z.object({ type: z.literal("job:submit"), jobId: z.string(), request: JobRequest }),
  z.object({ type: z.literal("job:cancel"), jobId: z.string() }),
  z.object({
    type: z.literal("settings"),
    policy: SchedulingPolicy.optional(),
    modelId: z.string().optional(),
  }),
]);
export type ClientToCoordinator = z.infer<typeof ClientToCoordinator>;

/* ------------------------------------------------------------------ *
 * Coordinator -> Client
 * ------------------------------------------------------------------ */

export const NodeView = z.object({
  nodeId: z.string(),
  label: z.string(),
  kind: CapabilityProfile.shape.kind,
  online: z.boolean(),
  profile: CapabilityProfile,
  governor: GovernorReport,
  metrics: NodeMetrics,
  activeTasks: z.number().int().nonnegative(),
  usableMemoryMB: z.number(),
  /** Measured throughput when available, otherwise the capability prior. */
  throughput: z.number(),
  throughputIsMeasured: z.boolean(),
  /** Set while a node is pulling or loading weights, so the UI can say so. */
  modelStatus: z
    .object({
      modelId: z.string(),
      phase: z.enum(["downloading", "loading", "ready", "error"]),
      progress: z.number().min(0).max(1),
      detail: z.string(),
    })
    .nullable(),
  lastSeenMs: z.number(),
});
export type NodeView = z.infer<typeof NodeView>;

export const NetworkStats = z.object({
  nodes: z.number().int(),
  desktopWorkers: z.number().int(),
  browserContributors: z.number().int(),
  contributing: z.number().int(),
  paused: z.number().int(),
  usableMemoryMB: z.number(),
  jobsCompleted: z.number().int(),
  tokensGenerated: z.number().int(),
});
export type NetworkStats = z.infer<typeof NetworkStats>;

export const CoordinatorToClient = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("snapshot"),
    nodes: z.array(NodeView),
    jobs: z.array(JobView),
    stats: NetworkStats,
    policy: SchedulingPolicy,
    modelId: z.string(),
    serverTimeMs: z.number(),
  }),
  z.object({ type: z.literal("job:planned"), jobId: z.string(), plan: ClusterPlan }),
  z.object({
    type: z.literal("job:token"),
    jobId: z.string(),
    taskId: z.string(),
    nodeId: z.string(),
    token: z.string(),
  }),
  z.object({
    type: z.literal("job:task"),
    jobId: z.string(),
    task: ClusterPlan.shape.tasks.element,
  }),
  z.object({ type: z.literal("job:completed"), job: JobView }),
  z.object({ type: z.literal("job:failed"), jobId: z.string(), error: z.string() }),
  z.object({ type: z.literal("error"), message: z.string() }),
]);
export type CoordinatorToClient = z.infer<typeof CoordinatorToClient>;

export function parseWorkerMessage(raw: unknown): WorkerToCoordinator {
  return WorkerToCoordinator.parse(raw);
}

export function parseClientMessage(raw: unknown): ClientToCoordinator {
  return ClientToCoordinator.parse(raw);
}

export function safeJsonParse(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}
