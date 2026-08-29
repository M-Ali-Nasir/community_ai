import type { GovernorState, NodeView } from "@community-ai/protocol";
import type { CoordinatorState } from "../lib/useCoordinator.js";

function governorTone(state: GovernorState): string {
  switch (state) {
    case "contributing":
    case "available":
      return "good";
    case "throttling":
    case "resuming":
      return "warn";
    case "paused":
      return "bad";
    default:
      return "";
  }
}

function gb(mb: number): string {
  return mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${Math.round(mb)} MB`;
}

export function NodeCard({ node, highlighted }: { node: NodeView; highlighted?: boolean }) {
  const { profile, governor } = node;
  const gpu = profile.gpu;

  return (
    <article className={`node${highlighted ? " assigned" : ""}`}>
      <div className="node-head">
        <span className="node-name">{node.label}</span>
        <span className={`pill ${node.kind === "desktop-worker" ? "info" : ""}`}>
          {node.kind === "desktop-worker" ? "desktop worker" : "browser contributor"}
        </span>
        <span className={`pill ${governorTone(governor.state)}`}>{governor.state}</span>
        {node.activeTasks > 0 ? <span className="pill good">{node.activeTasks} running</span> : null}
        {node.modelStatus ? (
          <span className="pill warn">
            {node.modelStatus.phase} {Math.round(node.modelStatus.progress * 100)}%
          </span>
        ) : null}
        {!node.online ? <span className="pill bad">offline</span> : null}
      </div>

      {node.modelStatus ? (
        <div>
          <div className="meter warn">
            <span style={{ width: `${Math.round(node.modelStatus.progress * 100)}%` }} />
          </div>
          <div className="mono muted" style={{ marginTop: 5, fontSize: 11.5 }}>
            {node.modelStatus.phase} {node.modelStatus.modelId} — {node.modelStatus.detail}
          </div>
        </div>
      ) : null}

      <div className="node-grid mono">
        <div>
          <span>Accelerator</span>
          {gpu ? `${gpu.model}` : "CPU only"}
        </div>
        <div>
          <span>Backend</span>
          {gpu ? gpu.backend : "cpu"}
        </div>
        <div>
          <span>Usable memory</span>
          {gb(node.usableMemoryMB)}
        </div>
        {profile.runtime.rpc?.endpoint ? (
          <div>
            <span>Pipeline offer</span>
            {gb(profile.runtime.rpc.offeredMemoryMB)} via {profile.runtime.rpc.endpoint}
          </div>
        ) : null}
        <div>
          <span>Throughput</span>
          {node.throughput > 0 ? `${node.throughput.toFixed(1)} ${node.throughputIsMeasured ? "tok/s" : "est"}` : "—"}
        </div>
        <div>
          <span>Round trip</span>
          {profile.network.latency > 0 ? `${profile.network.latency} ms` : "—"}
          {profile.network.jitter > 0 ? ` ±${profile.network.jitter}` : ""}
        </div>
        <div>
          <span>Owner state</span>
          {profile.userState.activity}
          {profile.userState.onBattery ? " · battery" : ""}
        </div>
        <div>
          <span>CPU free</span>
          {Math.round(profile.cpu.available * 100)}% of {profile.cpu.cores} cores
        </div>
        <div>
          <span>Tasks done</span>
          {node.metrics.tasksCompleted}
          {node.metrics.tasksFailed > 0 ? ` · ${node.metrics.tasksFailed} failed` : ""}
        </div>
      </div>

      <div>
        <div className="meter">
          <span style={{ width: `${Math.round(governor.capacity * 100)}%` }} />
        </div>
        <div className="mono muted" style={{ marginTop: 5, fontSize: 11.5 }}>
          governor offering {Math.round(governor.capacity * 100)}% of spare capacity
          {governor.reasons.length > 0 ? ` — ${governor.reasons.join("; ")}` : ""}
        </div>
      </div>
    </article>
  );
}

export function NetworkPanel({ state }: { state: CoordinatorState }) {
  const stats = state.stats;
  const nodes = state.nodes;

  return (
    <>
      <div className="stats">
        <div className="stat">
          <div className="k">Nodes</div>
          <div className="v">{stats?.nodes ?? 0}</div>
        </div>
        <div className="stat">
          <div className="k">Desktop</div>
          <div className="v">{stats?.desktopWorkers ?? 0}</div>
        </div>
        <div className="stat">
          <div className="k">Browser</div>
          <div className="v">{stats?.browserContributors ?? 0}</div>
        </div>
        <div className="stat">
          <div className="k">Pooled memory</div>
          <div className="v">{gb(stats?.usableMemoryMB ?? 0)}</div>
        </div>
        <div className="stat">
          <div className="k">Jobs done</div>
          <div className="v">{stats?.jobsCompleted ?? 0}</div>
        </div>
        <div className="stat">
          <div className="k">Tokens</div>
          <div className="v">{stats?.tokensGenerated ?? 0}</div>
        </div>
      </div>

      {nodes.length === 0 ? (
        <div className="card">
          <h2>No nodes yet</h2>
          <p className="sub">
            Nothing has joined this coordinator. Start a desktop worker on a machine with a GPU,
            or enable contribution from the Contribute tab on this device.
          </p>
          <pre className="answer mono" style={{ fontSize: 12.5 }}>
{`# on a friend's computer, on the same private network
git clone <your repo> && cd community-ai
npm install
npm run build
node packages/worker-node/dist/index.js \\
  --coordinator http://<coordinator-host>:8787 \\
  --model qwen2.5-1.5b`}
          </pre>
        </div>
      ) : (
        <div className="node-list">
          {nodes.map((node) => (
            <NodeCard key={node.nodeId} node={node} />
          ))}
        </div>
      )}

      <div className="card">
        <h2>How work is placed</h2>
        <p className="sub" style={{ marginBottom: 0 }}>
          The coordinator never looks at GPU model names. Each node publishes a capability profile —
          spare memory, free compute, measured round-trip time, and what its own resource governor is
          willing to give — and the scheduler ranks nodes on that. A machine that becomes busy drops
          out of the ranking on its own, without the coordinator being told to stop using it.
        </p>
      </div>
    </>
  );
}
