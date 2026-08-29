import {
  type CoordinatorToWorker,
  type GovernorReport,
  type NodeMetrics,
  type TaskSpec,
  MODEL_CATALOG,
  PROTOCOL_VERSION,
  emptyMetrics,
  getModel,
  safeJsonParse,
} from "@community-ai/protocol";
import WebllmWorker from "../contribute/webllm.worker.ts?worker";
import type { WorkerOut } from "../contribute/webllm.worker.ts";
import {
  BrowserGovernor,
  FrameMonitor,
  defaultBrowserGovernorOptions,
  watchBattery,
  type BatteryState,
  type BrowserGovernorOptions,
} from "./governor.js";
import { buildBrowserProfile, defaultBudgetMB, probeGpu, type BrowserGpuInfo } from "./capability.js";

/**
 * Browser contributor.
 *
 * Joins the network on the same worker protocol the desktop worker uses, so the
 * coordinator does not special-case it. It is opt-in and can disappear at any
 * moment; the scheduler already treats it as the lower-reliability tier.
 */

export interface ContributorStatus {
  enabled: boolean;
  connected: boolean;
  connection: string;
  gpu: BrowserGpuInfo | null;
  governor: GovernorReport | null;
  modelId: string | null;
  modelPhase: string;
  modelProgress: number;
  activeTask: string | null;
  tasksCompleted: number;
  tokensPerSecond: number | null;
  ownerBusy: number;
  battery: BatteryState;
  lastEvent: string;
  error: string | null;
}

const NODE_ID_KEY = "community-ai:node-id";

function stableNodeId(): string {
  const existing = localStorage.getItem(NODE_ID_KEY);
  if (existing) return existing;
  const created = `browser-${Math.random().toString(36).slice(2, 10)}`;
  localStorage.setItem(NODE_ID_KEY, created);
  return created;
}

import { resolveWsUrl } from "./useCoordinator.js";

function wsUrl(path: string): string {
  return resolveWsUrl(path);
}

export class Contributor {
  private socket: WebSocket | null = null;
  private engine: Worker | null = null;
  private governor = new BrowserGovernor();
  private frames = new FrameMonitor();
  private metrics: NodeMetrics = emptyMetrics();
  private heartbeatTimer: number | null = null;
  private reconnectTimer: number | null = null;
  private tickTimer: number | null = null;
  private backoffMs = 1000;
  private battery: BatteryState = { onBattery: false, level: null };
  private gpuInfo: BrowserGpuInfo | null = null;
  private modelId: string | null = null;
  private loadedModels: string[] = [];
  private runningTask: TaskSpec | null = null;
  private lastGovernor: GovernorReport | null = null;
  private rtt: number[] = [];
  private throughputSum = 0;
  private throughputSamples = 0;
  private options: BrowserGovernorOptions = {
    ...defaultBrowserGovernorOptions,
    budgetMB: defaultBudgetMB(),
  };
  private status: ContributorStatus = {
    enabled: false,
    connected: false,
    connection: "off",
    gpu: null,
    governor: null,
    modelId: null,
    modelPhase: "not loaded",
    modelProgress: 0,
    activeTask: null,
    tasksCompleted: 0,
    tokensPerSecond: null,
    ownerBusy: 0,
    battery: { onBattery: false, level: null },
    lastEvent: "",
    error: null,
  };

  constructor(
    private readonly token: string,
    private readonly onStatus: (status: ContributorStatus) => void
  ) {
    this.governor.setOptions(this.options);
    void watchBattery((state) => {
      this.battery = state;
    });
  }

  getStatus(): ContributorStatus {
    return this.status;
  }

  private emit(patch: Partial<ContributorStatus>): void {
    this.status = { ...this.status, ...patch };
    this.onStatus(this.status);
  }

  async probe(): Promise<BrowserGpuInfo> {
    if (!this.gpuInfo) {
      this.gpuInfo = await probeGpu();
      this.emit({ gpu: this.gpuInfo });
    }
    return this.gpuInfo;
  }

  setBudget(budgetMB: number): void {
    this.options = { ...this.options, budgetMB };
    this.governor.setOptions(this.options);
  }

  setOptions(patch: Partial<BrowserGovernorOptions>): void {
    this.options = { ...this.options, ...patch };
    this.governor.setOptions(this.options);
  }

  get budgetMB(): number {
    return this.options.budgetMB;
  }

  /** Models small enough for the memory this tab is willing to commit. */
  supportedModels(): string[] {
    return MODEL_CATALOG.filter(
      (m) => m.webllmMatch !== null && m.q4SizeMB * 1.25 <= this.options.budgetMB
    ).map((m) => m.id);
  }

  async start(modelId: string): Promise<void> {
    const gpu = await this.probe();
    this.modelId = modelId;
    this.governor.setManualPause(false);
    this.frames.start();
    this.emit({ enabled: true, error: null, modelId, lastEvent: "joining P2P mesh" });

    if (gpu.available) {
      this.startEngine();
      this.loadModel(modelId);
    }
    this.connect();
    this.startTicker();
  }

  stop(): void {
    this.governor.setManualPause(true);
    this.frames.stop();
    if (this.heartbeatTimer) window.clearInterval(this.heartbeatTimer);
    if (this.reconnectTimer) window.clearTimeout(this.reconnectTimer);
    if (this.tickTimer) window.clearInterval(this.tickTimer);
    this.heartbeatTimer = null;
    this.reconnectTimer = null;
    this.tickTimer = null;
    try {
      this.socket?.close(1000, "contributor stopped");
    } catch {
      /* already closed */
    }
    this.socket = null;
    this.engine?.postMessage({ type: "unload" });
    this.engine?.terminate();
    this.engine = null;
    this.loadedModels = [];
    this.runningTask = null;
    this.emit({
      enabled: false,
      connected: false,
      connection: "off",
      modelPhase: "not loaded",
      modelProgress: 0,
      activeTask: null,
      lastEvent: "contribution stopped",
    });
  }

  private startEngine(): void {
    if (this.engine) return;
    this.engine = new WebllmWorker();
    this.engine.addEventListener("message", (event: MessageEvent<WorkerOut>) => {
      this.onEngineMessage(event.data);
    });
  }

  private loadModel(modelId: string): void {
    const entry = getModel(modelId);
    if (!entry || !entry.webllmMatch) {
      this.emit({ error: `${modelId} has no browser build.` });
      return;
    }
    this.emit({ modelPhase: "resolving", modelProgress: 0, modelId });
    this.engine?.postMessage({ type: "load", catalogId: modelId, match: entry.webllmMatch });
  }

  private onEngineMessage(message: WorkerOut): void {
    switch (message.type) {
      case "progress":
        this.emit({
          modelPhase: message.phase,
          modelProgress: message.progress,
          lastEvent:
            message.phase === "downloading"
              ? `downloading weights ${Math.round(message.progress * 100)}%`
              : message.detail,
        });
        break;

      case "loaded":
        this.loadedModels = [message.catalogId];
        this.emit({
          modelPhase: "ready",
          modelProgress: 1,
          lastEvent: `${message.webllmId} is resident and ready`,
        });
        break;

      case "token": {
        const task = this.runningTask;
        if (task && task.taskId === message.taskId) {
          this.send({
            type: "task:token",
            jobId: task.jobId,
            taskId: task.taskId,
            token: message.token,
          });
        }
        break;
      }

      case "done": {
        const task = this.runningTask;
        if (!task || task.taskId !== message.taskId) break;
        this.send({
          type: "task:completed",
          jobId: task.jobId,
          taskId: task.taskId,
          output: message.text,
          metrics: {
            ttftMs: message.ttftMs,
            totalMs: message.totalMs,
            tokens: message.tokens,
            tokensPerSecond: message.tokensPerSecond,
            queueMs: 0,
          },
        });
        this.metrics.tasksCompleted += 1;
        this.metrics.samples += 1;
        this.metrics.ttftMs = message.ttftMs;
        this.throughputSum += message.tokensPerSecond;
        this.throughputSamples += 1;
        this.metrics.tokensPerSecond = this.throughputSum / this.throughputSamples;
        this.runningTask = null;
        this.governor.markContributing(false);
        this.emit({
          activeTask: null,
          tasksCompleted: this.metrics.tasksCompleted,
          tokensPerSecond: this.metrics.tokensPerSecond,
          lastEvent: `finished a task — ${message.tokens} tokens at ${message.tokensPerSecond.toFixed(1)} tok/s`,
        });
        break;
      }

      case "error": {
        const task = this.runningTask;
        if (task && (message.taskId === null || message.taskId === task.taskId)) {
          this.send({
            type: "task:failed",
            jobId: task.jobId,
            taskId: task.taskId,
            error: message.message,
          });
          this.metrics.tasksFailed += 1;
          this.runningTask = null;
        }
        this.emit({ activeTask: null, error: message.message, lastEvent: message.message });
        break;
      }

      case "probe:result":
        break;
    }
  }

  private connect(): void {
    if (this.governor.isPaused) return;
    this.emit({ connection: "connecting" });
    const socket = new WebSocket(wsUrl("/ws/worker"));
    this.socket = socket;

    socket.addEventListener("open", () => {
      this.backoffMs = 1000;
      void this.register();
    });

    socket.addEventListener("message", (event) => {
      const parsed = safeJsonParse(String(event.data));
      if (!parsed || typeof parsed !== "object") return;
      const message = parsed as CoordinatorToWorker;

      switch (message.type) {
        case "registered":
          this.emit({ connected: true, connection: "connected", lastEvent: "joined the network" });
          this.startHeartbeat(message.heartbeatMs);
          break;
        case "rejected":
          this.emit({ connected: false, connection: "rejected", error: message.reason });
          break;
        case "ping":
          this.send({ type: "pong", nonce: message.nonce, sentAtMs: message.sentAtMs });
          this.rtt.push(Math.max(0, Date.now() - message.sentAtMs));
          if (this.rtt.length > 20) this.rtt.shift();
          break;
        case "task:assign":
          this.handleAssignment(message.spec);
          break;
        case "task:cancel":
          if (this.runningTask?.taskId === message.taskId) {
            this.engine?.postMessage({ type: "cancel" });
            this.runningTask = null;
            this.emit({ activeTask: null, lastEvent: "task cancelled by the coordinator" });
          }
          break;
        case "model:prepare":
          if (this.loadedModels.includes(message.modelId)) break;
          this.loadModel(message.modelId);
          break;
      }
    });

    socket.addEventListener("close", () => {
      this.emit({ connected: false, connection: "disconnected" });
      if (this.heartbeatTimer) window.clearInterval(this.heartbeatTimer);
      this.heartbeatTimer = null;
      this.scheduleReconnect();
    });

    socket.addEventListener("error", () => {
      this.emit({ connection: "connection error" });
    });
  }

  private scheduleReconnect(): void {
    if (this.governor.isPaused || this.reconnectTimer) return;
    const delay = this.backoffMs;
    this.backoffMs = Math.min(15000, Math.round(this.backoffMs * 1.7));
    this.reconnectTimer = window.setTimeout(() => {
      this.reconnectTimer = null;
      this.connect();
    }, delay);
  }

  private handleAssignment(spec: TaskSpec): void {
    const governor = this.lastGovernor;
    const refusal =
      this.governor.isPaused
        ? "contribution is off"
        : !governor || governor.capacity <= 0
          ? `resource governor: ${governor?.reasons.join("; ") || "no spare capacity"}`
          : this.runningTask
            ? "already running a task"
            : !this.loadedModels.includes(spec.modelId)
              ? `${spec.modelId} is not loaded in this tab`
              : null;

    if (refusal) {
      this.send({ type: "task:rejected", jobId: spec.jobId, taskId: spec.taskId, reason: refusal });
      this.emit({ lastEvent: `declined a task: ${refusal}` });
      return;
    }

    this.runningTask = spec;
    this.governor.markContributing(true);
    this.send({ type: "task:accepted", jobId: spec.jobId, taskId: spec.taskId });
    this.emit({ activeTask: spec.taskId, lastEvent: `running ${spec.phase} task #${spec.index + 1}` });
    this.engine?.postMessage({
      type: "generate",
      taskId: spec.taskId,
      catalogId: spec.modelId,
      messages: spec.messages,
      maxTokens: spec.maxTokens,
      temperature: spec.temperature,
    });
  }

  private async register(): Promise<void> {
    this.send({
      type: "register",
      protocolVersion: PROTOCOL_VERSION,
      token: this.token,
      profile: this.profile(),
      governor: this.updateGovernor(),
    });
  }

  private startHeartbeat(intervalMs: number): void {
    if (this.heartbeatTimer) window.clearInterval(this.heartbeatTimer);
    this.heartbeatTimer = window.setInterval(() => {
      if (this.socket?.readyState !== WebSocket.OPEN) return;
      this.send({
        type: "heartbeat",
        profile: this.profile(),
        governor: this.updateGovernor(),
        metrics: this.metrics,
        activeTasks: this.runningTask ? 1 : 0,
      });
    }, Math.max(1000, intervalMs));
  }

  private startTicker(): void {
    if (this.tickTimer) window.clearInterval(this.tickTimer);
    this.tickTimer = window.setInterval(() => {
      const governor = this.updateGovernor();
      this.emit({
        governor,
        ownerBusy: this.frames.ownerBusy,
        battery: this.battery,
      });
    }, 1000);
  }

  private updateGovernor(): GovernorReport {
    const report = this.governor.update({
      ownerBusy: this.frames.ownerBusy,
      battery: this.battery,
      hidden: document.visibilityState === "hidden",
      runtimeReady: this.loadedModels.length > 0,
    });
    this.lastGovernor = report;
    return report;
  }

  private profile() {
    const rttMean =
      this.rtt.length > 0 ? this.rtt.reduce((a, b) => a + b, 0) / this.rtt.length : 0;
    const jitter =
      this.rtt.length > 1
        ? Math.sqrt(
            this.rtt.reduce((a, b) => a + (b - rttMean) ** 2, 0) / this.rtt.length
          )
        : 0;
    const entry = this.modelId ? getModel(this.modelId) : undefined;
    return buildBrowserProfile({
      nodeId: stableNodeId(),
      label: `${navigator.userAgent.includes("Mobile") ? "Phone" : "Browser"} · ${this.gpuInfo?.description ?? "WebGPU"}`,
      gpuInfo: this.gpuInfo ?? {
        available: false,
        shaderF16: false,
        vendor: "",
        architecture: "",
        description: "",
        reason: "",
      },
      budgetMB: this.options.budgetMB,
      ownerBusy: this.frames.ownerBusy,
      memoryUsedMB: this.loadedModels.length > 0 ? (entry?.q4SizeMB ?? 0) : 0,
      latencyMs: Math.round(rttMean * 10) / 10,
      jitterMs: Math.round(jitter * 10) / 10,
      battery: this.battery,
      loadedModels: this.loadedModels,
      supportedModels: this.supportedModels(),
      runtimeReady: this.loadedModels.length > 0,
    });
  }

  private send(message: unknown): void {
    if (this.socket?.readyState !== WebSocket.OPEN) return;
    this.socket.send(JSON.stringify(message));
  }
}
