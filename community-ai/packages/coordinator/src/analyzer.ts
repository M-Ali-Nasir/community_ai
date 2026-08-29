import {
  type JobRequest,
  type NodeView,
  type WorkloadAnalysis,
  requireModel,
} from "@community-ai/protocol";
import { config } from "./config.js";
import { planPipeline } from "./pipeline.js";

/**
 * Workload Analyzer.
 *
 *                        REQUEST
 *                           |
 *                    Workload Analyzer
 *                           |
 *                 +---------+---------+
 *                 |                   |
 *          Independent tasks    Tightly coupled
 *                 |                   |
 *          task-parallel        single-node / model-parallel
 *
 * The point of this stage is that the strategy is *derived*, not configured.
 * A chat turn is sequential: token N+1 depends on token N, so spreading it over
 * a WAN adds a round-trip per token and makes it slower. A batch of independent
 * items has no such dependency and is the case where more devices genuinely win.
 */

/** Below this many independent items, coordination overhead outweighs the fan-out. */
const MIN_ITEMS_TO_DISTRIBUTE = 2;

export function analyzeWorkload(
  request: JobRequest,
  eligible: NodeView[],
  allNodes: NodeView[] = eligible
): WorkloadAnalysis {
  const model = requireModel(request.modelId);
  const rejected: WorkloadAnalysis["rejected"] = [];

  const nodesThatFit = eligible.filter(
    (n) => n.usableMemoryMB >= model.q4SizeMB * 1.25
  );

  if (request.kind === "chat") {
    // Decoding one response is inherently sequential.
    rejected.push({
      strategy: "task-parallel",
      reason:
        "A single chat turn has no independent sub-tasks: each token depends on the previous one.",
    });

    if (nodesThatFit.length > 0) {
      // One machine can hold it, so splitting could only add hops per token.
      rejected.push({
        strategy: "model-parallel",
        reason:
          `${model.displayName} fits in one node's memory. Splitting the layers would add a ` +
          "network round-trip to every token for no gain in what is runnable.",
      });
      return {
        strategy: "single-node",
        coupling: "tight",
        reason:
          `${model.displayName} fits on ${nodesThatFit.length} node(s). Keeping the whole turn on one node: ` +
          "distributing a sequential decode would add a network hop per token.",
        targetNodes: 1,
        unitCount: 1,
        rejected,
      };
    }

    // Nothing can hold it alone. This is the case layer-splitting exists for.
    if (config.enableModelParallel) {
      const attempt = planPipeline(allNodes, request.modelId);
      if (attempt.plan) {
        return {
          strategy: "model-parallel",
          coupling: "tight",
          reason: attempt.plan.reason,
          targetNodes: attempt.plan.members.length,
          unitCount: 1,
          rejected,
        };
      }
      rejected.push({ strategy: "model-parallel", reason: attempt.reason });
    } else {
      rejected.push({
        strategy: "model-parallel",
        reason: "Disabled by configuration (ENABLE_MODEL_PARALLEL=false).",
      });
    }

    return {
      strategy: "single-node",
      coupling: "tight",
      reason:
        `No node has room for ${model.displayName} and no pipeline could be formed. ` +
        "Falling back to the largest available node, which will page weights and run slowly.",
      targetNodes: Math.min(1, eligible.length),
      unitCount: 1,
      rejected,
    };
  }

  // Batch: independent units, the case task-parallel exists for.
  const unitCount = request.items.length;

  if (unitCount === 0) {
    return {
      strategy: "single-node",
      coupling: "independent",
      reason: "Batch job submitted with no items.",
      targetNodes: 0,
      unitCount: 0,
      rejected,
    };
  }

  if (unitCount < MIN_ITEMS_TO_DISTRIBUTE || nodesThatFit.length < 2) {
    rejected.push({
      strategy: "task-parallel",
      reason:
        unitCount < MIN_ITEMS_TO_DISTRIBUTE
          ? `Only ${unitCount} item: fan-out costs more than it saves.`
          : `Only ${nodesThatFit.length} node can hold ${model.displayName}.`,
    });
    return {
      strategy: "single-node",
      coupling: "independent",
      reason:
        unitCount < MIN_ITEMS_TO_DISTRIBUTE
          ? "A single work item goes to a single node."
          : `Only one node has room for ${model.displayName}, so there is nothing to spread across.`,
      targetNodes: Math.min(1, eligible.length),
      unitCount,
      rejected,
    };
  }

  const targetNodes = Math.min(nodesThatFit.length, unitCount);
  return {
    strategy: "task-parallel",
    coupling: "independent",
    reason:
      `${unitCount} independent items across ${targetNodes} nodes that can hold ${model.displayName}. ` +
      "Items are assigned in proportion to each node's measured throughput.",
    targetNodes,
    unitCount,
    rejected,
  };
}
