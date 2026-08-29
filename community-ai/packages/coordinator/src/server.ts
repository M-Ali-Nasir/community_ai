import { createServer as createHttpServer, type Server as HttpServer } from "node:http";
import { createServer as createHttpsServer, type Server as HttpsServer } from "node:https";
import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import express from "express";
import { WebSocketServer, type WebSocket } from "ws";
import {
  type CoordinatorToClient,
  type CoordinatorToWorker,
  type NetworkStats,
  type SchedulingPolicy,
  MODEL_CATALOG,
  EXCLUDED_MODELS,
  PROTOCOL_VERSION,
  getModel,
  newId,
  parseClientMessage,
  parseWorkerMessage,
  safeJsonParse,
  usableMemoryMB,
} from "@community-ai/protocol";
import { config } from "./config.js";
import { DeviceRegistry, initialNodeMetrics } from "./registry.js";
import { JobManager } from "./jobs.js";
import type { TlsMaterial } from "./tls.js";

const here = dirname(fileURLToPath(import.meta.url));

interface ClientConn {
  clientId: string;
  socket: WebSocket;
  label: string;
}

export type CoordinatorServer = HttpServer | HttpsServer;

export interface Coordinator {
  server: CoordinatorServer;
  registry: DeviceRegistry;
  jobs: JobManager;
  close(): Promise<void>;
}

export function createCoordinator(tls: TlsMaterial | null = null): Coordinator {
  const app = express();
  app.use(express.json({ limit: "2mb" }));

  // Enable CORS for cross-origin LAN/WAN web and mobile client requests
  app.use((req, res, next) => {
    res.header("Access-Control-Allow-Origin", "*");
    res.header("Access-Control-Allow-Methods", "GET, POST, PUT, DELETE, OPTIONS");
    res.header("Access-Control-Allow-Headers", "Origin, X-Requested-With, Content-Type, Accept, Authorization, X-Community-Token");
    if (req.method === "OPTIONS") {
      return res.sendStatus(200);
    }
    next();
  });

  const registry = new DeviceRegistry();
  const clients = new Map<string, ClientConn>();

  let policy: SchedulingPolicy = "adaptive";
  let modelId = config.defaultModelId;

  const sendToWorker = (nodeId: string, message: CoordinatorToWorker): boolean => {
    const node = registry.get(nodeId);
    if (!node || node.socket.readyState !== node.socket.OPEN) return false;
    node.socket.send(JSON.stringify(message));
    return true;
  };

  const jobs = new JobManager(registry, sendToWorker);

  const broadcast = (message: CoordinatorToClient): void => {
    const payload = JSON.stringify(message);
    for (const client of clients.values()) {
      if (client.socket.readyState === client.socket.OPEN) client.socket.send(payload);
    }
  };

  const buildStats = (): NetworkStats => {
    const nodes = registry.toViews().filter((n) => n.online);
    return {
      nodes: nodes.length,
      desktopWorkers: nodes.filter((n) => n.kind === "desktop-worker").length,
      browserContributors: nodes.filter((n) => n.kind === "browser-contributor").length,
      contributing: nodes.filter(
        (n) => n.governor.state === "contributing" || n.governor.state === "available"
      ).length,
      paused: nodes.filter((n) => n.governor.state === "paused").length,
      usableMemoryMB: Math.round(nodes.reduce((sum, n) => sum + n.usableMemoryMB, 0)),
      ...jobs.stats,
    };
  };

  const snapshot = (): CoordinatorToClient => ({
    type: "snapshot",
    nodes: registry.toViews(),
    jobs: jobs.list(),
    stats: buildStats(),
    policy,
    modelId,
    serverTimeMs: Date.now(),
  });

  jobs.on("planned", (jobId, plan) => broadcast({ type: "job:planned", jobId, plan }));
  jobs.on("token", (jobId, taskId, nodeId, token) =>
    broadcast({ type: "job:token", jobId, taskId, nodeId, token })
  );
  jobs.on("task", (jobId, task) => broadcast({ type: "job:task", jobId, task }));
  jobs.on("completed", (job) => {
    broadcast({ type: "job:completed", job });
    broadcast(snapshot());
  });
  jobs.on("failed", (jobId, error) => broadcast({ type: "job:failed", jobId, error }));

  /* ---------------------------- HTTP API ---------------------------- */

  app.get("/api/health", (_req, res) => {
    res.json({ ok: true, protocolVersion: PROTOCOL_VERSION, uptimeSec: process.uptime() });
  });

  app.get("/api/state", (_req, res) => {
    res.json(snapshot());
  });

  app.get("/api/catalog", (_req, res) => {
    res.json({ models: MODEL_CATALOG, excluded: EXCLUDED_MODELS, defaultModelId: modelId });
  });

  // Lets a worker's Network Agent measure real throughput to the coordinator
  // instead of guessing a bandwidth number.
  app.get("/api/probe", (req, res) => {
    const requested = Number.parseInt(String(req.query.bytes ?? "262144"), 10);
    const bytes = Math.min(Math.max(Number.isFinite(requested) ? requested : 262144, 1024), 4_000_000);
    res.setHeader("Content-Type", "application/octet-stream");
    res.setHeader("Cache-Control", "no-store");
    res.end(Buffer.alloc(bytes, 7));
  });

  app.get("/api/config", (_req, res) => {
    res.json({
      protocolVersion: PROTOCOL_VERSION,
      heartbeatMs: config.heartbeatMs,
      requiresToken: config.joinToken.length > 0,
      allowBrowserContributors: config.allowBrowserContributors,
    });
  });

  // Serve the built PWA when it exists, so one process hosts everything.
  const webDist = resolve(here, "../../web/dist");
  if (existsSync(webDist)) {
    app.use(express.static(webDist));
    app.get("*", (req, res, next) => {
      if (req.path.startsWith("/api") || req.path.startsWith("/ws")) return next();
      res.sendFile(join(webDist, "index.html"));
    });
  } else {
    app.get("/", (_req, res) => {
      res.status(200).type("text/plain").send(
        "Coordinator is running. The web UI is not built yet.\n" +
          "Run `npm run build -w @community-ai/web`, or use `npm run dev` for the Vite dev server."
      );
    });
  }

  const server: CoordinatorServer = tls
    ? createHttpsServer({ key: tls.key, cert: tls.cert }, app)
    : createHttpServer(app);

  /* --------------------------- WebSockets --------------------------- */

  const workerWss = new WebSocketServer({ noServer: true });
  const clientWss = new WebSocketServer({ noServer: true });

  server.on("upgrade", (request, socket, head) => {
    const { url } = request;
    if (url?.startsWith("/ws/worker")) {
      workerWss.handleUpgrade(request, socket, head, (ws) =>
        workerWss.emit("connection", ws, request)
      );
    } else if (url?.startsWith("/ws/client")) {
      clientWss.handleUpgrade(request, socket, head, (ws) =>
        clientWss.emit("connection", ws, request)
      );
    } else {
      socket.destroy();
    }
  });

  const tokenOk = (token: string): boolean =>
    config.joinToken.length === 0 || token === config.joinToken;

  workerWss.on("connection", (socket: WebSocket) => {
    let nodeId: string | null = null;

    socket.on("message", (raw) => {
      const parsed = safeJsonParse(raw.toString());
      if (parsed === null) return;

      let message;
      try {
        message = parseWorkerMessage(parsed);
      } catch (err) {
        socket.send(
          JSON.stringify({ type: "rejected", reason: `malformed message: ${String(err)}` })
        );
        return;
      }

      switch (message.type) {
        case "register": {
          if (message.protocolVersion !== PROTOCOL_VERSION) {
            socket.send(
              JSON.stringify({
                type: "rejected",
                reason: `protocol mismatch: coordinator speaks v${PROTOCOL_VERSION}, worker speaks v${message.protocolVersion}. Update the worker.`,
              })
            );
            socket.close(4001, "protocol mismatch");
            return;
          }
          if (!tokenOk(message.token)) {
            socket.send(JSON.stringify({ type: "rejected", reason: "invalid join token" }));
            socket.close(4003, "invalid join token");
            return;
          }
          nodeId = message.profile.nodeId;
          const sessionId = newId("sess");
          registry.add({
            nodeId,
            sessionId,
            socket,
            profile: message.profile,
            governor: message.governor,
            metrics: initialNodeMetrics(),
            activeTasks: 0,
            connectedAtMs: Date.now(),
            lastSeenMs: Date.now(),
            rttSamples: [],
            pendingPings: new Map(),
            modelStatus: null,
          });
          socket.send(
            JSON.stringify({
              type: "registered",
              nodeId,
              sessionId,
              heartbeatMs: config.heartbeatMs,
              serverTimeMs: Date.now(),
            } satisfies CoordinatorToWorker)
          );
          const gpu = message.profile.gpu;
          console.log(
            `[registry] + ${message.profile.label} (${message.profile.kind}) ` +
              `${gpu ? `${gpu.model} ${gpu.vram}MB via ${gpu.backend}` : "cpu-only"} ` +
              `engine=${message.profile.runtime.engine}`
          );
          // A node that just joined has nothing loaded; get it warming now.
          if (!message.profile.runtime.loadedModels.includes(modelId)) {
            sendToWorker(nodeId, { type: "model:prepare", modelId });
          }
          broadcast(snapshot());
          break;
        }

        case "heartbeat": {
          if (!nodeId) return;
          registry.updateHeartbeat(
            nodeId,
            message.profile,
            message.governor,
            message.metrics,
            message.activeTasks
          );
          break;
        }

        case "task:accepted":
          jobs.onTaskAccepted(message.jobId, message.taskId);
          break;

        case "task:rejected":
          if (nodeId) jobs.onTaskRejected(message.jobId, message.taskId, nodeId, message.reason);
          break;

        case "task:token":
          if (nodeId) jobs.onToken(message.jobId, message.taskId, nodeId, message.token);
          break;

        case "task:completed":
          if (nodeId) {
            jobs.onTaskCompleted(
              message.jobId,
              message.taskId,
              nodeId,
              message.output,
              message.metrics
            );
          }
          break;

        case "task:failed":
          if (nodeId) jobs.onTaskFailed(message.jobId, message.taskId, nodeId, message.error);
          break;

        case "model:progress":
          if (nodeId) {
            registry.recordModelStatus(nodeId, {
              modelId: message.modelId,
              phase: message.phase,
              progress: message.progress,
              detail: message.detail,
            });
          }
          broadcast(snapshot());
          break;

        case "pong":
          if (nodeId) registry.recordPong(nodeId, message.nonce);
          break;
      }
    });

    const drop = (why: string) => {
      if (!nodeId) return;
      const node = registry.remove(nodeId);
      if (node) console.log(`[registry] - ${node.profile.label} (${why})`);
      jobs.onNodeLost(nodeId);
      broadcast(snapshot());
      nodeId = null;
    };

    socket.on("close", () => drop("disconnected"));
    socket.on("error", () => drop("socket error"));
  });

  clientWss.on("connection", (socket: WebSocket) => {
    const clientId = newId("client");

    socket.on("message", (raw) => {
      const parsed = safeJsonParse(raw.toString());
      if (parsed === null) return;

      let message;
      try {
        message = parseClientMessage(parsed);
      } catch (err) {
        socket.send(JSON.stringify({ type: "error", message: `malformed message: ${String(err)}` }));
        return;
      }

      switch (message.type) {
        case "subscribe": {
          if (!tokenOk(message.token)) {
            socket.send(JSON.stringify({ type: "error", message: "invalid join token" }));
            socket.close(4003, "invalid join token");
            return;
          }
          clients.set(clientId, { clientId, socket, label: message.label });
          socket.send(JSON.stringify(snapshot()));
          break;
        }

        case "settings": {
          if (message.policy) policy = message.policy;
          if (message.modelId && getModel(message.modelId) && message.modelId !== modelId) {
            modelId = message.modelId;
            // Start warming immediately rather than at first request, so the
            // download happens while the user is still typing.
            jobs.warmNodes(registry.toViews(), modelId);
          }
          broadcast(snapshot());
          break;
        }

        case "job:submit": {
          try {
            const request = { ...message.request };
            if (!getModel(request.modelId)) request.modelId = modelId;
            jobs.submit(message.jobId, request);
          } catch (err) {
            socket.send(
              JSON.stringify({
                type: "job:failed",
                jobId: message.jobId,
                error: err instanceof Error ? err.message : String(err),
              })
            );
          }
          break;
        }

        case "job:cancel":
          jobs.cancel(message.jobId);
          break;
      }
    });

    const drop = () => {
      clients.delete(clientId);
    };
    socket.on("close", drop);
    socket.on("error", drop);
  });

  /* --------------------------- Timers --------------------------- */

  const pingTimer = setInterval(() => {
    for (const node of registry.all()) {
      if (node.socket.readyState !== node.socket.OPEN) continue;
      const nonce = newId("ping");
      registry.recordPingSent(node.nodeId, nonce);
      node.socket.send(
        JSON.stringify({ type: "ping", nonce, sentAtMs: Date.now() } satisfies CoordinatorToWorker)
      );
    }
  }, config.pingIntervalMs);

  const sweepTimer = setInterval(() => {
    for (const node of registry.stale()) {
      console.log(`[registry] - ${node.profile.label} (heartbeat timeout)`);
      registry.remove(node.nodeId);
      jobs.onNodeLost(node.nodeId);
      try {
        node.socket.terminate();
      } catch {
        /* already closed */
      }
    }
    jobs.sweepTimeouts();
  }, config.heartbeatMs);

  const broadcastTimer = setInterval(() => {
    if (clients.size > 0) broadcast(snapshot());
  }, 2000);

  const close = async (): Promise<void> => {
    clearInterval(pingTimer);
    clearInterval(sweepTimer);
    clearInterval(broadcastTimer);
    for (const node of registry.all()) node.socket.close(1001, "coordinator shutting down");
    for (const client of clients.values()) client.socket.close(1001, "coordinator shutting down");
    workerWss.close();
    clientWss.close();
    await new Promise<void>((done) => server.close(() => done()));
  };

  void usableMemoryMB;
  return { server, registry, jobs, close };
}
