import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  type ClusterPlan,
  type CoordinatorToClient,
  type JobRequest,
  type JobView,
  type NetworkStats,
  type NodeView,
  type SchedulingPolicy,
  PROTOCOL_VERSION,
  newId,
  safeJsonParse,
} from "@community-ai/protocol";

export interface LiveJob {
  jobId: string;
  request: JobRequest;
  plan: ClusterPlan | null;
  /** Streamed text per task, so each node's output can be shown as it arrives. */
  streams: Record<string, { nodeId: string; text: string }>;
  view: JobView | null;
  error: string | null;
}

export interface CoordinatorState {
  connected: boolean;
  connection: string;
  nodes: NodeView[];
  stats: NetworkStats | null;
  policy: SchedulingPolicy;
  modelId: string;
  jobs: JobView[];
  live: LiveJob | null;
  history: LiveJob[];
  coordinatorUrl: string;
}

export const COORDINATOR_URL_KEY = "community-ai:coordinator-url";
export const DEFAULT_LAN_COORDINATOR = "http://192.168.1.9:8787";
export const DEFAULT_LOCAL_COORDINATOR = "http://localhost:8787";

export function getSavedCoordinatorUrl(): string {
  if (typeof window === "undefined") return DEFAULT_LAN_COORDINATOR;
  const saved = localStorage.getItem(COORDINATOR_URL_KEY);
  if (saved && saved.trim()) return saved.trim();

  // If inside Android WebView or loaded via file/appassets
  if (
    window.location.hostname === "appassets.androidplatform.net" ||
    window.location.protocol === "file:" ||
    !window.location.hostname
  ) {
    return DEFAULT_LAN_COORDINATOR;
  }

  // If running in browser on dev port 5173, connect to coordinator on 8787
  if (window.location.port === "5173") {
    return `http://${window.location.hostname}:8787`;
  }

  return window.location.origin;
}

export function saveCoordinatorUrl(url: string): void {
  if (typeof window === "undefined") return;
  localStorage.setItem(COORDINATOR_URL_KEY, url.trim());
}

export function resolveWsUrl(path: string, customBaseUrl?: string): string {
  let base = (customBaseUrl || getSavedCoordinatorUrl()).trim();
  if (!base.startsWith("http://") && !base.startsWith("https://") && !base.startsWith("ws://") && !base.startsWith("wss://")) {
    base = `http://${base}`;
  }

  try {
    const parsed = new URL(path, base);
    if (parsed.protocol === "https:") {
      parsed.protocol = "wss:";
    } else if (parsed.protocol === "http:") {
      parsed.protocol = "ws:";
    }
    return parsed.toString();
  } catch {
    return `ws://192.168.1.9:8787${path}`;
  }
}

const emptyState: CoordinatorState = {
  connected: false,
  connection: "connecting",
  nodes: [],
  stats: null,
  policy: "adaptive",
  modelId: "qwen2.5-0.5b",
  jobs: [],
  live: null,
  history: [],
  coordinatorUrl: getSavedCoordinatorUrl(),
};

export function useCoordinator(token: string) {
  const [state, setState] = useState<CoordinatorState>(emptyState);
  const [activeUrl, setActiveUrl] = useState<string>(getSavedCoordinatorUrl);
  const socketRef = useRef<WebSocket | null>(null);
  const backoffRef = useRef(1000);
  const stoppedRef = useRef(false);

  const send = useCallback((message: unknown) => {
    if (socketRef.current?.readyState !== WebSocket.OPEN) return false;
    socketRef.current.send(JSON.stringify(message));
    return true;
  }, []);

  const updateCoordinatorUrl = useCallback((newUrl: string) => {
    const trimmed = newUrl.trim();
    saveCoordinatorUrl(trimmed);
    setActiveUrl(trimmed);
    setState((prev) => ({
      ...prev,
      coordinatorUrl: trimmed,
      connected: false,
      connection: "connecting",
    }));
  }, []);

  useEffect(() => {
    stoppedRef.current = false;
    let reconnectTimer: number | null = null;

    const connect = () => {
      if (stoppedRef.current) return;
      const targetWs = resolveWsUrl("/ws/client", activeUrl);
      console.log("[Coordinator] Connecting WebSocket to:", targetWs);

      setState((prev) => ({ ...prev, connection: "connecting" }));

      try {
        const socket = new WebSocket(targetWs);
        socketRef.current = socket;

        socket.addEventListener("open", () => {
          console.log("[Coordinator] WebSocket connected to", targetWs);
          backoffRef.current = 1000;
          socket.send(
            JSON.stringify({
              type: "subscribe",
              protocolVersion: PROTOCOL_VERSION,
              token,
              label: "web/mobile client",
            })
          );
          setState((prev) => ({ ...prev, connected: true, connection: "connected" }));
        });

        socket.addEventListener("message", (event) => {
          const parsed = safeJsonParse(String(event.data));
          if (!parsed || typeof parsed !== "object") return;
          const message = parsed as CoordinatorToClient;

          setState((prev) => reduce(prev, message));
        });

        socket.addEventListener("close", () => {
          console.warn("[Coordinator] WebSocket closed, retrying...");
          setState((prev) => ({ ...prev, connected: false, connection: "reconnecting" }));
          if (stoppedRef.current) return;
          const delay = backoffRef.current;
          backoffRef.current = Math.min(8000, Math.round(backoffRef.current * 1.5));
          reconnectTimer = window.setTimeout(connect, delay);
        });

        socket.addEventListener("error", (err) => {
          console.error("[Coordinator] WebSocket connection error:", err);
        });
      } catch (err) {
        console.error("[Coordinator] Failed to instantiate WebSocket:", err);
        const delay = backoffRef.current;
        backoffRef.current = Math.min(8000, Math.round(backoffRef.current * 1.5));
        reconnectTimer = window.setTimeout(connect, delay);
      }
    };

    connect();
    return () => {
      stoppedRef.current = true;
      if (reconnectTimer) window.clearTimeout(reconnectTimer);
      socketRef.current?.close(1000, "unmounted");
      socketRef.current = null;
    };
  }, [token, activeUrl]);

  const submit = useCallback(
    (request: JobRequest): string => {
      const jobId = newId("job");
      setState((prev) => ({
        ...prev,
        live: { jobId, request, plan: null, streams: {}, view: null, error: null },
      }));

      send({ type: "job:submit", jobId, request });
      return jobId;
    },
    [send]
  );

  const cancel = useCallback(
    (jobId?: string) => {
      const target = jobId ?? state.live?.jobId;
      if (!target) return;
      send({ type: "job:cancel", jobId: target });
    },
    [send, state.live?.jobId]
  );

  const setPolicy = useCallback(
    (policy: SchedulingPolicy) => {
      send({ type: "set:policy", policy });
    },
    [send]
  );

  const setModel = useCallback(
    (modelId: string) => {
      send({ type: "set:model", modelId });
    },
    [send]
  );

  return {
    state,
    submit,
    cancel,
    setPolicy,
    setModel,
    updateCoordinatorUrl,
  };
}

function reduce(state: CoordinatorState, message: CoordinatorToClient): CoordinatorState {
  switch (message.type) {
    case "snapshot": {
      return {
        ...state,
        nodes: message.nodes,
        jobs: message.jobs,
        stats: message.stats,
        policy: message.policy,
        modelId: message.modelId,
      };
    }

    case "job:planned": {
      if (state.live && state.live.jobId === message.jobId) {
        return { ...state, live: { ...state.live, plan: message.plan } };
      }
      return state;
    }

    case "job:token": {
      if (!state.live || state.live.jobId !== message.jobId) return state;
      const current = state.live.streams[message.taskId]?.text ?? "";
      const streams = {
        ...state.live.streams,
        [message.taskId]: {
          nodeId: message.nodeId,
          text: current + message.token,
        },
      };
      return { ...state, live: { ...state.live, streams } };
    }

    case "job:task": {
      return state;
    }

    case "job:completed": {
      if (state.live && state.live.jobId === message.job.jobId) {
        return {
          ...state,
          live: { ...state.live, view: message.job },
          history: [state.live, ...state.history].slice(0, 20),
        };
      }
      return state;
    }

    case "job:failed": {
      if (state.live && state.live.jobId === message.jobId) {
        return {
          ...state,
          live: { ...state.live, error: message.error },
        };
      }
      return state;
    }

    case "error": {
      console.error("[Coordinator] Server error:", message.message);
      return state;
    }

    default:
      return state;
  }
}
