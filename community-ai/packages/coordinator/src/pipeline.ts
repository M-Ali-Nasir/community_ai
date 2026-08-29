import {
  type NodeView,
  type PipelineMember,
  type PipelinePlan,
  requireModel,
} from "@community-ai/protocol";
import { config } from "./config.js";

/**
 * Pipeline Planner.
 *
 * Assembles one virtual machine out of several real ones for a model that no
 * single member can hold. This is a different optimisation from the fan-out
 * scheduler and deliberately lives apart from it:
 *
 *   task-parallel   more nodes is better; work is independent, so the only
 *                   cost of adding a node is coordination
 *   model-parallel  more nodes is WORSE; every token traverses the whole chain,
 *                   so each extra member adds a round-trip to every single
 *                   token. Nodes are a cost to be minimised, not a resource
 *                   to be maximised.
 *
 * The objective is therefore: the fewest members whose pooled memory fits the
 * model, preferring members with the best links, then order them so adjacent
 * hops are cheap.
 */

/** Weights plus KV cache and activations need headroom beyond the file size. */
const MEMORY_OVERHEAD = 1.15;

/** Below this, a member contributes less than the round-trip it costs. */
const MIN_USEFUL_SHARE_MB = 256;

export interface PipelineCandidate {
  node: NodeView;
  endpoint: string;
  freeMB: number;
  latencyMs: number;
}

export interface PipelinePlanResult {
  plan: PipelinePlan | null;
  /** Why no pipeline could be formed, for the UI. */
  reason: string;
  pooledMemoryMB: number;
  neededMemoryMB: number;
}

/**
 * Nodes that can host part of a model right now: an RPC server is up, the
 * build matches, and the owner's governor is letting them contribute.
 */
export function pipelineCandidates(nodes: NodeView[], build?: string): PipelineCandidate[] {
  const out: PipelineCandidate[] = [];
  for (const node of nodes) {
    const rpc = node.profile.runtime.rpc;
    if (!node.online || !rpc || !rpc.endpoint) continue;
    if (node.governor.manualPause || node.governor.state === "paused") continue;
    if (node.governor.capacity <= 0.02) continue;
    // A mismatched build fails at the RPC handshake with an opaque error, so
    // it is cheaper to exclude the node than to debug it later.
    if (build && rpc.build && rpc.build !== build) continue;

    // Already governor-adjusted by the worker. The coordinator decides what a
    // node should do; the node alone decides how much it will give, so
    // re-applying capacity here would discount the same limit twice.
    if (rpc.offeredMemoryMB < MIN_USEFUL_SHARE_MB) continue;
    out.push({
      node,
      endpoint: rpc.endpoint,
      freeMB: Math.floor(rpc.offeredMemoryMB),
      latencyMs: Math.max(node.profile.network.latency, 0.1),
    });
  }
  return out;
}

export function totalPooledMemoryMB(candidates: PipelineCandidate[]): number {
  return candidates.reduce((sum, c) => sum + c.freeMB, 0);
}

export function neededMemoryMB(modelId: string): number {
  return Math.round(requireModel(modelId).q4SizeMB * MEMORY_OVERHEAD);
}

/**
 * Cost of a member for pipeline purposes. Memory is what makes the model
 * runnable at all, so it dominates; latency is a tiebreak that becomes
 * significant once several members could each supply the needed capacity.
 */
function memoryFirstRank(a: PipelineCandidate, b: PipelineCandidate): number {
  if (b.freeMB !== a.freeMB) return b.freeMB - a.freeMB;
  return a.latencyMs - b.latencyMs;
}

export function planPipeline(
  nodes: NodeView[],
  modelId: string,
  options: { build?: string; maxMembers?: number } = {}
): PipelinePlanResult {
  const model = requireModel(modelId);
  const needed = neededMemoryMB(modelId);
  const candidates = pipelineCandidates(nodes, options.build);
  const pooled = totalPooledMemoryMB(candidates);
  const maxMembers = options.maxMembers ?? config.maxPipelineMembers;

  if (candidates.length === 0) {
    return {
      plan: null,
      reason:
        "No node is exposing devices over RPC. Workers need the pipeline runtime enabled " +
        "(it starts automatically once the llama.cpp binaries finish downloading).",
      pooledMemoryMB: 0,
      neededMemoryMB: needed,
    };
  }

  if (pooled < needed) {
    return {
      plan: null,
      reason:
        `${model.displayName} needs about ${needed} MB but the whole network is only offering ` +
        `${Math.round(pooled)} MB across ${candidates.length} node(s). Add a node or pick a smaller model.`,
      pooledMemoryMB: pooled,
      neededMemoryMB: needed,
    };
  }

  // Greedy on memory: taking the largest members first minimises the member
  // count, and member count is what each token pays for.
  const ranked = [...candidates].sort(memoryFirstRank);
  const chosen: PipelineCandidate[] = [];
  let capacity = 0;
  for (const candidate of ranked) {
    if (capacity >= needed) break;
    if (chosen.length >= maxMembers) break;
    chosen.push(candidate);
    capacity += candidate.freeMB;
  }

  if (capacity < needed) {
    return {
      plan: null,
      reason:
        `${model.displayName} would need more than ${maxMembers} nodes, and every extra hop ` +
        `costs a round-trip on every token. Raise MAX_PIPELINE_MEMBERS to override.`,
      pooledMemoryMB: pooled,
      neededMemoryMB: needed,
    };
  }

  // Cheap links adjacent to each other, so the chain the tokens walk is short.
  chosen.sort((a, b) => a.latencyMs - b.latencyMs);

  const head = pickHead(chosen);
  const members: PipelineMember[] = chosen.map((c) => ({
    nodeId: c.node.nodeId,
    label: c.node.label,
    endpoint: c.endpoint,
    share: c.freeMB / capacity,
    assignedMB: Math.round((c.freeMB / capacity) * needed),
  }));

  const hopMs = estimateHopMs(chosen);
  const ceiling = hopMs > 0 ? 1000 / hopMs : 0;
  const bestSingle = Math.max(...candidates.map((c) => c.freeMB));

  return {
    plan: {
      modelId,
      headNodeId: head.node.nodeId,
      members,
      pooledMemoryMB: Math.round(capacity),
      bestSingleMemoryMB: Math.round(bestSingle),
      estimatedHopMs: Math.round(hopMs * 10) / 10,
      latencyCeilingTokensPerSec: Math.round(ceiling * 10) / 10,
      reason:
        `${model.displayName} needs ~${needed} MB, more than any single node here can hold ` +
        `(largest is ${Math.round(bestSingle)} MB). Split across ${members.length} node(s) ` +
        `pooling ${Math.round(capacity)} MB, with ${head.node.label} as head. ` +
        `Each token crosses ${members.length - 1} hop(s), so network latency caps this at ` +
        `roughly ${ceiling.toFixed(1)} tok/s before any compute time.`,
    },
    reason: "",
    pooledMemoryMB: pooled,
    neededMemoryMB: needed,
  };
}

/**
 * The head reads the GGUF from disk and feeds every member, so it wants the
 * best link rather than the most memory. It must also be willing to head:
 * doing so requires the whole weight file on local disk.
 */
function pickHead(chosen: PipelineCandidate[]): PipelineCandidate {
  const willing = chosen.filter((c) => c.node.profile.runtime.rpc?.canHead);
  const pool = willing.length > 0 ? willing : chosen;
  return [...pool].sort((a, b) => a.latencyMs - b.latencyMs)[0] as PipelineCandidate;
}

/**
 * Round-trip a token pays walking the chain.
 *
 * Approximation: we measure each node's RTT to the coordinator, not to its
 * neighbour, so the hop between two members is estimated as the mean of their
 * coordinator RTTs. That is only exact when the coordinator sits on the path.
 * Measuring true pairwise RTT is the next step and will change these numbers.
 */
export function estimateHopMs(chain: PipelineCandidate[]): number {
  if (chain.length < 2) return 0;
  let total = 0;
  for (let i = 0; i < chain.length - 1; i += 1) {
    const a = chain[i] as PipelineCandidate;
    const b = chain[i + 1] as PipelineCandidate;
    total += (a.latencyMs + b.latencyMs) / 2;
  }
  return total;
}

/**
 * A single-member "pipeline", used when one node can hold the model. Keeping
 * one execution path for both cases is what makes the split-versus-single
 * comparison trustworthy.
 */
export function soloPipeline(node: NodeView, modelId: string): PipelinePlan | null {
  const rpc = node.profile.runtime.rpc;
  if (!rpc?.endpoint) return null;
  const needed = neededMemoryMB(modelId);
  return {
    modelId,
    headNodeId: node.nodeId,
    members: [
      {
        nodeId: node.nodeId,
        label: node.label,
        endpoint: rpc.endpoint,
        share: 1,
        assignedMB: needed,
      },
    ],
    pooledMemoryMB: Math.round(rpc.offeredMemoryMB),
    bestSingleMemoryMB: Math.round(rpc.offeredMemoryMB),
    estimatedHopMs: 0,
    latencyCeilingTokensPerSec: 0,
    reason: `${node.label} holds the whole model, so there are no pipeline hops.`,
  };
}
