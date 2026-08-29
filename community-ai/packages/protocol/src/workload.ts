import { z } from "zod";

/**
 * v0.1 supports two forms of distribution. The Workload Analyzer picks one.
 *
 *  single-node    one node does the whole thing; chosen when splitting would
 *                 only add network round-trips
 *  task-parallel  independent sub-tasks fan out across nodes, then reduce
 *                 (primary, proven path for v0.1)
 *  model-parallel one model's layers split across nodes (Phase 2 research
 *                 target; requires a runtime that exposes layer shards)
 */
export const DistributionStrategy = z.enum([
  "single-node",
  "task-parallel",
  "model-parallel",
]);
export type DistributionStrategy = z.infer<typeof DistributionStrategy>;

export const Coupling = z.enum(["independent", "tight"]);
export type Coupling = z.infer<typeof Coupling>;

export const SchedulingPolicy = z.enum([
  "adaptive",
  "best-node",
  "compute-only",
  "network-aware",
  "resource-aware",
  "round-robin",
]);
export type SchedulingPolicy = z.infer<typeof SchedulingPolicy>;

export const ChatMessage = z.object({
  role: z.enum(["system", "user", "assistant"]),
  content: z.string(),
});
export type ChatMessage = z.infer<typeof ChatMessage>;

/** What a client asks the network to do. */
export const JobRequest = z.object({
  kind: z.enum([
    /** One conversational turn. Latency-sensitive, inherently sequential. */
    "chat",
    /** Many independent units of work. The task-parallel showcase. */
    "batch",
  ]),
  modelId: z.string(),
  policy: SchedulingPolicy.default("adaptive"),
  messages: z.array(ChatMessage).default([]),
  /** For batch jobs: the independent units (documents, questions, chunks). */
  items: z.array(z.string()).default([]),
  /** Instruction applied to every batch item. */
  itemInstruction: z.string().default("Summarise the following text in two sentences."),
  /** Combine step run on one node after the map phase. Empty disables reduce. */
  reduceInstruction: z.string().default(""),
  maxTokens: z.number().int().positive().max(4096).default(256),
  temperature: z.number().min(0).max(2).default(0.7),
});
export type JobRequest = z.infer<typeof JobRequest>;

/**
 * One member of a layer-split pipeline. `share` is the fraction of the model's
 * layers this node holds; it is derived from free memory, not from speed,
 * because a member that cannot fit its slice fails the whole pipeline while a
 * merely slow member only makes it slower.
 */
export const PipelineMember = z.object({
  nodeId: z.string(),
  label: z.string(),
  endpoint: z.string(),
  share: z.number().min(0).max(1),
  /** Memory this member is expected to consume, for the dashboard. */
  assignedMB: z.number().nonnegative(),
});
export type PipelineMember = z.infer<typeof PipelineMember>;

/**
 * A pipeline is a virtual machine assembled out of several real ones. The head
 * owns the weight file on disk and drives the members, each of which holds a
 * contiguous slice of the layers in memory.
 */
export const PipelinePlan = z.object({
  modelId: z.string(),
  headNodeId: z.string(),
  members: z.array(PipelineMember),
  /** Pooled memory the pipeline assembled, in MB. */
  pooledMemoryMB: z.number().nonnegative(),
  /** Largest single member's memory, i.e. what one machine could have done. */
  bestSingleMemoryMB: z.number().nonnegative(),
  /**
   * Sum of round-trips along the pipeline. Every token traverses the whole
   * chain once, so this is a hard floor on time-per-token no matter how fast
   * the individual accelerators are.
   */
  estimatedHopMs: z.number().nonnegative(),
  /** Ceiling implied by estimatedHopMs alone, before any compute time. */
  latencyCeilingTokensPerSec: z.number().nonnegative(),
  reason: z.string(),
});
export type PipelinePlan = z.infer<typeof PipelinePlan>;

/** A single unit of work handed to exactly one node. */
export const TaskSpec = z.object({
  jobId: z.string(),
  taskId: z.string(),
  modelId: z.string(),
  messages: z.array(ChatMessage),
  maxTokens: z.number().int().positive(),
  temperature: z.number().min(0).max(2),
  /** Index within the job, for ordered reduction. */
  index: z.number().int().nonnegative(),
  /** Marks the reduce step so the UI can label it. */
  phase: z.enum(["map", "reduce", "chat"]).default("map"),
  /**
   * Set when this task must run as the head of a layer-split pipeline. The
   * receiving node brings the pipeline up before executing, and the assignment
   * is otherwise an ordinary task, so failure and reassignment work unchanged.
   */
  pipeline: PipelinePlan.nullable().default(null),
});
export type TaskSpec = z.infer<typeof TaskSpec>;

export const TaskMetrics = z.object({
  ttftMs: z.number().nonnegative(),
  totalMs: z.number().nonnegative(),
  tokens: z.number().int().nonnegative(),
  tokensPerSecond: z.number().nonnegative(),
  queueMs: z.number().nonnegative().default(0),
});
export type TaskMetrics = z.infer<typeof TaskMetrics>;

export const TaskStatus = z.enum([
  "planned",
  "assigned",
  "running",
  "completed",
  "failed",
  "reassigned",
  "cancelled",
]);
export type TaskStatus = z.infer<typeof TaskStatus>;

export const TaskView = z.object({
  taskId: z.string(),
  nodeId: z.string().nullable(),
  nodeLabel: z.string().nullable(),
  index: z.number().int(),
  phase: z.enum(["map", "reduce", "chat"]),
  status: TaskStatus,
  attempts: z.number().int().nonnegative(),
  output: z.string().default(""),
  metrics: TaskMetrics.nullable(),
  error: z.string().nullable(),
});
export type TaskView = z.infer<typeof TaskView>;

/** The Workload Analyzer's decision, shown verbatim in the UI. */
export const WorkloadAnalysis = z.object({
  strategy: DistributionStrategy,
  coupling: Coupling,
  reason: z.string(),
  /** How many nodes the analyzer wants to involve. */
  targetNodes: z.number().int().nonnegative(),
  unitCount: z.number().int().nonnegative(),
  /** Set when a strategy was considered and rejected, e.g. model-parallel. */
  rejected: z.array(z.object({ strategy: DistributionStrategy, reason: z.string() })).default([]),
});
export type WorkloadAnalysis = z.infer<typeof WorkloadAnalysis>;

export const ClusterPlan = z.object({
  jobId: z.string(),
  analysis: WorkloadAnalysis,
  policy: SchedulingPolicy,
  modelId: z.string(),
  nodeIds: z.array(z.string()),
  /** Share of the work each node received, keyed by nodeId. Sums to ~1. */
  shares: z.record(z.string(), z.number()),
  tasks: z.array(TaskView),
  formedAtMs: z.number(),
  /** Why these nodes, in plain language. */
  reason: z.string(),
  /** Set only for model-parallel jobs. */
  pipeline: PipelinePlan.nullable().default(null),
});
export type ClusterPlan = z.infer<typeof ClusterPlan>;

export const JobStatus = z.enum([
  "planning",
  "running",
  "reducing",
  "completed",
  "failed",
  "cancelled",
]);
export type JobStatus = z.infer<typeof JobStatus>;

export const JobView = z.object({
  jobId: z.string(),
  status: JobStatus,
  request: JobRequest,
  plan: ClusterPlan.nullable(),
  output: z.string().default(""),
  error: z.string().nullable(),
  startedAtMs: z.number(),
  finishedAtMs: z.number().nullable(),
  /** Measured, end to end, as the submitting client experiences it. */
  wallClockMs: z.number().nullable(),
  totalTokens: z.number().int().nonnegative().default(0),
});
export type JobView = z.infer<typeof JobView>;
