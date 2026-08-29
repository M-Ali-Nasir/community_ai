import { useState } from "react";
import {
  DEFAULT_LAN_COORDINATOR,
  DEFAULT_LOCAL_COORDINATOR,
} from "../lib/useCoordinator.js";

interface NetworkConfigModalProps {
  isOpen: boolean;
  onClose: () => void;
  coordinatorUrl: string;
  connected: boolean;
  connectionState: string;
  token: string;
  onUpdateUrl: (newUrl: string) => void;
  onUpdateToken: (newToken: string) => void;
}

export function NetworkConfigModal({
  isOpen,
  onClose,
  coordinatorUrl,
  connected,
  connectionState,
  token,
  onUpdateUrl,
  onUpdateToken,
}: NetworkConfigModalProps) {
  const [urlInput, setUrlInput] = useState(coordinatorUrl);
  const [tokenInput, setTokenInput] = useState(token);
  const [savedSuccess, setSavedSuccess] = useState(false);

  if (!isOpen) return null;

  const handleSave = (e?: React.FormEvent) => {
    if (e) e.preventDefault();
    onUpdateUrl(urlInput);
    onUpdateToken(tokenInput);
    setSavedSuccess(true);
    setTimeout(() => {
      setSavedSuccess(false);
      onClose();
    }, 600);
  };

  const handlePreset = (presetUrl: string) => {
    setUrlInput(presetUrl);
  };

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal-content" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <span style={{ fontSize: 20 }}>🌐</span>
            <div>
              <h3 style={{ margin: 0, fontSize: 16 }}>Network & Mesh Configuration</h3>
              <p className="sub" style={{ margin: "2px 0 0", fontSize: 12 }}>
                Connect to local or global WAN decentralized coordinator.
              </p>
            </div>
          </div>
          <button className="btn-close" onClick={onClose}>
            ✕
          </button>
        </div>

        <form onSubmit={handleSave} className="modal-body">
          {/* Connection Status Banner */}
          <div className={`status-banner ${connected ? "online" : "connecting"}`}>
            <span className={`dot ${connected ? "online" : "offline"}`} />
            <div>
              <strong>Status: {connected ? "CONNECTED (MESH ACTIVE)" : connectionState.toUpperCase()}</strong>
              <div style={{ fontSize: 11.5, opacity: 0.85 }}>
                Current Target: {coordinatorUrl}
              </div>
            </div>
          </div>

          {/* Coordinator Endpoint Input */}
          <div className="field" style={{ marginTop: 16 }}>
            <label htmlFor="coord-url">
              Coordinator Host URL (WAN / LAN / Localhost)
            </label>
            <input
              id="coord-url"
              type="text"
              className="chat-input"
              style={{ width: "100%", marginTop: 4 }}
              placeholder="http://192.168.1.9:8787 or https://mesh.your-domain.com"
              value={urlInput}
              onChange={(e) => setUrlInput(e.target.value)}
            />
          </div>

          {/* Quick Presets */}
          <div className="presets-row">
            <span style={{ fontSize: 12, color: "var(--text-muted)" }}>Presets:</span>
            <button
              type="button"
              className="btn-preset"
              onClick={() => handlePreset(DEFAULT_LOCAL_COORDINATOR)}
            >
              Localhost (8787)
            </button>
            <button
              type="button"
              className="btn-preset"
              onClick={() => handlePreset(DEFAULT_LAN_COORDINATOR)}
            >
              LAN Host (192.168.1.9)
            </button>
          </div>

          {/* WAN Guidance notice */}
          <div className="notice" style={{ marginTop: 14, fontSize: 12 }}>
            💡 <strong>WAN Internet Mesh:</strong> If connecting from a mobile phone or remote device on another Wi-Fi/4G network, run <code>./dist/start-wan-mesh.sh</code> on your computer to get a public HTTPS domain.
          </div>

          {/* Security & Auth Token */}
          <div className="field" style={{ marginTop: 14 }}>
            <label htmlFor="auth-token">
              Mesh Security Token / Shared Secret (Optional)
            </label>
            <input
              id="auth-token"
              type="password"
              className="chat-input"
              style={{ width: "100%", marginTop: 4 }}
              placeholder="Enter secure join token (leave blank for open mesh)"
              value={tokenInput}
              onChange={(e) => setTokenInput(e.target.value)}
            />
            <span className="sub" style={{ fontSize: 11.5, marginTop: 4, display: "block" }}>
              🔒 Zero-Trust Security: Cryptographic Ed25519 node identities and BLAKE3 payload hashes protect all data across the WAN.
            </span>
          </div>

          {/* Footer Actions */}
          <div className="modal-footer" style={{ marginTop: 20 }}>
            <button type="button" className="btn-clear" onClick={onClose}>
              Cancel
            </button>
            <button type="submit" className="btn-send">
              {savedSuccess ? "✓ Connected!" : "Save & Reconnect"}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
