import { useEffect, useMemo, useState } from "react";
import { useCoordinator } from "./lib/useCoordinator.js";
import { Contributor, type ContributorStatus } from "./lib/contributor.js";
import { ChatPanel } from "./components/ChatPanel.js";
import { DeviceResourcesPanel } from "./components/DeviceResourcesPanel.js";
import { ContributePanel } from "./components/ContributePanel.js";
import { NetworkPanel } from "./components/NetworkPanel.js";

type Tab = "chat" | "resources" | "contribute" | "network";

export function App() {
  const [tab, setTab] = useState<Tab>("chat");
  const coordinator = useCoordinator("");
  const [contributorStatus, setContributorStatus] = useState<ContributorStatus | null>(null);

  const contributor = useMemo(
    () => new Contributor("", (status) => setContributorStatus({ ...status })),
    []
  );

  useEffect(() => {
    void contributor.probe().then(() => {
      void contributor.start(coordinator.state.modelId || "qwen3-14b");
    });
    return () => contributor.stop();
  }, [contributor, coordinator.state.modelId]);

  const { state } = coordinator;
  const onlineNodes = state.nodes.filter((n) => n.online).length;

  return (
    <div className="app-container">
      {/* Sleek Topbar */}
      <header className="topbar">
        <div className="brand">
          <div className="logo-icon">🌐</div>
          <div className="brand-text">
            <strong>Community AI</strong>
            <span className="brand-tag">True P2P Decentralized Mesh</span>
          </div>
        </div>

        <div className="topbar-right">
          <div className="cluster-status-pill">
            <span className="dot online" />
            <span>{onlineNodes} P2P Node{onlineNodes === 1 ? "" : "s"} Online</span>
            <span className="peer-badge">{state.peerId}</span>
          </div>
        </div>
      </header>

      {/* Modern Tab Navigation */}
      <nav className="tab-navigation">
        <button
          className={`tab-btn ${tab === "chat" ? "active" : ""}`}
          onClick={() => setTab("chat")}
        >
          💬 AI Chat
        </button>
        <button
          className={`tab-btn ${tab === "resources" ? "active" : ""}`}
          onClick={() => setTab("resources")}
        >
          📊 Device Resources
        </button>
        <button
          className={`tab-btn ${tab === "contribute" ? "active" : ""}`}
          onClick={() => setTab("contribute")}
        >
          ⚡ Worker Controls
          {contributorStatus?.enabled ? <span className="badge-active">ON</span> : null}
        </button>
        <button
          className={`tab-btn ${tab === "network" ? "active" : ""}`}
          onClick={() => setTab("network")}
        >
          🌐 Cluster Network <span className="tab-count">{onlineNodes}</span>
        </button>
      </nav>

      {/* Main Content View */}
      <main className="main-content">
        {tab === "chat" && <ChatPanel coordinator={coordinator} />}
        {tab === "resources" && (
          <DeviceResourcesPanel coordinator={coordinator} contributorStatus={contributorStatus} />
        )}
        {tab === "contribute" && (
          <ContributePanel
            contributor={contributor}
            status={contributorStatus}
            defaultModelId={state.modelId}
          />
        )}
        {tab === "network" && <NetworkPanel state={state} />}
      </main>
    </div>
  );
}
