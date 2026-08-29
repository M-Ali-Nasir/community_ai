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
}

function wsUrl(path: string): string {
  const url = new URL(path, window.location.origin);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  return url.toString();
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
};

export function useCoordinator(token: string) {
  const [state, setState] = useState<CoordinatorState>(emptyState);
  const socketRef = useRef<WebSocket | null>(null);
  const backoffRef = useRef(1000);
  const stoppedRef = useRef(false);

  const send = useCallback((message: unknown) => {
    if (socketRef.current?.readyState !== WebSocket.OPEN) return false;
    socketRef.current.send(JSON.stringify(message));
    return true;
  }, []);

  useEffect(() => {
    stoppedRef.current = false;
    let reconnectTimer: number | null = null;

    const connect = () => {
      if (stoppedRef.current) return;
      const socket = new WebSocket(wsUrl("/ws/client"));
      socketRef.current = socket;

      socket.addEventListener("open", () => {
        backoffRef.current = 1000;
        socket.send(
          JSON.stringify({
            type: "subscribe",
            protocolVersion: PROTOCOL_VERSION,
            token,
            label: "web client",
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
        setState((prev) => ({ ...prev, connected: false, connection: "reconnecting" }));
        if (stoppedRef.current) return;
        const delay = backoffRef.current;
        backoffRef.current = Math.min(10000, Math.round(backoffRef.current * 1.7));
        reconnectTimer = window.setTimeout(connect, delay);
      });
    };

    connect();
    return () => {
      stoppedRef.current = true;
      if (reconnectTimer) window.clearTimeout(reconnectTimer);
      socketRef.current?.close(1000, "unmounted");
      socketRef.current = null;
    };
  }, [token]);

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

  const cancel = useCallback((jobId: string) => send({ type: "job:cancel", jobId }), [send]);

  const setPolicy = useCallback(
    (policy: SchedulingPolicy) => send({ type: "settings", policy }),
    [send]
  );

  const setModel = useCallback(
    (modelId: string) => send({ type: "settings", modelId }),
    [send]
  );

  return useMemo(
    () => ({ state, submit, cancel, setPolicy, setModel }),
    [state, submit, cancel, setPolicy, setModel]
  );
}

function reduce(prev: CoordinatorState, message: CoordinatorToClient): CoordinatorState {
  switch (message.type) {
    case "snapshot":
      return {
        ...prev,
        nodes: message.nodes,
        stats: message.stats,
        policy: message.policy,
        modelId: message.modelId,
        jobs: message.jobs,
      };

    case "job:planned": {
      if (!prev.live || prev.live.jobId !== message.jobId) return prev;
      return { ...prev, live: { ...prev.live, plan: message.plan } };
    }

    case "job:token": {
      if (!prev.live || prev.live.jobId !== message.jobId) return prev;
      const existing = prev.live.streams[message.taskId];
      return {
        ...prev,
        live: {
          ...prev.live,
          streams: {
            ...prev.live.streams,
            [message.taskId]: {
              nodeId: message.nodeId,
              text: (existing?.text ?? "") + message.token,
            },
          },
        },
      };
    }

    case "job:task": {
      if (!prev.live || prev.live.jobId !== message.jobId || !prev.live.plan) return prev;
      const tasks = prev.live.plan.tasks.some((t) => t.taskId === message.task.taskId)
        ? prev.live.plan.tasks.map((t) => (t.taskId === message.task.taskId ? message.task : t))
        : [...prev.live.plan.tasks, message.task];
      return { ...prev, live: { ...prev.live, plan: { ...prev.live.plan, tasks } } };
    }

    case "job:completed": {
      if (!prev.live || prev.live.jobId !== message.job.jobId) return prev;
      const finished: LiveJob = {
        ...prev.live,
        view: message.job,
        plan: message.job.plan ?? prev.live.plan,
      };
      return { ...prev, live: finished, history: [finished, ...prev.history].slice(0, 12) };
    }

    case "job:failed": {
      if (!prev.live || prev.live.jobId !== message.jobId) return prev;
      return { ...prev, live: { ...prev.live, error: message.error } };
    }

    case "error":
      return { ...prev, connection: message.message };

    default:
      return prev;
  }
}
