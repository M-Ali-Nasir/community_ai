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
  const [pauseWhenHidden, setPauseWhenHidden] = useState(false);
  const [pauseOnBattery, setPauseOnBattery] = useState(true);
  const [runInBackgroundService, setRunInBackgroundService] = useState(true);

  useEffect(() => {
    contributor.setOptions({ budgetMB, pauseWhenHidden, pauseOnBattery });
  }, [contributor, budgetMB, pauseWhenHidden, pauseOnBattery]);

  const gpu = status?.gpu ?? null;
  const enabled = status?.enabled ?? false;
  const memory = deviceMemoryGB();

  const handleToggleContribution = async () => {
    if (enabled) {
      contributor.stop();
    } else {
      await contributor.start(modelId);
    }
  };

  return (
    <div className="contribute-layout">
      {/* Master Toggle Card */}
      <div className="card highlight-card">
        <div className="master-toggle-row">
          <div>
            <h2 style={{ margin: 0, fontSize: 20 }}>Compute Worker Contribution</h2>
            <p className="sub" style={{ margin: "4px 0 0" }}>
              Contribute spare GPU/CPU compute to the decentralized community cluster.
            </p>
          </div>
          <button
            className={`btn-master-toggle ${enabled ? "active" : ""}`}
            onClick={handleToggleContribution}
          >
            {enabled ? "STOP CONTRIBUTION" : "ENABLE CONTRIBUTION"}
          </button>
        </div>

        {enabled ? (
          <div className="notice good" style={{ marginTop: 16 }}>
            🟢 Worker is active and connected to the coordinator. Ready to process distributed layer passes.
          </div>
        ) : (
          <div className="notice" style={{ marginTop: 16 }}>
            ⚪ Contribution is currently disabled. Toggle the button above to begin contributing compute.
          </div>
        )}
      </div>

      {/* Background Persistence Settings */}
      <div className="card" style={{ marginTop: 20 }}>
        <h3 style={{ margin: 0, fontSize: 16 }}>Persistent Background Execution</h3>
        <p className="sub" style={{ margin: "4px 0 16px" }}>
          Configure whether this worker continues contributing when the app or browser tab is minimized or terminated.
        </p>

        <div className="toggle-setting-row">
          <div>
            <strong>Run as Background System Service</strong>
            <p className="sub" style={{ margin: 0, fontSize: 12 }}>
              Keep worker active 24/7 as a native OS daemon (systemd on Linux, Windows Service, launchd on macOS).
            </p>
          </div>
          <label className="switch">
            <input
              type="checkbox"
              checked={runInBackgroundService}
              onChange={(e) => setRunInBackgroundService(e.target.checked)}
            />
            <span className="slider round"></span>
          </label>
        </div>

        <div className="toggle-setting-row" style={{ marginTop: 12 }}>
          <div>
            <strong>Pause Automatically on Battery Power</strong>
            <p className="sub" style={{ margin: 0, fontSize: 12 }}>
              Prevents battery drain on laptops and mobile devices by pausing when unplugged from AC mains.
            </p>
          </div>
          <label className="switch">
            <input
              type="checkbox"
              checked={pauseOnBattery}
              onChange={(e) => setPauseOnBattery(e.target.checked)}
            />
            <span className="slider round"></span>
          </label>
        </div>
      </div>

      {/* Memory & Resource Quota Settings */}
      <div className="card" style={{ marginTop: 20 }}>
        <h3 style={{ margin: 0, fontSize: 16 }}>Hardware & Memory Allocation</h3>
        <p className="sub" style={{ margin: "4px 0 16px" }}>
          Set limits on how much RAM / VRAM the application may safely utilize.
        </p>

        <div className="field">
          <label htmlFor="budget">
            Maximum Memory Budget — <strong>{(budgetMB / 1024).toFixed(1)} GB</strong>
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
          <span className="sub" style={{ fontSize: 12 }}>
            Total System Memory: {memory ? `${memory} GB` : "8 GB"}. The Resource Governor automatically reduces usage when you use heavy apps.
          </span>
        </div>
      </div>

      {/* Native Desktop / Mobile Worker Installation */}
      <div className="card" style={{ marginTop: 20 }}>
        <h3 style={{ margin: 0, fontSize: 16 }}>Native Background Daemon (Zero-Browser)</h3>
        <p className="sub" style={{ margin: "4px 0 16px" }}>
          For true 24/7 contribution without opening any browser or app:
        </p>

        <div className="code-box">
          <code>
            # Install & start 24/7 background worker on Linux:<br />
            sudo ./platform/linux/install.sh<br /><br />
            # Or run standalone daemon executable:<br />
            ./target/release/community-daemon --name volunteer-node --coordinator 127.0.0.1:8080
          </code>
        </div>
      </div>
    </div>
  );
}
