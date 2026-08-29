import type { WebSocket } from "ws";
import {
  type CapabilityProfile,
  type GovernorReport,
  type NodeMetrics,
  type NodeView,
  emptyMetrics,
  throughputPrior,
  usableMemoryMB,
} from "@community-ai/protocol";
import { config } from "./config.js";

/**
 * Device Registry.
 *
 * Holds the live capability profile of every connected node. It records what a
 * node reports and what the coordinator itself measures (round-trip latency and
 * jitter), but it never overrides the node's own Resource Governor.
 */

export interface RegisteredNode {
  nodeId: string;
  sessionId: string;
  socket: WebSocket;
  profile: CapabilityProfile;
  governor: GovernorReport;
  metrics: NodeMetrics;
  activeTasks: number;
  connectedAtMs: number;
  lastSeenMs: number;
  /** Coordinator-measured RTT samples, authoritative over self-reported values. */
  rttSamples: number[];
  pendingPings: Map<string, number>;
  modelStatus: NodeView["modelStatus"];
}

const EWMA_ALPHA = 0.3;

export class DeviceRegistry {
  private nodes = new Map<string, RegisteredNode>();

  add(node: RegisteredNode): void {
    const existing = this.nodes.get(node.nodeId);
    if (existing && existing.sessionId !== node.sessionId) {
      // Same machine reconnecting. Drop the stale socket, keep learned metrics.
      node.metrics = existing.metrics;
      try {
        existing.socket.close(4000, "replaced by newer session");
      } catch {
        /* socket already gone */
      }
    }
    this.nodes.set(node.nodeId, node);
  }

  remove(nodeId: string): RegisteredNode | undefined {
    const node = this.nodes.get(nodeId);
    if (node) this.nodes.delete(nodeId);
    return node;
  }

  get(nodeId: string): RegisteredNode | undefined {
    return this.nodes.get(nodeId);
  }

  all(): RegisteredNode[] {
    return [...this.nodes.values()];
  }

  updateHeartbeat(
    nodeId: string,
    profile: CapabilityProfile,
    governor: GovernorReport,
    metrics: NodeMetrics,
    activeTasks: number
  ): void {
    const node = this.nodes.get(nodeId);
    if (!node) return;
    node.profile = { ...profile, network: { ...profile.network, ...this.measuredNetwork(node) } };
    node.governor = governor;
    node.activeTasks = activeTasks;
    node.lastSeenMs = Date.now();
    // Keep coordinator-side counters; the worker only reports its own view.
    node.metrics = {
      ...node.metrics,
      tokensPerSecond: metrics.tokensPerSecond ?? node.metrics.tokensPerSecond,
      ttftMs: metrics.ttftMs ?? node.metrics.ttftMs,
    };
  }

  /** Fold a completed task's real numbers into the node's rolling performance. */
  recordTaskResult(nodeId: string, tokensPerSecond: number, ttftMs: number): void {
    const node = this.nodes.get(nodeId);
    if (!node) return;
    const m = node.metrics;
    m.tokensPerSecond =
      m.tokensPerSecond === null
        ? tokensPerSecond
        : EWMA_ALPHA * tokensPerSecond + (1 - EWMA_ALPHA) * m.tokensPerSecond;
    m.ttftMs = m.ttftMs === null ? ttftMs : EWMA_ALPHA * ttftMs + (1 - EWMA_ALPHA) * m.ttftMs;
    m.tasksCompleted += 1;
    m.samples += 1;
  }

  recordTaskFailure(nodeId: string): void {
    const node = this.nodes.get(nodeId);
    if (node) node.metrics.tasksFailed += 1;
  }

  recordModelStatus(nodeId: string, status: NonNullable<NodeView["modelStatus"]>): void {
    const node = this.nodes.get(nodeId);
    if (!node) return;
    // Once a model is resident the profile's loadedModels says so; keeping a
    // stale "ready" banner around would just be noise.
    node.modelStatus = status.phase === "ready" ? null : status;
  }

  recordPingSent(nodeId: string, nonce: string): void {
    const node = this.nodes.get(nodeId);
    if (!node) return;
    node.pendingPings.set(nonce, Date.now());
    // Bound the map if a node stops answering.
    if (node.pendingPings.size > 10) {
      const oldest = [...node.pendingPings.keys()][0];
      if (oldest !== undefined) node.pendingPings.delete(oldest);
    }
  }

  recordPong(nodeId: string, nonce: string): void {
    const node = this.nodes.get(nodeId);
    if (!node) return;
    const sent = node.pendingPings.get(nonce);
    if (sent === undefined) return;
    node.pendingPings.delete(nonce);
    node.rttSamples.push(Date.now() - sent);
    if (node.rttSamples.length > 20) node.rttSamples.shift();
    node.lastSeenMs = Date.now();
  }

  /** Latency and jitter as observed by the coordinator, not as claimed by the node. */
  private measuredNetwork(node: RegisteredNode): { latency: number; jitter: number } | object {
    if (node.rttSamples.length === 0) return {};
    const mean = node.rttSamples.reduce((a, b) => a + b, 0) / node.rttSamples.length;
    const variance =
      node.rttSamples.reduce((a, b) => a + (b - mean) ** 2, 0) / node.rttSamples.length;
    return { latency: Math.round(mean * 10) / 10, jitter: Math.round(Math.sqrt(variance) * 10) / 10 };
  }

  /** Nodes that have gone silent past the timeout. */
  stale(now = Date.now()): RegisteredNode[] {
    return this.all().filter((n) => now - n.lastSeenMs > config.nodeTimeoutMs);
  }

  toViews(): NodeView[] {
    const now = Date.now();
    return this.all()
      .map((node) => {
        const measured = node.metrics.tokensPerSecond;
        return {
          nodeId: node.nodeId,
          label: node.profile.label,
          kind: node.profile.kind,
          online: now - node.lastSeenMs <= config.nodeTimeoutMs,
          profile: node.profile,
          governor: node.governor,
          metrics: node.metrics,
          activeTasks: node.activeTasks,
          usableMemoryMB: Math.round(usableMemoryMB(node.profile, node.governor)),
          throughput:
            measured !== null && measured > 0
              ? measured
              : throughputPrior(node.profile, node.governor),
          throughputIsMeasured: measured !== null && measured > 0,
          modelStatus: node.modelStatus,
          lastSeenMs: node.lastSeenMs,
        } satisfies NodeView;
      })
      .sort((a, b) => b.throughput - a.throughput);
  }
}

export function initialNodeMetrics(): NodeMetrics {
  return emptyMetrics();
}
