import { useEffect, useMemo, useRef, useState } from "react";
import {
  type ChatMessage,
  type JobRequest,
  type PipelinePlan,
  type TaskView,
  MODEL_CATALOG,
} from "@community-ai/protocol";
import type { useCoordinator } from "../lib/useCoordinator.js";

type Coordinator = ReturnType<typeof useCoordinator>;

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
        {pipeline.members.map((member, i) => (
          <div className="pipeline-node" key={member.nodeId}>
            <div className="node-rank">{i === 0 ? "HEAD NODE" : `STAGE ${i}`}</div>
            <div className="node-name">{member.label}</div>
            <div className="node-meter">
              <span style={{ width: `${Math.round(member.share * 100)}%` }} />
            </div>
            <div className="node-meta">
              {Math.round(member.share * 100)}% layers ({member.assignedMB} MB)
            </div>
          </div>
        ))}
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

export function ChatPanel({ coordinator }: { coordinator: Coordinator }) {
  const { state, submit, cancel } = coordinator;
  const [prompt, setPrompt] = useState("");
  const [history, setHistory] = useState<ChatMessage[]>([
    {
      role: "assistant",
      content:
        "Hello! I am the Community AI flagship model (Qwen2.5 7B Instruct) running across a distributed pool of volunteer devices. Ask me anything!",
    },
  ]);
  const [maxTokens, setMaxTokens] = useState(256);
  const [temperature, setTemperature] = useState(0.7);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  const live = state.live;
  const isGenerating =
    live !== null &&
    live.error === null &&
    (live.view === null || live.view.status === "running" || live.view.status === "reducing");

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
  }, [history, streamedText]);

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
    if (!prompt.trim() || isGenerating) return;

    const userMsg: ChatMessage = { role: "user", content: prompt.trim() };
    const newHistory = [...history, userMsg];
    setHistory(newHistory);
    setPrompt("");

    const req: JobRequest = {
      kind: "chat",
      modelId: state.modelId,
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
          <span className="dot online" />
          <strong>Qwen2.5 7B Instruct</strong>
          <span className="badge-tag">Flagship Default</span>
        </div>
        <div className="nodes-info">
          <span>{onlineNodes} compute node(s) pooled</span>
          <button className="btn-clear" onClick={clearChat} title="Clear conversation">
            Clear Chat
          </button>
        </div>
      </div>

      {/* Main Messages View */}
      <div className="messages-stream">
        {history.map((msg, index) => (
          <div key={index} className={`message-bubble ${msg.role}`}>
            <div className="avatar">{msg.role === "user" ? "👤" : "🤖"}</div>
            <div className="message-content">
              <div className="message-author">
                {msg.role === "user" ? "You" : "Community AI Cluster"}
              </div>
              <div className="message-text">{msg.content}</div>
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

        {/* Live Streaming Assistant Output */}
        {isGenerating && live ? (
          <div className="message-bubble assistant streaming">
            <div className="avatar">🤖</div>
            <div className="message-content">
              <div className="message-author">
                Community AI Cluster <span className="streaming-dot" />
              </div>
              <div className="message-text">
                {streamedText || "Generating tokens across cluster..."}
              </div>
            </div>
          </div>
        ) : null}

        {/* Distributed Pipeline Visualizer Card if active */}
        {live?.plan?.pipeline && live.plan.pipeline.members.length > 1 ? (
          <PipelineCard
            pipeline={live.plan.pipeline}
            tasks={live.plan.tasks}
          />
        ) : null}

        <div ref={messagesEndRef} />
      </div>

      {/* Chat Input Bar */}
      <form className="chat-input-box" onSubmit={handleSend}>
        <input
          type="text"
          className="chat-input"
          placeholder="Ask the distributed cluster anything (e.g. 'Explain quantum computing in simple terms')..."
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          disabled={isGenerating}
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
          <button type="submit" className="btn-send" disabled={!prompt.trim()}>
            Send ➔
          </button>
        )}
      </form>
    </div>
  );
}
