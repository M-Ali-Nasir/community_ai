import { requireModel, type TaskSpec } from "@community-ai/protocol";
import type { ResourceGovernor } from "./agents/governor.js";
import type { AiRuntime } from "./runtime/types.js";

/**
 * Worker Scheduler.
 *
 * Local admission control. The coordinator offers work; this decides whether
 * the machine can honestly take it right now, and is the only place allowed to
 * say no on the owner's behalf. It sits between the network layer and the AI
 * Runtime so neither knows about the other.
 */

export interface RunningTask {
  spec: TaskSpec;
  controller: AbortController;
  startedAtMs: number;
}

export interface SchedulerCallbacks {
  onAccepted(spec: TaskSpec): void;
  onRejected(spec: TaskSpec, reason: string): void;
  /** The task was refused because the model is cold; warming has started. */
  onWarmupNeeded(modelId: string): void;
  /**
   * Last step before execution, after the task has been accepted. A pipeline
   * head brings its members up here. Runs inside the task's own error handling
   * so a failed pipeline fails one task rather than the whole worker.
   */
  onPrepareTask?(spec: TaskSpec): Promise<void>;
  onToken(spec: TaskSpec, token: string): void;
  onCompleted(spec: TaskSpec, output: string, metrics: Awaited<ReturnType<AiRuntime["generate"]>>["metrics"]): void;
  onFailed(spec: TaskSpec, error: string): void;
}

export class WorkerScheduler {
  private running = new Map<string, RunningTask>();

  constructor(
    private readonly runtime: AiRuntime,
    private readonly governor: ResourceGovernor,
    private readonly callbacks: SchedulerCallbacks
  ) {}

  get activeTasks(): number {
    return this.running.size;
  }

  /** Reason the task cannot be taken, or null when it can. */
  private admissionError(spec: TaskSpec, maxConcurrent: number, capacity: number): string | null {
    if (this.governor.isManuallyPaused) return "contribution paused by the owner";
    if (capacity <= 0) return "resource governor has capacity at zero";
    if (this.running.size >= Math.max(1, maxConcurrent)) {
      return `already running ${this.running.size} task(s)`;
    }
    if (this.runtime.unavailableReason) return this.runtime.unavailableReason;
    try {
      requireModel(spec.modelId);
    } catch {
      return `unknown model ${spec.modelId}`;
    }
    const supported = this.runtime.supportedModels();
    if (supported.length > 0 && !supported.includes(spec.modelId)) {
      return `this node does not serve ${spec.modelId}`;
    }
    if (!this.runtime.loadedModels().includes(spec.modelId)) {
      // Accepting would mean holding the task through a multi-GB download,
      // which is indistinguishable from a hang at the other end. Refuse, start
      // warming, and let the coordinator place it on a node that is ready.
      return `${spec.modelId} is not loaded yet, warming up now`;
    }
    return null;
  }

  async accept(spec: TaskSpec, maxConcurrent: number, capacity: number): Promise<void> {
    const refusal = this.admissionError(spec, maxConcurrent, capacity);
    if (refusal) {
      this.callbacks.onRejected(spec, refusal);
      if (!this.runtime.loadedModels().includes(spec.modelId) && !this.runtime.unavailableReason) {
        this.callbacks.onWarmupNeeded(spec.modelId);
      }
      return;
    }

    const controller = new AbortController();
    this.running.set(spec.taskId, { spec, controller, startedAtMs: Date.now() });
    this.callbacks.onAccepted(spec);
    this.governor.markContributing(true);

    try {
      await this.callbacks.onPrepareTask?.(spec);
      const result = await this.runtime.generate(
        {
          modelId: spec.modelId,
          messages: spec.messages,
          maxTokens: spec.maxTokens,
          temperature: spec.temperature,
          signal: controller.signal,
        },
        (token) => this.callbacks.onToken(spec, token)
      );
      this.running.delete(spec.taskId);
      this.callbacks.onCompleted(spec, result.text, result.metrics);
    } catch (err) {
      this.running.delete(spec.taskId);
      if (controller.signal.aborted) {
        this.callbacks.onFailed(spec, "cancelled");
        return;
      }
      this.callbacks.onFailed(spec, err instanceof Error ? err.message : String(err));
    } finally {
      this.governor.markContributing(this.running.size > 0);
    }
  }

  cancel(taskId: string): void {
    const task = this.running.get(taskId);
    if (!task) return;
    task.controller.abort();
    this.running.delete(taskId);
  }

  /** Owner pressed pause: drop everything in flight immediately. */
  cancelAll(reason: string): void {
    for (const [taskId, task] of this.running) {
      task.controller.abort();
      this.callbacks.onFailed(task.spec, reason);
      this.running.delete(taskId);
    }
  }
}
