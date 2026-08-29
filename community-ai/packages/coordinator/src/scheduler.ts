import {
  type NodeView,
  type SchedulingPolicy,
  type WorkloadAnalysis,
  requireModel,
} from "@community-ai/protocol";
import { config } from "./config.js";

/**
 * Cluster Scheduler.
 *
 * Forms a temporary cluster for one job. This is the part of the system the
 * research is actually about, so every policy is implemented side by side and
 * can be swapped per request to compare them on the same live network.
 */

export interface Candidate {
  node: NodeView;
  /** Throughput, measured where possible, capability prior otherwise. */
  throughput: number;
  /** Penalty applied for link quality. */
  latencyMs: number;
  /** True when the node already holds the model, so it skips a cold load. */
  warm: boolean;
}

export interface SchedulingResult {
  nodeIds: string[];
  shares: Record<string, number>;
  reason: string;
}

export interface EligibilityOptions {
  /**
   * Cluster formation wants nodes that can start immediately. Reassigning an
   * orphaned task instead wants anyone *capable*, because the task can sit in
   * that node's queue until it frees up — refusing to queue is how a task ends
   * up with nowhere to go when its node dies while the others are busy.
   */
  allowBusy?: boolean;
  /**
   * Accept nodes that could serve the model after downloading it. Off by
   * default: a cold node holds the task for minutes while it pulls gigabytes,
   * which looks identical to a hang from the coordinator's side. Warming is
   * driven separately by `model:prepare`.
   */
  allowCold?: boolean;
}

export function eligibleNodes(
  nodes: NodeView[],
  modelId: string,
  options: EligibilityOptions = {}
): NodeView[] {
  const model = requireModel(modelId);
  const needed = model.q4SizeMB * 1.25;
  return nodes.filter((n) => {
    if (!n.online) return false;
    if (n.kind === "client") return false;
    if (n.kind === "browser-contributor" && !config.allowBrowserContributors) return false;
    if (n.governor.manualPause) return false;
    if (n.governor.state === "paused" || n.governor.capacity <= 0.02) return false;
    if (!options.allowBusy && n.activeTasks >= Math.max(1, n.governor.maxConcurrentTasks)) {
      return false;
    }
    if (n.usableMemoryMB < needed) return false;
    if (!n.profile.runtime.ready) return false;
    // A node must be willing to run this model at all.
    const supported = n.profile.runtime.supportedModels;
    if (supported.length > 0 && !supported.includes(modelId)) return false;
    // And must already hold it, unless a cold start is explicitly acceptable.
    if (!options.allowCold && !n.profile.runtime.loadedModels.includes(modelId)) return false;
    return true;
  });
}

/**
 * Nodes that could serve this model once warmed. Used to decide who to send
 * `model:prepare` to, and to explain to the user why a job could not run yet.
 */
export function warmableNodes(nodes: NodeView[], modelId: string): NodeView[] {
  return eligibleNodes(nodes, modelId, { allowBusy: true, allowCold: true }).filter(
    (n) => !n.profile.runtime.loadedModels.includes(modelId)
  );
}

/**
 * Per-node explanation of why the cluster could not be formed. A testnet of
 * mismatched machines is impossible to debug from "no eligible nodes".
 */
export function explainIneligibility(nodes: NodeView[], modelId: string): string[] {
  const model = requireModel(modelId);
  const needed = Math.round(model.q4SizeMB * 1.25);
  const out: string[] = [];

  for (const n of nodes) {
    if (n.kind === "client") continue;
    const why: string[] = [];
    if (!n.online) why.push("offline");
    if (n.governor.manualPause) why.push("owner paused contribution");
    else if (n.governor.state === "paused" || n.governor.capacity <= 0.02) {
      why.push(
        `governor at ${Math.round(n.governor.capacity * 100)}%` +
          (n.governor.reasons.length > 0 ? ` (${n.governor.reasons.join("; ")})` : "")
      );
    }
    if (n.usableMemoryMB < needed) {
      why.push(`${n.usableMemoryMB} MB usable, needs ${needed} MB`);
    }
    if (!n.profile.runtime.ready) why.push("inference runtime not ready");
    const supported = n.profile.runtime.supportedModels;
    if (supported.length > 0 && !supported.includes(modelId)) {
      why.push("does not serve this model");
    } else if (!n.profile.runtime.loadedModels.includes(modelId)) {
      why.push("model not loaded yet");
    }
    if (n.kind === "browser-contributor" && !config.allowBrowserContributors) {
      why.push("browser contributors disabled");
    }
    if (why.length > 0) out.push(`${n.label}: ${why.join(", ")}`);
  }
  return out;
}

function toCandidates(nodes: NodeView[], modelId: string): Candidate[] {
  return nodes.map((node) => ({
    node,
    throughput: Math.max(node.throughput, 0.01),
    latencyMs: Math.max(node.profile.network.latency, 0.1),
    warm: node.profile.runtime.loadedModels.includes(modelId),
  }));
}

/**
 * Effective value of a node for one task: how fast it decodes, discounted by
 * the round-trip we pay to reach it and by a cold-start penalty.
 */
function effectiveScore(c: Candidate, networkWeight: number): number {
  const warmBonus = c.warm ? 1.0 : 0.75;
  const latencyPenalty = 1 / (1 + networkWeight * (c.latencyMs / 100));
  // Native workers are the reliable tier; a browser tab can vanish mid-task.
  const tierFactor = c.node.kind === "desktop-worker" ? 1.0 : 0.6;
  return c.throughput * warmBonus * latencyPenalty * tierFactor;
}

function sharesFrom(chosen: Candidate[], networkWeight: number): Record<string, number> {
  const scores = chosen.map((c) => effectiveScore(c, networkWeight));
  const total = scores.reduce((a, b) => a + b, 0) || 1;
  const shares: Record<string, number> = {};
  chosen.forEach((c, i) => {
    shares[c.node.nodeId] = (scores[i] ?? 0) / total;
  });
  return shares;
}

function pickSingle(candidates: Candidate[], networkWeight: number): Candidate | undefined {
  return [...candidates].sort(
    (a, b) => effectiveScore(b, networkWeight) - effectiveScore(a, networkWeight)
  )[0];
}

/**
 * Marginal-benefit cut-off for fan-out. Adding a node only helps if it can
 * finish at least one unit faster than redistributing that unit to nodes we
 * already picked. Stops a 12 ms/token desktop being slowed down by a phone.
 */
function trimUnproductiveNodes(ranked: Candidate[], unitCount: number, networkWeight: number): Candidate[] {
  const chosen: Candidate[] = [];
  let aggregate = 0;
  for (const candidate of ranked) {
    if (chosen.length >= unitCount) break;
    const score = effectiveScore(candidate, networkWeight);
    // A node contributing under 8% of the cluster's current speed adds
    // scheduling and straggler risk for almost no throughput.
    if (chosen.length > 0 && score < aggregate * 0.08) continue;
    chosen.push(candidate);
    aggregate += score;
  }
  return chosen;
}

export function formCluster(
  analysis: WorkloadAnalysis,
  nodes: NodeView[],
  modelId: string,
  policy: SchedulingPolicy,
  roundRobinCursor = 0
): SchedulingResult {
  const candidates = toCandidates(nodes, modelId);
  if (candidates.length === 0) {
    return { nodeIds: [], shares: {}, reason: "No eligible nodes." };
  }

  const wantsFanOut = analysis.strategy === "task-parallel";
  const unitCount = Math.max(1, analysis.unitCount);

  switch (policy) {
    case "best-node": {
      const best = pickSingle(candidates, 1);
      if (!best) return { nodeIds: [], shares: {}, reason: "No eligible nodes." };
      return {
        nodeIds: [best.node.nodeId],
        shares: { [best.node.nodeId]: 1 },
        reason: `Baseline: everything on the single strongest node (${best.node.label}).`,
      };
    }

    case "compute-only": {
      const ranked = [...candidates].sort((a, b) => b.throughput - a.throughput);
      const chosen = wantsFanOut ? ranked.slice(0, unitCount) : ranked.slice(0, 1);
      return {
        nodeIds: chosen.map((c) => c.node.nodeId),
        shares: sharesFrom(chosen, 0),
        reason: "Compute-only: ranked purely by throughput, network latency ignored.",
      };
    }

    case "network-aware": {
      const ranked = [...candidates].sort(
        (a, b) => effectiveScore(b, 2.5) - effectiveScore(a, 2.5)
      );
      const chosen = wantsFanOut ? ranked.slice(0, unitCount) : ranked.slice(0, 1);
      return {
        nodeIds: chosen.map((c) => c.node.nodeId),
        shares: sharesFrom(chosen, 2.5),
        reason: "Network-aware: throughput weighted down hard by measured round-trip time.",
      };
    }

    case "resource-aware": {
      const ranked = [...candidates].sort(
        (a, b) =>
          b.node.usableMemoryMB * b.node.governor.capacity -
          a.node.usableMemoryMB * a.node.governor.capacity
      );
      const chosen = wantsFanOut ? ranked.slice(0, unitCount) : ranked.slice(0, 1);
      return {
        nodeIds: chosen.map((c) => c.node.nodeId),
        shares: sharesFrom(chosen, 1),
        reason:
          "Resource-aware: ranked by governor-approved spare memory rather than raw speed.",
      };
    }

    case "round-robin": {
      const ordered = [...candidates].sort((a, b) => a.node.nodeId.localeCompare(b.node.nodeId));
      const count = wantsFanOut ? Math.min(ordered.length, unitCount) : 1;
      const chosen: Candidate[] = [];
      for (let i = 0; i < count; i += 1) {
        const item = ordered[(roundRobinCursor + i) % ordered.length];
        if (item) chosen.push(item);
      }
      const even = 1 / Math.max(chosen.length, 1);
      return {
        nodeIds: chosen.map((c) => c.node.nodeId),
        shares: Object.fromEntries(chosen.map((c) => [c.node.nodeId, even])),
        reason: "Round-robin: equal share regardless of capability. Control condition.",
      };
    }

    case "adaptive":
    default: {
      const networkWeight = 1.5;
      const ranked = [...candidates].sort(
        (a, b) => effectiveScore(b, networkWeight) - effectiveScore(a, networkWeight)
      );

      if (!wantsFanOut) {
        const best = ranked[0];
        if (!best) return { nodeIds: [], shares: {}, reason: "No eligible nodes." };
        return {
          nodeIds: [best.node.nodeId],
          shares: { [best.node.nodeId]: 1 },
          reason:
            `Adaptive: work is tightly coupled, so it stays on ${best.node.label}` +
            (best.warm ? " (model already resident)." : " (fastest eligible node)."),
        };
      }

      const chosen = trimUnproductiveNodes(ranked, unitCount, networkWeight);
      const dropped = ranked.length - chosen.length;
      return {
        nodeIds: chosen.map((c) => c.node.nodeId),
        shares: sharesFrom(chosen, networkWeight),
        reason:
          `Adaptive: ${chosen.length} node(s) share ${unitCount} independent items, weighted by ` +
          "measured throughput and round-trip time" +
          (dropped > 0
            ? `. ${dropped} eligible node(s) left out: too slow to be worth the coordination.`
            : "."),
      };
    }
  }
}

/**
 * The reduce step is a single sequential call over all map outputs, so it goes
 * to the most reliable fast node. Browser tabs are avoided when a native worker
 * exists because losing the reduce loses the whole job.
 */
export function pickReduceNode(nodes: NodeView[], modelId: string): NodeView | undefined {
  const eligible = eligibleNodes(nodes, modelId);
  if (eligible.length === 0) return undefined;
  const desktops = eligible.filter((n) => n.kind === "desktop-worker");
  const pool = config.preferDesktopForReduce && desktops.length > 0 ? desktops : eligible;
  const candidates = toCandidates(pool, modelId);
  return pickSingle(candidates, 1.5)?.node;
}
