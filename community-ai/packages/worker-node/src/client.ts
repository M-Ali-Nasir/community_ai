import WebSocket from "ws";
import {
  type CapabilityProfile,
  type CoordinatorToWorker,
  type GovernorReport,
  type NodeMetrics,
  type WorkerToCoordinator,
  PROTOCOL_VERSION,
  emptyMetrics,
  safeJsonParse,
} from "@community-ai/protocol";

/**
 * Transport to the coordinator. Reconnects with backoff so a friend's machine
 * rejoins the network on its own after a laptop lid, a Wi-Fi switch, or the
 * coordinator restarting.
 */

export interface WorkerClientHandlers {
  onRegistered(nodeId: string, heartbeatMs: number): void;
  onRejected(reason: string): void;
  onMessage(message: CoordinatorToWorker): void;
  onConnectionChange(connected: boolean, detail: string): void;
}

export interface WorkerClientOptions {
  wsUrl: string;
  token: string;
  buildProfile: () => Promise<CapabilityProfile>;
  buildGovernor: () => GovernorReport;
  buildMetrics: () => NodeMetrics;
  activeTasks: () => number;
}

const MAX_BACKOFF_MS = 15000;

export class WorkerClient {
  private socket: WebSocket | null = null;
  private heartbeatTimer: NodeJS.Timeout | null = null;
  private reconnectTimer: NodeJS.Timeout | null = null;
  private backoffMs = 1000;
  private stopped = false;
  private registered = false;

  constructor(
    private readonly options: WorkerClientOptions,
    private readonly handlers: WorkerClientHandlers
  ) {}

  get isConnected(): boolean {
    return this.registered && this.socket?.readyState === WebSocket.OPEN;
  }

  start(): void {
    this.stopped = false;
    this.connect();
  }

  stop(): void {
    this.stopped = true;
    if (this.heartbeatTimer) clearInterval(this.heartbeatTimer);
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer);
    this.heartbeatTimer = null;
    this.reconnectTimer = null;
    try {
      this.socket?.close(1000, "worker shutting down");
    } catch {
      /* already closed */
    }
    this.socket = null;
  }

  send(message: WorkerToCoordinator): void {
    if (this.socket?.readyState !== WebSocket.OPEN) return;
    this.socket.send(JSON.stringify(message));
  }

  private connect(): void {
    if (this.stopped) return;
    this.handlers.onConnectionChange(false, "connecting");

    const socket = new WebSocket(this.options.wsUrl, {
      // The LAN testnet uses a self-signed cert so browser WebGPU works. Workers
      // trust that same local cert by skipping CA verification here.
      rejectUnauthorized: false,
    });
    this.socket = socket;

    socket.on("open", () => {
      void (async () => {
        this.backoffMs = 1000;
        const profile = await this.options.buildProfile();
        this.send({
          type: "register",
          protocolVersion: PROTOCOL_VERSION,
          token: this.options.token,
          profile,
          governor: this.options.buildGovernor(),
        });
      })();
    });

    socket.on("message", (raw) => {
      const parsed = safeJsonParse(raw.toString());
      if (parsed === null || typeof parsed !== "object") return;
      const message = parsed as CoordinatorToWorker;

      if (message.type === "registered") {
        this.registered = true;
        this.handlers.onConnectionChange(true, "connected");
        this.handlers.onRegistered(message.nodeId, message.heartbeatMs);
        this.startHeartbeat(message.heartbeatMs);
        return;
      }
      if (message.type === "rejected") {
        this.registered = false;
        this.handlers.onRejected(message.reason);
        return;
      }
      if (message.type === "ping") {
        this.send({ type: "pong", nonce: message.nonce, sentAtMs: message.sentAtMs });
        return;
      }
      this.handlers.onMessage(message);
    });

    socket.on("close", (code, reasonBuf) => {
      const reason = reasonBuf?.toString() || `code ${code}`;
      this.registered = false;
      if (this.heartbeatTimer) clearInterval(this.heartbeatTimer);
      this.heartbeatTimer = null;
      this.handlers.onConnectionChange(false, reason);
      this.scheduleReconnect();
    });

    socket.on("error", (err) => {
      this.handlers.onConnectionChange(false, err.message);
    });
  }

  private startHeartbeat(intervalMs: number): void {
    if (this.heartbeatTimer) clearInterval(this.heartbeatTimer);
    this.heartbeatTimer = setInterval(() => {
      void (async () => {
        if (this.socket?.readyState !== WebSocket.OPEN) return;
        this.send({
          type: "heartbeat",
          profile: await this.options.buildProfile(),
          governor: this.options.buildGovernor(),
          metrics: this.options.buildMetrics(),
          activeTasks: this.options.activeTasks(),
        });
      })();
    }, Math.max(1000, intervalMs));
  }

  private scheduleReconnect(): void {
    if (this.stopped || this.reconnectTimer) return;
    const delay = this.backoffMs;
    this.backoffMs = Math.min(MAX_BACKOFF_MS, Math.round(this.backoffMs * 1.7));
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      this.connect();
    }, delay);
  }
}

export function initialMetrics(): NodeMetrics {
  return emptyMetrics();
}
