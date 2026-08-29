import { useMemo, useState } from "react";
import {
  type ChatMessage,
  type JobRequest,
  type PipelinePlan,
  type SchedulingPolicy,
  type TaskView,
  MODEL_CATALOG,
} from "@community-ai/protocol";
import type { useCoordinator } from "../lib/useCoordinator.js";

type Coordinator = ReturnType<typeof useCoordinator>;

/**
 * Shown only when the model was too large for any single member. The numbers
 * that matter are the two comparisons: pooled memory against the largest
 * single node (what this made possible), and measured throughput against the
 * latency ceiling (what it cost).
 */
function PipelineCard({
  pipeline,
  tasks,
}: {
  pipeline: PipelinePlan;
  tasks: TaskView[];
}): JSX.Element {
  const measured = tasks.find((t) => t.metrics)?.metrics ?? null;
  const gain = pipeline.pooledMemoryMB / Math.max(pipeline.bestSingleMemoryMB, 1);

  return (
    <div className="card">
      <h2>Model pipeline</h2>
      <p className="sub">{pipeline.reason}</p>

      <div className="pipeline">
        {pipeline.members.map((member, i) => (
          <div className="pipeline-node" key={member.nodeId}>
            <div className="pipeline-rank">
              {i === 0 ? "head" : `hop ${i}`}
            </div>
            <div className="pipeline-label">{member.label}</div>
            <div className="meter">
              <span style={{ width: `${Math.round(member.share * 100)}%` }} />
            </div>
            <div className="mono muted" style={{ fontSize: 11.5 }}>
              {Math.round(member.share * 100)}% of layers · {member.assignedMB} MB
            </div>
          </div>
        ))}
      </div>

      <div className="pipeline-stats">
        <div>
          <strong>{(pipeline.pooledMemoryMB / 1024).toFixed(1)} GB</strong>
          <span>
            pooled, against {(pipeline.bestSingleMemoryMB / 1024).toFixed(1)} GB on the
            largest single node — {gain.toFixed(1)}× what one machine could hold
          </span>
        </div>
        <div>
          <strong>{pipeline.estimatedHopMs.toFixed(1)} ms</strong>
          <span>
            of round-trip per token, across {pipeline.members.length - 1} hop(s)
          </span>
        </div>
        <div>
          <strong>{measured ? `${measured.tokensPerSecond.toFixed(1)} tok/s` : "—"}</strong>
          <span>
            measured, against a latency ceiling of{" "}
            {pipeline.latencyCeilingTokensPerSec.toFixed(1)} tok/s
          </span>
        </div>
      </div>
    </div>
  );
}

const POLICIES: { id: SchedulingPolicy; label: string; blurb: string }[] = [
  { id: "adaptive", label: "Adaptive", blurb: "Throughput and latency, with weak nodes dropped." },
  { id: "best-node", label: "Best node", blurb: "Baseline: one strongest node does everything." },
  { id: "compute-only", label: "Compute only", blurb: "Ranks by speed, ignores the network." },
  { id: "network-aware", label: "Network aware", blurb: "Penalises distant nodes heavily." },
  { id: "resource-aware", label: "Resource aware", blurb: "Ranks by governor-approved free memory." },
  { id: "round-robin", label: "Round robin", blurb: "Control: equal shares regardless of capability." },
];

const SAMPLE_ITEMS = [
  "Peer-to-peer file sharing spread because it turned every downloader into an uploader, so capacity grew with demand rather than against it.",
  "Volunteer computing projects such as SETI@home and Folding@home showed that idle consumer hardware can add up to serious aggregate throughput.",
  "Modern consumer GPUs ship with enough memory to hold quantised language models that would have needed a datacentre a few years ago.",
  "The hard part of distributed inference is not raw compute but the latency and variability of links between homes.",
];

export function ChatPanel({ coordinator }: { coordinator: Coordinator }) {
  const { state, submit, cancel, setPolicy, setModel } = coordinator;
  const [mode, setMode] = useState<"chat" | "batch">("chat");
  const [prompt, setPrompt] = useState("");
  const [history, setHistory] = useState<ChatMessage[]>([]);
  const [items, setItems] = useState(SAMPLE_ITEMS.join("\n"));
  const [itemInstruction, setItemInstruction] = useState(
    "Summarise the following passage in one sentence."
  );
  const [reduceInstruction, setReduceInstruction] = useState(
    "Combine these summaries into a single short paragraph."
  );
  const [maxTokens, setMaxTokens] = useState(220);

  const live = state.live;
  const running =
    live !== null &&
    live.error === null &&
    (live.view === null || live.view.status === "running" || live.view.status === "reducing");

  const eligibleCount = state.nodes.filter(
    (n) => n.online && n.kind !== "client" && n.governor.capacity > 0 && n.profile.runtime.ready
  ).length;

  const itemList = useMemo(
    () => items.split("\n").map((line) => line.trim()).filter(Boolean),
    [items]
  );

  const streamedAnswer = useMemo(() => {
    if (!live) return "";
    if (live.view?.output) return live.view.output;
    const streams = Object.entries(live.streams);
    if (streams.length === 0) return "";
    if (mode === "chat") return streams[0]?.[1].text ?? "";
    return "";
  }, [live, mode]);

  const onSubmit = () => {
    if (running) return;
    const request: JobRequest = {
      kind: mode,
      modelId: state.modelId,
      policy: state.policy,
      messages:
        mode === "chat" ? [...history, { role: "user", content: prompt.trim() }] : [],
      items: mode === "batch" ? itemList : [],
      itemInstruction,
      reduceInstruction: mode === "batch" ? reduceInstruction : "",
      maxTokens,
      temperature: 0.7,
    };
    if (mode === "chat") {
      if (!prompt.trim()) return;
      setHistory((prev) => [...prev, { role: "user", content: prompt.trim() }]);
      setPrompt("");
    } else if (itemList.length === 0) {
      return;
    }
    submit(request);
  };

  return (
    <div className="grid-2">
      <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
        <div className="card">
          <div className="row" style={{ marginBottom: 14 }}>
            <div className="segmented">
              <button className={mode === "chat" ? "active" : ""} onClick={() => setMode("chat")}>
                Chat
              </button>
              <button className={mode === "batch" ? "active" : ""} onClick={() => setMode("batch")}>
                Batch
              </button>
            </div>
            <div className="spacer" />
            <span className="pill">
              {eligibleCount} node{eligibleCount === 1 ? "" : "s"} ready
            </span>
          </div>

          <div className="row">
            <div className="field" style={{ flex: "1 1 190px" }}>
              <label htmlFor="model">Model</label>
              <select
                id="model"
                value={state.modelId}
                onChange={(e) => setModel(e.target.value)}
              >
                {MODEL_CATALOG.map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.displayName} · {m.license}
                  </option>
                ))}
              </select>
            </div>
            <div className="field" style={{ flex: "1 1 190px" }}>
              <label htmlFor="policy">Scheduling policy</label>
              <select
                id="policy"
                value={state.policy}
                onChange={(e) => setPolicy(e.target.value as SchedulingPolicy)}
              >
                {POLICIES.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.label}
                  </option>
                ))}
              </select>
            </div>
            <div className="field" style={{ flex: "0 1 110px" }}>
              <label htmlFor="tokens">Max tokens</label>
              <input
                id="tokens"
                type="number"
                min={32}
                max={2048}
                value={maxTokens}
                onChange={(e) => setMaxTokens(Number(e.target.value) || 220)}
              />
            </div>
          </div>
          <p className="muted" style={{ fontSize: 12.5, margin: "8px 0 0" }}>
            {POLICIES.find((p) => p.id === state.policy)?.blurb}
          </p>
        </div>

        {mode === "chat" ? (
          <div className="card">
            <h2>Conversation</h2>
            <p className="sub">
              A single turn is sequential, so the analyzer keeps it on one node. Switch to Batch to
              see work spread across the network.
            </p>
            <div className="conversation">
              {history.map((turn, i) => (
                <div key={i} className={`turn ${turn.role}`}>
                  <span className="who">{turn.role}</span>
                  <div className="bubble">{turn.content}</div>
                </div>
              ))}
              {streamedAnswer ? (
                <div className="turn assistant">
                  <span className="who">assistant</span>
                  <div className="answer">{streamedAnswer}</div>
                </div>
              ) : null}
            </div>
            <div className="composer" style={{ marginTop: 14 }}>
              <textarea
                value={prompt}
                placeholder="Ask the network something…"
                onChange={(e) => setPrompt(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) onSubmit();
                }}
              />
              <button className="primary" onClick={onSubmit} disabled={running || !prompt.trim()}>
                {running ? "Running…" : "Send"}
              </button>
            </div>
          </div>
        ) : (
          <div className="card">
            <h2>Batch of independent items</h2>
            <p className="sub">
              One line per item. These have no dependency on each other, which is exactly when
              spreading work across devices pays off.
            </p>
            <div className="items-editor">
              <div className="field">
                <label htmlFor="instruction">Applied to every item</label>
                <input
                  id="instruction"
                  type="text"
                  value={itemInstruction}
                  onChange={(e) => setItemInstruction(e.target.value)}
                />
              </div>
              <div className="field">
                <label htmlFor="items">Items ({itemList.length})</label>
                <textarea
                  id="items"
                  value={items}
                  style={{ minHeight: 140 }}
                  onChange={(e) => setItems(e.target.value)}
                />
              </div>
              <div className="field">
                <label htmlFor="reduce">Reduce step (leave empty to skip)</label>
                <input
                  id="reduce"
                  type="text"
                  value={reduceInstruction}
                  onChange={(e) => setReduceInstruction(e.target.value)}
                />
              </div>
              <div className="row">
                <button
                  className="primary"
                  onClick={onSubmit}
                  disabled={running || itemList.length === 0}
                >
                  {running ? "Running…" : `Distribute ${itemList.length} items`}
                </button>
                {running ? (
                  <button className="danger" onClick={() => live && cancel(live.jobId)}>
                    Cancel
                  </button>
                ) : null}
              </div>
            </div>
          </div>
        )}

        {live?.view?.output && mode === "batch" ? (
          <div className="card">
            <h2>Combined result</h2>
            <div className="answer">{live.view.output}</div>
          </div>
        ) : null}
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
        <PlanCard coordinator={coordinator} />
      </div>
    </div>
  );
}

function PlanCard({ coordinator }: { coordinator: Coordinator }) {
  const live = coordinator.state.live;

  if (!live) {
    return (
      <div className="card">
        <h2>Execution plan</h2>
        <p className="sub" style={{ marginBottom: 0 }}>
          Submit something and the Workload Analyzer's decision appears here: which strategy it
          chose, which nodes were selected, what each one produced, and how fast.
        </p>
      </div>
    );
  }

  if (live.error) {
    return (
      <div className="card">
        <h2>Execution plan</h2>
        <div className="notice bad">{live.error}</div>
      </div>
    );
  }

  const plan = live.plan;
  if (!plan) {
    return (
      <div className="card">
        <h2>Execution plan</h2>
        <p className="sub" style={{ marginBottom: 0 }}>Planning…</p>
      </div>
    );
  }

  const { analysis } = plan;
  const view = live.view;

  return (
    <>
      <div className="card">
        <h2>Workload analyzer</h2>
        <div className="plan">
          <div className="strategy">
            <span className="pill good">{analysis.strategy}</span>
            <span className="pill">{analysis.coupling}</span>
            <span className="pill info">{plan.policy}</span>
            {analysis.unitCount > 0 ? (
              <span className="pill">{analysis.unitCount} unit{analysis.unitCount === 1 ? "" : "s"}</span>
            ) : null}
          </div>
          <p style={{ margin: 0, fontSize: 13.5 }}>{analysis.reason}</p>
          {analysis.rejected.map((r) => (
            <p className="rejected" key={r.strategy}>
              <strong>{r.strategy} rejected</strong> — {r.reason}
            </p>
          ))}
        </div>
      </div>

      {plan.pipeline && plan.pipeline.members.length > 1 ? (
        <PipelineCard pipeline={plan.pipeline} tasks={plan.tasks} />
      ) : null}

      <div className="card">
        <h2>Cluster</h2>
        <p className="sub">{plan.reason}</p>
        <div className="tasklist">
          {plan.tasks
            .slice()
            .sort((a, b) => a.index - b.index)
            .map((task) => {
              const streamed = live.streams[task.taskId]?.text ?? "";
              const text = task.output || streamed;
              const tone =
                task.status === "completed"
                  ? "good"
                  : task.status === "failed"
                    ? "bad"
                    : task.status === "reassigned"
                      ? "warn"
                      : "info";
              return (
                <div className="task" key={task.taskId}>
                  <div className="task-head">
                    <span className="pill">#{task.index + 1}</span>
                    <span className="pill info">{task.phase}</span>
                    <strong style={{ fontSize: 13 }}>{task.nodeLabel ?? "unassigned"}</strong>
                    <span className={`pill ${tone}`}>{task.status}</span>
                    {task.attempts > 1 ? (
                      <span className="pill warn">attempt {task.attempts}</span>
                    ) : null}
                    {task.metrics ? (
                      <span className="pill mono">
                        {task.metrics.tokensPerSecond.toFixed(1)} tok/s · {Math.round(task.metrics.ttftMs)} ms first
                      </span>
                    ) : null}
                  </div>
                  {task.error ? <div className="notice bad">{task.error}</div> : null}
                  {text ? <div className="task-body">{text}</div> : null}
                </div>
              );
            })}
        </div>
      </div>

      {view && view.wallClockMs !== null ? (
        <div className="card">
          <h2>Measured</h2>
          <div className="node-grid mono">
            <div>
              <span>Wall clock</span>
              {(view.wallClockMs / 1000).toFixed(2)} s
            </div>
            <div>
              <span>Tokens</span>
              {view.totalTokens}
            </div>
            <div>
              <span>Nodes used</span>
              {new Set(plan.tasks.map((t) => t.nodeId).filter(Boolean)).size}
            </div>
            <div>
              <span>Status</span>
              {view.status}
            </div>
          </div>
        </div>
      ) : null}
    </>
  );
}
