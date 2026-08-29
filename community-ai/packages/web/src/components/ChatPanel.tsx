import { useEffect, useMemo, useRef, useState } from "react";
import {
  type ChatMessage,
  type JobRequest,
} from "@community-ai/protocol";
import type { useCoordinator } from "../lib/useCoordinator.js";
import type { ContributorStatus } from "../lib/contributor.js";

type Coordinator = ReturnType<typeof useCoordinator>;

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
        "Hello! I am Community AI. Ask me anything, request code, or explore ideas!",
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

  const isModelReady = Boolean(
    contributorStatus &&
    (contributorStatus.modelPhase === "ready" || contributorStatus.modelProgress >= 1.0)
  );
  const prepProgress = Math.min(100, Math.max(10, Math.round((contributorStatus?.modelProgress ?? 0.1) * 100)));

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
          <strong>Community AI</strong>
          <span className="badge-tag">
            {isModelReady ? "Ready" : `Preparing Shards ${prepProgress}%`}
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
              <strong>Preparing Local Community AI Shards</strong> ({prepProgress}%)
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

        <div ref={messagesEndRef} />
      </div>

      {/* Chat Input Bar */}
      <form className="chat-input-box" onSubmit={handleSend}>
        <input
          type="text"
          className="chat-input"
          placeholder={
            !isModelReady
              ? `⏳ System is getting ready (${prepProgress}%)... Prompting enabled once ready.`
              : "Ask Community AI anything (e.g. 'Tell me a joke', 'Write a Python function')..."
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
