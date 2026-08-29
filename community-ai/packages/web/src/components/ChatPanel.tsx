import { useEffect, useMemo, useRef, useState } from "react";
import {
  type ChatMessage,
  type JobRequest,
  type PipelinePlan,
  type TaskView,
} from "@community-ai/protocol";
import type { useCoordinator } from "../lib/useCoordinator.js";
import type { ContributorStatus } from "../lib/contributor.js";

type Coordinator = ReturnType<typeof useCoordinator>;

function PipelineCard({
  pipeline,
  tasks,
  myPeerId,
}: {
  pipeline: PipelinePlan;
  tasks: TaskView[];
  myPeerId?: string;
}): JSX.Element {
  const measured = tasks.find((t) => t.metrics)?.metrics ?? null;
  const gain = pipeline.pooledMemoryMB / Math.max(pipeline.bestSingleMemoryMB, 1);

  return (
    <div className="pipeline-card">
      <div className="pipeline-header">
        <div className="pipeline-icon">⚡</div>
        <div>
          <h3 style={{ margin: 0, fontSize: 15 }}>Distributed Layer Pipeline</h3>
          <p className="sub" style={{ margin: "2px 0 0", fontSize: 12 }}>
            {pipeline.reason}
          </p>
        </div>
      </div>

      <div className="pipeline-nodes">
        {pipeline.members.map((member, i) => {
          const isMe = member.nodeId === myPeerId;
          const anonymizedLabel = isMe
            ? "Your Node (Active)"
            : i === 0
            ? "Head Cluster Peer"
            : `Anonymous Peer #${i + 1}`;

          return (
            <div className="pipeline-node" key={member.nodeId}>
              <div className="node-rank">{i === 0 ? "HEAD NODE" : `STAGE ${i}`}</div>
              <div className="node-name">{anonymizedLabel}</div>
              <div className="node-meter">
                <span style={{ width: `${Math.round(member.share * 100)}%` }} />
              </div>
              <div className="node-meta">
                {Math.round(member.share * 100)}% layers ({member.assignedMB} MB)
              </div>
            </div>
          );
        })}
      </div>

      <div className="pipeline-metrics">
        <div className="p-metric">
          <strong>{(pipeline.pooledMemoryMB / 1024).toFixed(1)} GB</strong>
          <span>Pooled ({gain.toFixed(1)}× single node)</span>
        </div>
        <div className="p-metric">
          <strong>{pipeline.estimatedHopMs.toFixed(1)} ms</strong>
          <span>Hop Latency floor</span>
        </div>
        <div className="p-metric">
          <strong>{measured ? `${measured.tokensPerSecond.toFixed(1)} tok/s` : "—"}</strong>
          <span>Speed (Ceiling: {pipeline.latencyCeilingTokensPerSec.toFixed(1)} tok/s)</span>
        </div>
      </div>
    </div>
  );
}

export function ChatPanel({
  coordinator,
  contributorStatus,
}: {
  coordinator: Coordinator;
  contributorStatus?: ContributorStatus | null;
}) {
  const { state, submit, cancel } = coordinator;
  const [prompt, setPrompt] = useState("");
  const [history, setHistory] = useState<ChatMessage[]>([
    {
      role: "assistant",
      content:
        "Hello! I am the Community AI flagship model (Qwen3 14B Instruct) running across a distributed pool of volunteer devices. Ask me anything!",
    },
  ]);
  const [maxTokens, setMaxTokens] = useState(256);
  const [temperature, setTemperature] = useState(0.7);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  const live = state.live;
  const isGenerating =
    live !== null &&
    (live.view === null || live.view.status === "running" || live.view.status === "reducing") &&
    live.stage !== "completed";

  const isModelReady =
    !contributorStatus ||
    contributorStatus.modelPhase === "ready" ||
    contributorStatus.modelProgress >= 1.0;
  const prepProgress = Math.round((contributorStatus?.modelProgress ?? 0) * 100);

  const onlineNodes = state.nodes.filter(
    (n) => n.online && n.kind !== "client" && n.governor.capacity > 0
  ).length;

  const streamedText = useMemo(() => {
    if (!live) return "";
    return Object.values(live.streams)
      .map((s) => s.text)
      .join("");
  }, [live?.streams]);

  const scrollToBottom = () => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  };

  useEffect(() => {
    scrollToBottom();
  }, [history, streamedText, live?.statusText]);

  // When response completes, record it into conversational history
  useEffect(() => {
    if (live && live.view?.status === "completed" && live.view.output) {
      setHistory((prev) => {
        const last = prev[prev.length - 1];
        if (last && last.role === "assistant" && last.content === live.view?.output) {
          return prev;
        }
        return [...prev, { role: "assistant", content: live.view!.output }];
      });
    }
  }, [live?.view?.status, live?.view?.output]);

  const handleSend = (e?: React.FormEvent) => {
    if (e) e.preventDefault();
    if (!prompt.trim() || isGenerating || !isModelReady) return;

    const userMsg: ChatMessage = { role: "user", content: prompt.trim() };
    const newHistory = [...history, userMsg];
    setHistory(newHistory);
    setPrompt("");

    const req: JobRequest = {
      kind: "chat",
      modelId: state.modelId || "qwen3-14b",
      policy: "adaptive",
      messages: newHistory,
      items: [],
      itemInstruction: "",
      reduceInstruction: "",
      maxTokens,
      temperature,
    };

    submit(req);
  };

  const clearChat = () => {
    setHistory([
      {
        role: "assistant",
        content: "Chat cleared. What would you like to explore next?",
      },
    ]);
  };

  const copyToClipboard = (text: string) => {
    navigator.clipboard.writeText(text);
  };

  return (
    <div className="chat-layout">
      {/* Top Model Info Bar */}
      <div className="chat-topbar">
        <div className="model-badge">
          <span className={`dot ${isModelReady ? "online" : "busy"}`} />
          <strong>Qwen3 14B Instruct</strong>
          <span className="badge-tag">
            {isModelReady ? "Shard: Ready" : `Preparing Shard ${prepProgress}%`}
          </span>
        </div>
        <div className="nodes-info">
          <span>{onlineNodes} compute node(s) pooled</span>
          <button className="btn-clear" onClick={clearChat} title="Clear conversation">
            Clear Chat
          </button>
        </div>
      </div>

      {/* Model Shard Preparation Progress Banner (if still loading) */}
      {!isModelReady ? (
        <div className="shard-prep-banner">
          <div className="shard-prep-header">
            <span className="shard-prep-title">
              <span className="status-step-spinner" style={{ width: 12, height: 12, display: "inline-block" }} />
              <strong>Preparing Local Qwen3-14B Shard</strong> ({prepProgress}%)
            </span>
            <span className="shard-prep-sub">
              {contributorStatus?.lastEvent || "Downloading weights & allocating local tensors..."}
            </span>
          </div>
          <div className="progress-bar" style={{ height: 4, marginTop: 6 }}>
            <div className="progress-fill blue" style={{ width: `${prepProgress}%` }} />
          </div>
        </div>
      ) : null}

      {/* Main Messages View */}
      <div className="messages-stream">
        {history.map((msg, index) => (
          <div key={index} className={`message-bubble ${msg.role}`}>
            <div className="avatar">{msg.role === "user" ? "👤" : "🤖"}</div>
            <div className="message-content">
              <div className="message-author">
                {msg.role === "user" ? "You" : "Community AI Cluster"}
              </div>
              <div className="message-text" style={{ whiteSpace: "pre-wrap" }}>
                {msg.content}
              </div>
              {msg.role === "assistant" ? (
                <button
                  className="btn-copy"
                  onClick={() => copyToClipboard(msg.content)}
                  title="Copy text"
                >
                  Copy
                </button>
              ) : null}
            </div>
          </div>
        ))}

        {/* Live Streaming Assistant Output with Status Progression */}
        {isGenerating && live ? (
          <div className="message-bubble assistant streaming">
            <div className="avatar">🤖</div>
            <div className="message-content">
              <div className="message-author">
                Community AI Cluster <span className="streaming-dot" />
              </div>
              
              {streamedText ? (
                <div className="message-text" style={{ whiteSpace: "pre-wrap" }}>
                  {streamedText}
                  <span className="streaming-cursor">▋</span>
                </div>
              ) : (
                <div className="generation-status-box">
                  <div className="status-step active">
                    <div className="status-step-spinner" />
                    <span>
                      {live.statusText || "Initializing distributed neural network"}
                      <span className="blinking-dots">
                        <span>.</span><span>.</span><span>.</span>
                      </span>
                    </span>
                  </div>
                </div>
              )}
            </div>
          </div>
        ) : null}

        {/* Distributed Pipeline Visualizer Card if active */}
        {live?.plan?.pipeline && live.plan.pipeline.members.length > 1 ? (
          <PipelineCard
            pipeline={live.plan.pipeline}
            tasks={live.plan.tasks}
            myPeerId={state.peerId}
          />
        ) : null}

        <div ref={messagesEndRef} />
      </div>

      {/* Chat Input Bar */}
      <form className="chat-input-box" onSubmit={handleSend}>
        <input
          type="text"
          className="chat-input"
          placeholder={
            !isModelReady
              ? `⏳ Model shard is preparing on your device (${prepProgress}%)... Prompting enabled once ready.`
              : "Ask the distributed cluster anything (e.g. 'Explain quantum computing in simple terms')..."
          }
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          disabled={isGenerating || !isModelReady}
          autoFocus
        />
        {isGenerating ? (
          <button
            type="button"
            className="btn-send cancel"
            onClick={() => {
              if (live) cancel(live.jobId);
            }}
          >
            Stop
          </button>
        ) : (
          <button
            type="submit"
            className="btn-send"
            disabled={!prompt.trim() || !isModelReady}
          >
            Send ➔
          </button>
        )}
      </form>
    </div>
  );
}
