import { useEffect, useMemo, useState } from "react";
import { useCoordinator } from "./lib/useCoordinator.js";
import { Contributor, type ContributorStatus } from "./lib/contributor.js";
import { ChatPanel } from "./components/ChatPanel.js";
import { NetworkPanel } from "./components/NetworkPanel.js";
import { ContributePanel } from "./components/ContributePanel.js";

type Tab = "run" | "network" | "contribute";

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
  const [tab, setTab] = useState<Tab>("run");
  const token = useMemo(readToken, []);
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

  const { state } = coordinator;
  const onlineNodes = state.nodes.filter((n) => n.online).length;

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <img src="/icon.svg" alt="" />
          <span>Community AI</span>
        </div>
        <div className="spacer" />
        <span className={`status-dot${state.connected ? " on" : ""}`}>
          {state.connected ? `${onlineNodes} node${onlineNodes === 1 ? "" : "s"} online` : state.connection}
        </span>
      </header>

      <nav className="tabs">
        <button className={`tab${tab === "run" ? " active" : ""}`} onClick={() => setTab("run")}>
          Run
        </button>
        <button
          className={`tab${tab === "network" ? " active" : ""}`}
          onClick={() => setTab("network")}
        >
          Network <span className="count">{onlineNodes}</span>
        </button>
        <button
          className={`tab${tab === "contribute" ? " active" : ""}`}
          onClick={() => setTab("contribute")}
        >
          Contribute
          {contributorStatus?.enabled ? <span className="count">on</span> : null}
        </button>
      </nav>

      <main>
        {tab === "run" ? <ChatPanel coordinator={coordinator} /> : null}
        {tab === "network" ? <NetworkPanel state={state} /> : null}
        {tab === "contribute" ? (
          <ContributePanel
            contributor={contributor}
            status={contributorStatus}
            defaultModelId={state.modelId}
          />
        ) : null}
      </main>
    </div>
  );
}
