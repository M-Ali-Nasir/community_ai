import { useEffect, useState } from "react";
import { MODEL_CATALOG, getModel } from "@community-ai/protocol";
import type { Contributor, ContributorStatus } from "../lib/contributor.js";
import { deviceMemoryGB, isMobileLike } from "../lib/capability.js";

export function ContributePanel({
  contributor,
  status,
  defaultModelId,
}: {
  contributor: Contributor;
  status: ContributorStatus | null;
  defaultModelId: string;
}) {
  const [budgetMB, setBudgetMB] = useState(contributor.budgetMB);
  const [modelId, setModelId] = useState(defaultModelId);
  const [pauseWhenHidden, setPauseWhenHidden] = useState(true);
  const [pauseOnBattery, setPauseOnBattery] = useState(true);

  useEffect(() => {
    contributor.setOptions({ budgetMB, pauseWhenHidden, pauseOnBattery });
  }, [contributor, budgetMB, pauseWhenHidden, pauseOnBattery]);

  const gpu = status?.gpu ?? null;
  const enabled = status?.enabled ?? false;
  const governor = status?.governor ?? null;
  const mobile = isMobileLike();
  const memory = deviceMemoryGB();

  const browserModels = MODEL_CATALOG.filter((m) => m.webllmMatch !== null);
  const fits = browserModels.filter((m) => m.q4SizeMB * 1.25 <= budgetMB);
  const selected = getModel(modelId);
  const selectedFits = selected ? selected.q4SizeMB * 1.25 <= budgetMB : false;

  return (
    <>
      <div className="card">
        <h2>Contribute from this device</h2>
        <p className="sub">
          Optional and off by default. This tab can run a small model in a Web Worker over WebGPU and
          take independent tasks from the coordinator. It is the experimental tier: the network is
          designed to keep working perfectly if every browser closes.
        </p>

        {gpu === null ? (
          <div className="notice">Checking this device for WebGPU…</div>
        ) : !gpu.available ? (
          <>
            <div className="notice bad">{gpu.reason}</div>
            <div className="notice" style={{ marginTop: 12 }}>
              Most often this happens because the page was opened as{" "}
              <code>http://192.168…</code> — browsers block WebGPU outside HTTPS
              or localhost. Use the coordinator&apos;s <code>https://</code> URL
              (accept the certificate warning once), or run the native worker
              below.
            </div>
            <WorkerInstallCard />
          </>
        ) : (
          <>
            <div className="node-grid mono" style={{ marginBottom: 14 }}>
              <div>
                <span>WebGPU device</span>
                {gpu.description || "available"}
              </div>
              <div>
                <span>Vendor</span>
                {gpu.vendor || "hidden by browser"}
              </div>
              <div>
                <span>16-bit shaders</span>
                {gpu.shaderF16 ? "yes" : "no (uses 32-bit build)"}
              </div>
              <div>
                <span>Device memory</span>
                {memory === null ? "not reported" : `${memory} GB`}
              </div>
              <div>
                <span>Form factor</span>
                {mobile ? "phone or tablet" : "desktop"}
              </div>
              <div>
                <span>Logical cores</span>
                {navigator.hardwareConcurrency ?? "unknown"}
              </div>
            </div>

            {mobile ? (
              <div className="notice" style={{ marginBottom: 14 }}>
                On a phone, expect the browser to suspend this tab when you switch apps, and expect
                thermal throttling within a few minutes. That is exactly why phones are optional
                contributors here rather than part of the core compute pool.
              </div>
            ) : null}

            <div className="field" style={{ marginBottom: 14 }}>
              <label htmlFor="budget">
                Memory this tab may use — {(budgetMB / 1024).toFixed(1)} GB
              </label>
              <input
                id="budget"
                type="range"
                min={512}
                max={8192}
                step={256}
                value={budgetMB}
                disabled={enabled}
                onChange={(e) => setBudgetMB(Number(e.target.value))}
              />
              <span className="muted" style={{ fontSize: 12 }}>
                WebGPU does not expose how much video memory the device has, so this is a budget you
                commit rather than a measurement. {fits.length} of {browserModels.length} models fit.
              </span>
            </div>

            <div className="row" style={{ marginBottom: 14 }}>
              <div className="field" style={{ flex: "1 1 220px" }}>
                <label htmlFor="contrib-model">Model to serve</label>
                <select
                  id="contrib-model"
                  value={modelId}
                  disabled={enabled}
                  onChange={(e) => setModelId(e.target.value)}
                >
                  {browserModels.map((m) => (
                    <option key={m.id} value={m.id} disabled={m.q4SizeMB * 1.25 > budgetMB}>
                      {m.displayName} · {Math.round(m.q4SizeMB)} MB
                      {m.q4SizeMB * 1.25 > budgetMB ? " (over budget)" : ""}
                    </option>
                  ))}
                </select>
              </div>
            </div>

            <div className="row" style={{ marginBottom: 14, gap: 18 }}>
              <label className="switch">
                <input
                  type="checkbox"
                  checked={pauseWhenHidden}
                  onChange={(e) => setPauseWhenHidden(e.target.checked)}
                />
                <span>Stop when this tab is in the background</span>
              </label>
              <label className="switch">
                <input
                  type="checkbox"
                  checked={pauseOnBattery}
                  onChange={(e) => setPauseOnBattery(e.target.checked)}
                />
                <span>Stop on battery power</span>
              </label>
            </div>

            <div className="row">
              {enabled ? (
                <button className="danger" onClick={() => contributor.stop()}>
                  Stop contributing
                </button>
              ) : (
                <button
                  className="primary"
                  disabled={!selectedFits}
                  onClick={() => void contributor.start(modelId)}
                >
                  Start contributing
                </button>
              )}
              {!selectedFits ? (
                <span className="muted" style={{ fontSize: 12.5 }}>
                  Raise the memory budget to serve this model.
                </span>
              ) : null}
            </div>

            {status?.error ? (
              <div className="notice bad" style={{ marginTop: 14 }}>
                {status.error}
              </div>
            ) : null}
          </>
        )}
      </div>

      {enabled && status ? (
        <div className="card">
          <h2>This device's resource governor</h2>
          <p className="sub">
            Your device decides how much it gives. The coordinator can ask for work; it cannot take
            more than the governor allows, and every task can be refused.
          </p>

          <div className="node-grid mono" style={{ marginBottom: 12 }}>
            <div>
              <span>Connection</span>
              {status.connected ? "connected" : status.connection}
            </div>
            <div>
              <span>Governor state</span>
              {governor?.state ?? "—"}
            </div>
            <div>
              <span>Model</span>
              {status.modelPhase}
              {status.modelPhase === "downloading" ? ` ${Math.round(status.modelProgress * 100)}%` : ""}
            </div>
            <div>
              <span>Tasks done</span>
              {status.tasksCompleted}
            </div>
            <div>
              <span>Throughput</span>
              {status.tokensPerSecond ? `${status.tokensPerSecond.toFixed(1)} tok/s` : "—"}
            </div>
            <div>
              <span>Battery</span>
              {status.battery.level === null
                ? "not reported"
                : `${Math.round(status.battery.level * 100)}%${status.battery.onBattery ? " discharging" : " charging"}`}
            </div>
          </div>

          {status.modelPhase === "downloading" || status.modelPhase === "loading" ? (
            <div style={{ marginBottom: 12 }}>
              <div className="meter">
                <span style={{ width: `${Math.round(status.modelProgress * 100)}%` }} />
              </div>
              <div className="muted mono" style={{ fontSize: 11.5, marginTop: 5 }}>
                weights are cached by the browser, so this only happens once
              </div>
            </div>
          ) : null}

          <div className="meter">
            <span style={{ width: `${Math.round((governor?.capacity ?? 0) * 100)}%` }} />
          </div>
          <div className="mono muted" style={{ fontSize: 11.5, marginTop: 5 }}>
            offering {Math.round((governor?.capacity ?? 0) * 100)}% capacity
            {governor && governor.reasons.length > 0 ? ` — ${governor.reasons.join("; ")}` : ""}
          </div>

          {status.lastEvent ? (
            <div className="notice" style={{ marginTop: 12 }}>
              {status.lastEvent}
            </div>
          ) : null}
        </div>
      ) : null}

      {gpu?.available ? <WorkerInstallCard /> : null}
    </>
  );
}

function WorkerInstallCard() {
  const origin = typeof window !== "undefined" ? window.location.origin : "http://HOST:8787";
  return (
    <div className="card">
      <h2>Join from another PC (native worker)</h2>
      <p className="sub" style={{ marginBottom: 12 }}>
        This is the intended path for friends&apos; desktops and laptops. The browser Contribute tab
        is only a lightweight optional mode. The native worker uses CUDA / Vulkan / Metal / CPU,
        can hold larger models, and participates in layer-split pipelines.
      </p>
      <ol className="sub" style={{ margin: "0 0 12px", paddingLeft: 18, lineHeight: 1.55 }}>
        <li>Install Node.js 20+ on that machine.</li>
        <li>Copy the <code>community-ai</code> folder (or clone the repo) onto it.</li>
        <li>On the same private network / Tailscale as the coordinator, run:</li>
      </ol>
      <pre className="answer mono" style={{ fontSize: 12.5 }}>
{`cd community-ai
npm install
npm run build
node packages/worker-node/dist/index.js \\
  --coordinator ${origin} \\
  --name ${suggestWorkerName()} \\
  --model qwen2.5-0.5b \\
  --run-on-battery`}
      </pre>
      <p className="muted" style={{ fontSize: 12.5, marginTop: 10, marginBottom: 0 }}>
        First start downloads llama.cpp (~32 MB) into <code>~/.community-ai/llama/</code>, then the
        model into <code>./models</code>. Press <kbd>p</kbd> to pause, <kbd>q</kbd> to quit. That PC
        should then appear on the Network tab.
      </p>
    </div>
  );
}

function suggestWorkerName(): string {
  try {
    const host = window.location.hostname;
    if (host && host !== "localhost" && host !== "127.0.0.1") return "friend-pc";
  } catch {
    /* ignore */
  }
  return "friend-pc";
}
