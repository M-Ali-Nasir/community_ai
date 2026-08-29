import { EventEmitter } from "node:events";
import {
  type ChatMessage,
  type ClusterPlan,
  type CoordinatorToWorker,
  type JobRequest,
  type JobView,
  type NodeView,
  type PipelinePlan,
  type TaskMetrics,
  type TaskSpec,
  type TaskView,
  newId,
  requireModel,
} from "@community-ai/protocol";
import { analyzeWorkload } from "./analyzer.js";
import { config } from "./config.js";
import { planPipeline } from "./pipeline.js";
import {
  eligibleNodes,
  explainIneligibility,
  formCluster,
  pickReduceNode,
  warmableNodes,
} from "./scheduler.js";
import type { DeviceRegistry } from "./registry.js";

/**
 * Job Manager.
 *
 * Owns the lifecycle of a job: analyse, form a cluster, dispatch tasks, stream
 * tokens, survive nodes disappearing mid-flight, then reduce and finish.
 *
 * Initial allocation follows each node's capability share. After that, whichever
 * node finishes first steals from the longest remaining queue, so a node that
 * turned out slower than its profile suggested cannot hold up the whole job.
 */

interface ManagedTask {
  taskId: string;
  jobId: string;
  index: number;
  phase: "map" | "reduce" | "chat";
  messages: ChatMessage[];
  status: TaskView["status"];
  nodeId: string | null;
  nodeLabel: string | null;
  attempts: number;
  output: string;
  metrics: TaskMetrics | null;
  error: string | null;
  assignedAtMs: number | null;
  triedNodes: Set<string>;
}

interface ManagedJob {
  jobId: string;
  request: JobRequest;
  status: JobView["status"];
  plan: ClusterPlan | null;
  tasks: Map<string, ManagedTask>;
  /** taskIds still waiting for a node, keyed by their preferred node. */
  queues: Map<string, string[]>;
  output: string;
  error: string | null;
  startedAtMs: number;
  finishedAtMs: number | null;
  totalTokens: number;
  reduceDispatched: boolean;
}

export type JobEvents = {
  planned: [jobId: string, plan: ClusterPlan];
  token: [jobId: string, taskId: string, nodeId: string, token: string];
  task: [jobId: string, task: TaskView];
  completed: [job: JobView];
  failed: [jobId: string, error: string];
};

type SendToWorker = (nodeId: string, message: CoordinatorToWorker) => boolean;

export class JobManager extends EventEmitter {
  private jobs = new Map<string, ManagedJob>();
  private roundRobinCursor = 0;
  private completedCount = 0;
  private tokenCount = 0;

  constructor(
    private readonly registry: DeviceRegistry,
    private readonly send: SendToWorker
  ) {
    super();
  }

  get stats(): { jobsCompleted: number; tokensGenerated: number } {
    return { jobsCompleted: this.completedCount, tokensGenerated: this.tokenCount };
  }

  list(limit = 20): JobView[] {
    return [...this.jobs.values()]
      .sort((a, b) => b.startedAtMs - a.startedAtMs)
      .slice(0, limit)
      .map((job) => this.toView(job));
  }

  submit(jobId: string, request: JobRequest): JobView {
    const model = requireModel(request.modelId);
    const job: ManagedJob = {
      jobId,
      request,
      status: "planning",
      plan: null,
      tasks: new Map(),
      queues: new Map(),
      output: "",
      error: null,
      startedAtMs: Date.now(),
      finishedAtMs: null,
      totalTokens: 0,
      reduceDispatched: false,
    };
    this.jobs.set(jobId, job);

    const allNodes = this.registry.toViews();
    const eligible = eligibleNodes(allNodes, request.modelId);
    const analysis = analyzeWorkload(request, eligible, allNodes);

    // A layer-split job bypasses ordinary cluster formation: the members are
    // chosen by the pipeline planner on memory and link quality, and only the
    // head receives a task. The members are already serving over RPC.
    let pipeline: PipelinePlan | null = null;
    let cluster: { nodeIds: string[]; shares: Record<string, number>; reason: string };

    if (analysis.strategy === "model-parallel") {
      const attempt = planPipeline(allNodes, request.modelId);
      if (!attempt.plan) return this.fail(job, attempt.reason);
      pipeline = attempt.plan;
      cluster = {
        nodeIds: [pipeline.headNodeId],
        shares: Object.fromEntries(pipeline.members.map((m) => [m.nodeId, m.share])),
        reason: pipeline.reason,
      };
    } else {
      cluster = formCluster(
        analysis,
        eligible,
        request.modelId,
        request.policy,
        this.roundRobinCursor
      );
      this.roundRobinCursor += 1;
    }

    if (cluster.nodeIds.length === 0) {
      // Ask anyone who could serve this model to start loading it, so the next
      // attempt succeeds instead of a task stalling on a cold multi-GB download.
      const warming = this.warmNodes(allNodes, request.modelId);
      const reasons = explainIneligibility(allNodes, request.modelId);
      const detail = reasons.length > 0 ? ` Nodes: ${reasons.join(" | ")}.` : "";
      // When the model is simply too big, why the *pipeline* could not be built
      // is the useful message; per-node memory shortfalls are the symptom.
      const pipelineNote = analysis.rejected
        .filter((r) => r.strategy === "model-parallel")
        .map((r) => ` Layer-splitting: ${r.reason}`)
        .join("");
      const warmingNote =
        warming.length > 0
          ? ` Asked ${warming.length} node(s) to load ${model.displayName} now — try again once they report ready.`
          : "";
      return this.fail(
        job,
        allNodes.length === 0
          ? `No nodes are connected. Start a desktop worker, or enable contribution from a browser.`
          : `No node can currently run ${model.displayName}.${detail}${pipelineNote}${warmingNote}`
      );
    }

    this.buildTasks(job, analysis, cluster, allNodes);

    const plan: ClusterPlan = {
      jobId,
      analysis,
      policy: request.policy,
      modelId: request.modelId,
      nodeIds: cluster.nodeIds,
      shares: cluster.shares,
      tasks: [...job.tasks.values()].map(toTaskView),
      formedAtMs: Date.now(),
      reason: cluster.reason,
      pipeline,
    };
    job.plan = plan;
    job.status = "running";
    this.emit("planned", jobId, plan);

    this.pump(job);
    return this.toView(job);
  }

  /** Tell capable-but-cold nodes to load a model. Returns who was asked. */
  warmNodes(nodes: NodeView[], modelId: string): string[] {
    const targets = warmableNodes(nodes, modelId);
    const asked: string[] = [];
    for (const node of targets) {
      if (this.send(node.nodeId, { type: "model:prepare", modelId })) asked.push(node.nodeId);
    }
    return asked;
  }

  private buildTasks(
    job: ManagedJob,
    analysis: ReturnType<typeof analyzeWorkload>,
    cluster: { nodeIds: string[]; shares: Record<string, number> },
    allNodes: NodeView[]
  ): void {
    const labels = new Map(allNodes.map((n) => [n.nodeId, n.label]));
    const { request } = job;

    if (analysis.strategy !== "task-parallel" || request.kind === "chat") {
      const nodeId = cluster.nodeIds[0] ?? null;
      const task: ManagedTask = {
        taskId: newId("task"),
        jobId: job.jobId,
        index: 0,
        phase: request.kind === "chat" ? "chat" : "map",
        messages:
          request.kind === "chat"
            ? request.messages
            : buildItemMessages(request, request.items[0] ?? ""),
        status: "planned",
        nodeId: null,
        nodeLabel: null,
        attempts: 0,
        output: "",
        metrics: null,
        error: null,
        assignedAtMs: null,
        triedNodes: new Set(),
      };
      job.tasks.set(task.taskId, task);
      if (nodeId) job.queues.set(nodeId, [task.taskId]);
      void labels;
      return;
    }

    // Task-parallel: one task per item, allocated by capability share.
    const items = request.items;
    const quotas = allocateByShare(cluster.nodeIds, cluster.shares, items.length);
    let cursor = 0;
    for (const nodeId of cluster.nodeIds) {
      const quota = quotas[nodeId] ?? 0;
      const queue: string[] = [];
      for (let i = 0; i < quota && cursor < items.length; i += 1, cursor += 1) {
        const task: ManagedTask = {
          taskId: newId("task"),
          jobId: job.jobId,
          index: cursor,
          phase: "map",
          messages: buildItemMessages(request, items[cursor] ?? ""),
          status: "planned",
          nodeId: null,
          nodeLabel: null,
          attempts: 0,
          output: "",
          metrics: null,
          error: null,
          assignedAtMs: null,
          triedNodes: new Set(),
        };
        job.tasks.set(task.taskId, task);
        queue.push(task.taskId);
      }
      job.queues.set(nodeId, queue);
    }
    // Largest-remainder rounding can leave a tail; give it to the first node.
    const first = cluster.nodeIds[0];
    if (first && cursor < items.length) {
      const queue = job.queues.get(first) ?? [];
      for (; cursor < items.length; cursor += 1) {
        const task: ManagedTask = {
          taskId: newId("task"),
          jobId: job.jobId,
          index: cursor,
          phase: "map",
          messages: buildItemMessages(request, items[cursor] ?? ""),
          status: "planned",
          nodeId: null,
          nodeLabel: null,
          attempts: 0,
          output: "",
          metrics: null,
          error: null,
          assignedAtMs: null,
          triedNodes: new Set(),
        };
        job.tasks.set(task.taskId, task);
        queue.push(task.taskId);
      }
      job.queues.set(first, queue);
    }
  }

  /** Push as many queued tasks onto free nodes as their governors allow. */
  private pump(job: ManagedJob): void {
    if (job.status !== "running" && job.status !== "reducing") return;

    for (const [nodeId, queue] of job.queues) {
      if (queue.length === 0) continue;
      const node = this.registry.get(nodeId);
      if (!node) continue;
      const slots = Math.max(1, node.governor.maxConcurrentTasks) - node.activeTasks;
      for (let i = 0; i < slots && queue.length > 0; i += 1) {
        const taskId = queue.shift();
        if (!taskId) break;
        this.dispatch(job, taskId, nodeId);
      }
    }

    // Work stealing: an idle node pulls from the longest remaining queue.
    let guard = 0;
    for (;;) {
      guard += 1;
      if (guard > 200) break;
      const idle = this.idleClusterNodes(job);
      if (idle.length === 0) break;
      const donor = [...job.queues.entries()]
        .filter(([, q]) => q.length > 1)
        .sort((a, b) => b[1].length - a[1].length)[0];
      if (!donor) break;
      const thief = idle[0];
      if (!thief) break;
      const taskId = donor[1].pop();
      if (!taskId) break;
      this.dispatch(job, taskId, thief);
    }

    this.maybeFinish(job);
  }

  private idleClusterNodes(job: ManagedJob): string[] {
    const out: string[] = [];
    for (const nodeId of job.queues.keys()) {
      const queue = job.queues.get(nodeId) ?? [];
      if (queue.length > 0) continue;
      const node = this.registry.get(nodeId);
      if (!node) continue;
      if (node.activeTasks < Math.max(1, node.governor.maxConcurrentTasks)) out.push(nodeId);
    }
    return out;
  }

  private dispatch(job: ManagedJob, taskId: string, nodeId: string): void {
    const task = job.tasks.get(taskId);
    if (!task) return;
    const node = this.registry.get(nodeId);
    if (!node) {
      this.requeue(job, task, "node disappeared before dispatch");
      return;
    }

    const spec: TaskSpec = {
      jobId: job.jobId,
      taskId: task.taskId,
      modelId: job.request.modelId,
      messages: task.messages,
      maxTokens: job.request.maxTokens,
      temperature: job.request.temperature,
      index: task.index,
      phase: task.phase,
      // Only the head is told about the pipeline; the members are already
      // serving their devices and do not need to know a job exists.
      pipeline:
        job.plan?.pipeline && job.plan.pipeline.headNodeId === nodeId
          ? job.plan.pipeline
          : null,
    };

    const ok = this.send(nodeId, { type: "task:assign", spec });
    if (!ok) {
      this.requeue(job, task, "worker socket unavailable");
      return;
    }

    task.status = "assigned";
    task.nodeId = nodeId;
    task.nodeLabel = node.profile.label;
    task.attempts += 1;
    task.assignedAtMs = Date.now();
    task.triedNodes.add(nodeId);
    node.activeTasks += 1;
    this.emit("task", job.jobId, toTaskView(task));
  }

  /** Send a task back to the pool, preferring a node that has not failed it. */
  private requeue(job: ManagedJob, task: ManagedTask, why: string): void {
    if (task.nodeId) {
      const previous = this.registry.get(task.nodeId);
      if (previous) previous.activeTasks = Math.max(0, previous.activeTasks - 1);
    }
    task.nodeId = null;
    task.nodeLabel = null;
    task.assignedAtMs = null;

    if (task.attempts >= config.maxTaskAttempts) {
      task.status = "failed";
      task.error = `${why} (gave up after ${task.attempts} attempts)`;
      this.emit("task", job.jobId, toTaskView(task));
      this.maybeFinish(job);
      return;
    }

    const views = this.registry.toViews();

    // A pipeline is all-or-nothing: losing one member loses the layers it held,
    // so there is no "retry on another node". The planner has to rebuild the
    // whole chain from whoever is left, which is dynamic model placement in its
    // simplest form.
    if (job.plan?.pipeline) {
      const attempt = planPipeline(views, job.request.modelId);
      if (!attempt.plan) {
        task.status = "failed";
        task.error = `${why}, and the remaining nodes cannot re-form a pipeline: ${attempt.reason}`;
        this.emit("task", job.jobId, toTaskView(task));
        this.maybeFinish(job);
        return;
      }
      job.plan.pipeline = attempt.plan;
      job.plan.nodeIds = [attempt.plan.headNodeId];
      job.plan.shares = Object.fromEntries(
        attempt.plan.members.map((m) => [m.nodeId, m.share])
      );
      job.plan.reason = `Re-formed after ${why}. ${attempt.plan.reason}`;
      task.status = "reassigned";
      task.error = why;
      this.emit("task", job.jobId, toTaskView(task));
      this.emit("planned", job.jobId, job.plan);
      job.queues.clear();
      job.queues.set(attempt.plan.headNodeId, [task.taskId]);
      queueMicrotask(() => this.pump(job));
      return;
    }

    // A busy node is a fine destination here: the task waits in its queue rather
    // than being dropped because everyone happened to be mid-task. A cold node is
    // a last resort — better a slow retry than losing the work entirely.
    const warm = eligibleNodes(views, job.request.modelId, { allowBusy: true });
    const capable =
      warm.length > 0
        ? warm
        : eligibleNodes(views, job.request.modelId, { allowBusy: true, allowCold: true });
    const untried = capable.filter((n) => !task.triedNodes.has(n.nodeId));
    const target = untried[0]?.nodeId ?? capable[0]?.nodeId;

    if (!target) {
      task.status = "failed";
      task.error = `${why} and no other node can take it`;
      this.emit("task", job.jobId, toTaskView(task));
      this.maybeFinish(job);
      return;
    }

    task.status = "reassigned";
    task.error = why;
    this.emit("task", job.jobId, toTaskView(task));
    const queue = job.queues.get(target) ?? [];
    queue.unshift(task.taskId);
    job.queues.set(target, queue);
    queueMicrotask(() => this.pump(job));
  }

  onTaskAccepted(jobId: string, taskId: string): void {
    const job = this.jobs.get(jobId);
    const task = job?.tasks.get(taskId);
    if (!job || !task) return;
    task.status = "running";
    this.emit("task", jobId, toTaskView(task));
  }

  onTaskRejected(jobId: string, taskId: string, nodeId: string, reason: string): void {
    const job = this.jobs.get(jobId);
    const task = job?.tasks.get(taskId);
    if (!job || !task) return;
    void nodeId;
    this.requeue(job, task, `worker declined: ${reason}`);
  }

  onToken(jobId: string, taskId: string, nodeId: string, token: string): void {
    const job = this.jobs.get(jobId);
    const task = job?.tasks.get(taskId);
    if (!job || !task) return;
    task.output += token;
    this.tokenCount += 1;
    this.emit("token", jobId, taskId, nodeId, token);
  }

  onTaskCompleted(
    jobId: string,
    taskId: string,
    nodeId: string,
    output: string,
    metrics: TaskMetrics
  ): void {
    const job = this.jobs.get(jobId);
    const task = job?.tasks.get(taskId);
    if (!job || !task) return;

    const node = this.registry.get(nodeId);
    if (node) node.activeTasks = Math.max(0, node.activeTasks - 1);
    if (metrics.tokensPerSecond > 0) {
      this.registry.recordTaskResult(nodeId, metrics.tokensPerSecond, metrics.ttftMs);
    }

    task.status = "completed";
    task.output = output || task.output;
    task.metrics = metrics;
    task.error = null;
    job.totalTokens += metrics.tokens;
    this.emit("task", jobId, toTaskView(task));
    this.pump(job);
  }

  onTaskFailed(jobId: string, taskId: string, nodeId: string, error: string): void {
    const job = this.jobs.get(jobId);
    const task = job?.tasks.get(taskId);
    if (!job || !task) return;
    this.registry.recordTaskFailure(nodeId);
    this.requeue(job, task, error);
  }

  /** A node vanished. Every task it held goes back into the pool. */
  onNodeLost(nodeId: string): void {
    for (const job of this.jobs.values()) {
      if (job.status !== "running" && job.status !== "reducing") continue;
      const queue = job.queues.get(nodeId);
      if (queue && queue.length > 0) {
        job.queues.delete(nodeId);
        const survivors = [...job.queues.keys()];
        const fallback = survivors[0];
        if (fallback) {
          job.queues.set(fallback, [...(job.queues.get(fallback) ?? []), ...queue]);
        } else {
          for (const taskId of queue) {
            const task = job.tasks.get(taskId);
            if (task) this.requeue(job, task, "every node in the cluster went offline");
          }
        }
      } else {
        job.queues.delete(nodeId);
      }
      for (const task of job.tasks.values()) {
        if (task.nodeId === nodeId && (task.status === "assigned" || task.status === "running")) {
          this.requeue(job, task, "node went offline mid-task");
        }
      }
      this.pump(job);
    }
  }

  private maybeFinish(job: ManagedJob): void {
    if (job.status === "completed" || job.status === "failed" || job.status === "cancelled") return;

    const tasks = [...job.tasks.values()];
    const mapTasks = tasks.filter((t) => t.phase !== "reduce");
    const pending = mapTasks.filter(
      (t) => t.status !== "completed" && t.status !== "failed"
    );
    if (pending.length > 0) return;

    const succeeded = mapTasks.filter((t) => t.status === "completed");
    if (succeeded.length === 0) {
      this.fail(job, "Every task failed. See the task list for per-node errors.");
      return;
    }

    const wantsReduce =
      job.request.kind === "batch" &&
      job.request.reduceInstruction.trim().length > 0 &&
      succeeded.length > 1;

    if (wantsReduce && !job.reduceDispatched) {
      const reduceNode = pickReduceNode(this.registry.toViews(), job.request.modelId);
      if (reduceNode) {
        job.reduceDispatched = true;
        job.status = "reducing";
        const combined = succeeded
          .sort((a, b) => a.index - b.index)
          .map((t, i) => `[${i + 1}] ${t.output.trim()}`)
          .join("\n\n");
        const task: ManagedTask = {
          taskId: newId("task"),
          jobId: job.jobId,
          index: mapTasks.length,
          phase: "reduce",
          messages: [
            {
              role: "user",
              content: `${job.request.reduceInstruction}\n\n${combined}`,
            },
          ],
          status: "planned",
          nodeId: null,
          nodeLabel: null,
          attempts: 0,
          output: "",
          metrics: null,
          error: null,
          assignedAtMs: null,
          triedNodes: new Set(),
        };
        job.tasks.set(task.taskId, task);
        this.dispatch(job, task.taskId, reduceNode.nodeId);
        return;
      }
    }

    const reduceTask = tasks.find((t) => t.phase === "reduce");
    if (reduceTask && reduceTask.status !== "completed" && reduceTask.status !== "failed") return;

    job.status = "completed";
    job.finishedAtMs = Date.now();
    job.output =
      reduceTask?.status === "completed" && reduceTask.output.trim().length > 0
        ? reduceTask.output.trim()
        : succeeded
            .sort((a, b) => a.index - b.index)
            .map((t) => t.output.trim())
            .join("\n\n");
    this.completedCount += 1;
    if (job.plan) job.plan.tasks = [...job.tasks.values()].map(toTaskView);
    this.emit("completed", this.toView(job));
  }

  cancel(jobId: string): void {
    const job = this.jobs.get(jobId);
    if (!job || job.status === "completed" || job.status === "failed") return;
    job.status = "cancelled";
    job.finishedAtMs = Date.now();
    for (const task of job.tasks.values()) {
      if (task.nodeId && (task.status === "assigned" || task.status === "running")) {
        this.send(task.nodeId, { type: "task:cancel", jobId, taskId: task.taskId });
        const node = this.registry.get(task.nodeId);
        if (node) node.activeTasks = Math.max(0, node.activeTasks - 1);
      }
      if (task.status !== "completed") task.status = "cancelled";
    }
    job.queues.clear();
    this.emit("completed", this.toView(job));
  }

  /** Reclaim tasks stuck on an unresponsive node. */
  sweepTimeouts(): void {
    const now = Date.now();
    for (const job of this.jobs.values()) {
      if (job.status !== "running" && job.status !== "reducing") continue;
      for (const task of job.tasks.values()) {
        if (
          (task.status === "assigned" || task.status === "running") &&
          task.assignedAtMs !== null &&
          now - task.assignedAtMs > config.taskTimeoutMs
        ) {
          this.requeue(job, task, "task timed out");
        }
      }
    }
  }

  private fail(job: ManagedJob, error: string): JobView {
    job.status = "failed";
    job.error = error;
    job.finishedAtMs = Date.now();
    this.emit("failed", job.jobId, error);
    return this.toView(job);
  }

  private toView(job: ManagedJob): JobView {
    const plan = job.plan
      ? { ...job.plan, tasks: [...job.tasks.values()].map(toTaskView) }
      : null;
    return {
      jobId: job.jobId,
      status: job.status,
      request: job.request,
      plan,
      output: job.output,
      error: job.error,
      startedAtMs: job.startedAtMs,
      finishedAtMs: job.finishedAtMs,
      wallClockMs: job.finishedAtMs === null ? null : job.finishedAtMs - job.startedAtMs,
      totalTokens: job.totalTokens,
    };
  }
}

function toTaskView(task: ManagedTask): TaskView {
  return {
    taskId: task.taskId,
    nodeId: task.nodeId,
    nodeLabel: task.nodeLabel,
    index: task.index,
    phase: task.phase,
    status: task.status,
    attempts: task.attempts,
    output: task.output,
    metrics: task.metrics,
    error: task.error,
  };
}

function buildItemMessages(request: JobRequest, item: string): ChatMessage[] {
  return [{ role: "user", content: `${request.itemInstruction}\n\n${item}` }];
}

/** Largest-remainder allocation so shares turn into whole task counts. */
function allocateByShare(
  nodeIds: string[],
  shares: Record<string, number>,
  total: number
): Record<string, number> {
  const exact = nodeIds.map((id) => ({ id, value: (shares[id] ?? 0) * total }));
  const out: Record<string, number> = {};
  let assigned = 0;
  for (const { id, value } of exact) {
    const floor = Math.floor(value);
    out[id] = floor;
    assigned += floor;
  }
  const remainders = exact
    .map(({ id, value }) => ({ id, rem: value - Math.floor(value) }))
    .sort((a, b) => b.rem - a.rem);
  let i = 0;
  while (assigned < total && remainders.length > 0) {
    const entry = remainders[i % remainders.length];
    if (entry) {
      out[entry.id] = (out[entry.id] ?? 0) + 1;
      assigned += 1;
    }
    i += 1;
  }
  return out;
}
