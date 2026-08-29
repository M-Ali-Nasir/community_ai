import { useEffect, useMemo, useState } from "react";
import { useCoordinator } from "./lib/useCoordinator.js";
import { Contributor, type ContributorStatus } from "./lib/contributor.js";
import { ChatPanel } from "./components/ChatPanel.js";
import { DeviceResourcesPanel } from "./components/DeviceResourcesPanel.js";
import { ContributePanel } from "./components/ContributePanel.js";
import { NetworkPanel } from "./components/NetworkPanel.js";
import { NetworkConfigModal } from "./components/NetworkConfigModal.js";

type Tab = "chat" | "resources" | "contribute" | "network";

const TOKEN_KEY = "community-ai:token";

function readToken(): string {
  const fromUrl = new URLSearchParams(window.location.search).get("token");
  if (fromUrl) {
    localStorage.setItem(TOKEN_KEY, fromUrl);
    const url = new URL(window.location.href);
    url.searchParams.delete("token");
    window.history.replaceState({}, "", url.toString());
    return fromUrl;
  }
  return localStorage.getItem(TOKEN_KEY) ?? "";
}

export function App() {
  const [tab, setTab] = useState<Tab>("chat");
  const [token, setToken] = useState(readToken);
  const [isConfigOpen, setIsConfigOpen] = useState(false);
  const coordinator = useCoordinator(token);
  const [contributorStatus, setContributorStatus] = useState<ContributorStatus | null>(null);

  const contributor = useMemo(
    () => new Contributor(token, (status) => setContributorStatus({ ...status })),
    [token]
  );

  useEffect(() => {
    void contributor.probe();
    return () => contributor.stop();
  }, [contributor]);

  const { state, updateCoordinatorUrl } = coordinator;
  const onlineNodes = state.nodes.filter((n) => n.online).length;

  const handleUpdateToken = (newToken: string) => {
    const trimmed = newToken.trim();
    localStorage.setItem(TOKEN_KEY, trimmed);
    setToken(trimmed);
  };

  return (
    <div className="app-container">
      {/* Sleek Topbar */}
      <header className="topbar">
        <div className="brand">
          <div className="logo-icon">🌐</div>
          <div className="brand-text">
            <strong>Community AI</strong>
            <span className="brand-tag">Decentralized Mesh</span>
          </div>
        </div>

        <div className="topbar-right">
          <button
            className="cluster-status-pill-btn"
            onClick={() => setIsConfigOpen(true)}
            title="Configure Network & Coordinator URL"
          >
            <span className={`dot ${state.connected ? "online" : "offline"}`} />
            <span>
              {state.connected
                ? `${onlineNodes} Worker${onlineNodes === 1 ? "" : "s"} Active`
                : state.connection.toUpperCase()}
            </span>
            <span style={{ fontSize: 11, opacity: 0.7 }}>⚙️</span>
          </button>
        </div>
      </header>

      {/* Network & Security Settings Modal */}
      <NetworkConfigModal
        isOpen={isConfigOpen}
        onClose={() => setIsConfigOpen(false)}
        coordinatorUrl={state.coordinatorUrl}
        connected={state.connected}
        connectionState={state.connection}
        token={token}
        onUpdateUrl={updateCoordinatorUrl}
        onUpdateToken={handleUpdateToken}
      />

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
