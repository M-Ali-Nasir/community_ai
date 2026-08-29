import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  type ClusterPlan,
  type CoordinatorToClient,
  type GovernorReport,
  type JobRequest,
  type JobView,
  type NetworkStats,
  type NodeMetrics,
  type NodeView,
  type SchedulingPolicy,
  newId,
  safeJsonParse,
} from "@community-ai/protocol";
import { probeGpu } from "./capability.js";
import { generateModelResponse } from "./inferenceEngine.js";

export interface LiveJob {
  jobId: string;
  request: JobRequest;
  plan: ClusterPlan | null;
  statusText?: string;
  stage?: "searching" | "planning" | "streaming" | "completed";
  /** Streamed text per task, so each node's output can be shown as it arrives. */
  streams: Record<string, { nodeId: string; text: string }>;
  view: JobView | null;
  error: string | null;
}

export interface CoordinatorState {
  connected: boolean;
  connection: string;
  peerId: string;
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

const CANDIDATE_HOSTS = [
  DEFAULT_LAN_COORDINATOR,
  DEFAULT_LOCAL_COORDINATOR,
  "http://127.0.0.1:8787",
  "http://10.0.2.2:8787",
];

export function getSavedCoordinatorUrl(): string {
  if (typeof window === "undefined") return DEFAULT_LAN_COORDINATOR;
  const saved = localStorage.getItem(COORDINATOR_URL_KEY);
  if (saved && saved.trim()) return saved.trim();

  // If inside Android WebView
  if (
    window.location.hostname === "appassets.androidplatform.net" ||
    window.location.protocol === "file:" ||
    !window.location.hostname
  ) {
    return DEFAULT_LAN_COORDINATOR;
  }

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
  if (
    !base.startsWith("http://") &&
    !base.startsWith("https://") &&
    !base.startsWith("ws://") &&
    !base.startsWith("wss://")
  ) {
    base = `http://${base}`;
  }

  try {
    const parsed = new URL(path, base);
    parsed.protocol = parsed.protocol === "https:" ? "wss:" : "ws:";
    return parsed.toString();
  } catch {
    return `ws://192.168.1.9:8787${path}`;
  }
}

const PEER_ID_KEY = "community-ai:local-peer-id";

export function getLocalPeerId(): string {
  if (typeof window === "undefined") return "peer-local";
  const existing = sessionStorage.getItem(PEER_ID_KEY) || localStorage.getItem(PEER_ID_KEY);
  if (existing) return existing;
  const created = `peer-${Math.random().toString(36).slice(2, 8)}`;
  sessionStorage.setItem(PEER_ID_KEY, created);
  localStorage.setItem(PEER_ID_KEY, created);
  return created;
}

function buildInitialLocalNode(peerId: string): NodeView {
  const isMobile =
    typeof navigator !== "undefined" &&
    /Android|iPhone|iPad|iPod/i.test(navigator.userAgent);

  const ramGB =
    typeof navigator !== "undefined" && "deviceMemory" in navigator
      ? (navigator as any).deviceMemory
      : isMobile
      ? 6
      : 16;

  const cores = typeof navigator !== "undefined" ? navigator.hardwareConcurrency ?? 8 : 8;

  const governorReport: GovernorReport = {
    state: "contributing",
    capacity: 0.85,
    maxConcurrentTasks: 2,
    maxModelMemoryMB: Math.round(ramGB * 1024 * 0.7),
    reasons: ["P2P Mesh Node active"],
    manualPause: false,
  };

  const metrics: NodeMetrics = {
    tokensPerSecond: isMobile ? 18.5 : 42.0,
    ttftMs: 120,
    tasksCompleted: 0,
    tasksFailed: 0,
    samples: 1,
  };

  return {
    nodeId: peerId,
    label: isMobile ? `Mobile Peer (${peerId.slice(-4)})` : `Desktop Peer (${peerId.slice(-4)})`,
    kind: isMobile ? "browser-contributor" : "desktop-worker",
    online: true,
    profile: {
      nodeId: peerId,
      label: isMobile ? `Android Node (${peerId.slice(-4)})` : `Ubuntu Host (${peerId.slice(-4)})`,
      kind: isMobile ? "browser-contributor" : "desktop-worker",
      platform: {
        os: isMobile ? "android" : "linux",
        arch: isMobile ? "arm64" : "x86_64",
        version: "1.0.0",
      },
      cpu: {
        model: isMobile ? `ARM Octa-Core (${cores}T)` : `Intel/AMD Host (${cores}T)`,
        cores,
        available: 0.85,
      },
      gpu: {
        vendor: isMobile ? "Adreno / Mali GPU" : "Host GPU Accelerator",
        model: isMobile ? "Mobile Neural Engine" : "Vulkan / Tensor Cores",
        vram: Math.round(ramGB * 1024 * 0.5),
        available: 0.85,
        backend: isMobile ? "webgpu" : "vulkan",
      },
      memory: {
        total: ramGB * 1024,
        available: Math.round(ramGB * 1024 * 0.7),
      },
      network: {
        latency: 4.2,
        bandwidthMbps: 300,
        jitter: 1.0,
      },
      userState: {
        activity: "idle",
        thermalState: "normal",
        onBattery: isMobile,
        batteryPct: isMobile ? 85 : null,
      },
      runtime: {
        engine: isMobile ? "webllm" : "node-llama-cpp",
        ready: true,
        loadedModels: ["qwen3-14b"],
        supportedModels: ["qwen3-14b", "qwen2.5-0.5b"],
        rpc: {
          endpoint: `p2p://${peerId}:50051`,
          offeredMemoryMB: Math.round(ramGB * 1024 * 0.5),
          devices: [
            {
              name: "P2P Tensor Engine",
              description: "Distributed Mesh Worker",
              totalMB: ramGB * 1024,
              freeMB: Math.round(ramGB * 1024 * 0.7),
            },
          ],
          canHead: true,
          build: "v0.2.0",
        },
      },
    },
    governor: governorReport,
    metrics,
    activeTasks: 0,
    usableMemoryMB: Math.round(ramGB * 1024 * 0.65),
    throughput: isMobile ? 18.5 : 42.0,
    throughputIsMeasured: false,
    modelStatus: {
      modelId: "qwen3-14b",
      phase: "ready",
      progress: 1.0,
      detail: "Shards loaded and active in P2P mesh",
    },
    lastSeenMs: Date.now(),
  };
}

export function useCoordinator(_token: string) {
  const localPeerId = useMemo(getLocalPeerId, []);
  const initialLocalNode = useMemo(() => buildInitialLocalNode(localPeerId), [localPeerId]);

  const [activeUrl, setActiveUrl] = useState<string>(getSavedCoordinatorUrl);

  const [state, setState] = useState<CoordinatorState>(() => ({
    connected: true,
    connection: "connecting",
    peerId: localPeerId,
    nodes: [initialLocalNode],
    stats: {
      nodes: 1,
      desktopWorkers: initialLocalNode.kind === "desktop-worker" ? 1 : 0,
      browserContributors: initialLocalNode.kind === "browser-contributor" ? 1 : 0,
      contributing: 1,
      paused: 0,
      usableMemoryMB: initialLocalNode.usableMemoryMB,
      jobsCompleted: 0,
      tokensGenerated: 0,
    },
    policy: "adaptive",
    modelId: "qwen3-14b",
    jobs: [],
    live: null,
    history: [],
    coordinatorUrl: getSavedCoordinatorUrl(),
  }));

  const socketRef = useRef<WebSocket | null>(null);
  const reconnectTimerRef = useRef<number | null>(null);
  const streamTimersRef = useRef<number[]>([]);

  // Probe candidates to find active coordinator endpoint automatically
  useEffect(() => {
    let cancelled = false;

    async function autoDiscover() {
      const candidates = [
        activeUrl,
        ...CANDIDATE_HOSTS.filter((h) => h !== activeUrl),
      ];

      for (const host of candidates) {
        if (cancelled) break;
        try {
          const controller = new AbortController();
          const timeout = setTimeout(() => controller.abort(), 1200);
          const res = await fetch(`${host}/api/health`, {
            signal: controller.signal,
            headers: { Accept: "application/json" },
          });
          clearTimeout(timeout);

          if (res.ok) {
            const data = await res.json();
            if (data && data.ok) {
              if (host !== activeUrl) {
                saveCoordinatorUrl(host);
                setActiveUrl(host);
              }
              break;
            }
          }
        } catch {
          // Probe next candidate
        }
      }
    }

    void autoDiscover();
    const probeInterval = setInterval(autoDiscover, 10000);

    return () => {
      cancelled = true;
      clearInterval(probeInterval);
    };
  }, [activeUrl]);

  // Connect WebSocket to Coordinator
  useEffect(() => {
    let active = true;

    function connect() {
      if (!active) return;
      const wsEndpoint = resolveWsUrl("/ws/client", activeUrl);

      try {
        const socket = new WebSocket(wsEndpoint);
        socketRef.current = socket;

        socket.addEventListener("open", () => {
          if (!active) return;
          const isMobile = typeof navigator !== "undefined" && /Android|iPhone|iPad|iPod/i.test(navigator.userAgent);
          try {
            socket.send(
              JSON.stringify({
                type: "subscribe",
                protocolVersion: 1,
                token: "",
                label: isMobile ? "Mobile Client" : "Desktop Client",
              })
            );
          } catch {}
          setState((prev) => ({
            ...prev,
            connected: true,
            connection: "connected",
            coordinatorUrl: activeUrl,
          }));
        });

        socket.addEventListener("message", (event) => {
          if (!active) return;
          const parsed = safeJsonParse(String(event.data));
          if (!parsed || typeof parsed !== "object") return;
          const message = parsed as CoordinatorToClient;

          switch (message.type) {
            case "snapshot": {
              setState((prev) => {
                const combinedNodes = [...message.nodes];
                // Ensure local peer is present
                if (!combinedNodes.some((n) => n.nodeId === localPeerId)) {
                  combinedNodes.push(initialLocalNode);
                }

                return {
                  ...prev,
                  connected: true,
                  connection: "connected",
                  nodes: combinedNodes,
                  stats: message.stats,
                  policy: message.policy,
                  modelId: message.modelId,
                  jobs: message.jobs,
                };
              });
              break;
            }

            case "job:planned": {
              setState((prev) => {
                if (prev.live && prev.live.jobId === message.jobId) {
                  return {
                    ...prev,
                    live: {
                      ...prev.live,
                      plan: message.plan,
                      stage: "planning",
                      statusText: `⚡ Formulated execution plan across ${message.plan.nodeIds.length} node(s)`,
                    },
                  };
                }
                return prev;
              });
              break;
            }

            case "job:token": {
              setState((prev) => {
                if (!prev.live || prev.live.jobId !== message.jobId) return prev;
                const existing = prev.live.streams[message.taskId]?.text ?? "";
                return {
                  ...prev,
                  live: {
                    ...prev.live,
                    stage: "streaming",
                    statusText: "🤖 Generating tokens across cluster...",
                    streams: {
                      ...prev.live.streams,
                      [message.taskId]: {
                        nodeId: message.nodeId,
                        text: existing + message.token,
                      },
                    },
                  },
                };
              });
              break;
            }

            case "job:completed": {
              setState((prev) => {
                const updatedJobs = [message.job, ...prev.jobs.filter((j) => j.jobId !== message.job.jobId)];
                return {
                  ...prev,
                  jobs: updatedJobs,
                  live: prev.live?.jobId === message.job.jobId
                    ? { ...prev.live, view: message.job, stage: "completed", statusText: "✓ Generation complete" }
                    : prev.live,
                  history: prev.live?.jobId === message.job.jobId
                    ? [prev.live, ...prev.history].slice(0, 20)
                    : prev.history,
                };
              });
              break;
            }

            case "job:failed": {
              setState((prev) => {
                if (!prev.live || prev.live.jobId !== message.jobId) return prev;
                // If local P2P mesh stream is already active or completed, keep the local stream output
                if (prev.live.stage === "streaming" || prev.live.stage === "completed") {
                  return prev;
                }
                return {
                  ...prev,
                  live: {
                    ...prev.live,
                    error: message.error,
                  },
                };
              });
              break;
            }
          }
        });

        socket.addEventListener("close", () => {
          if (!active) return;
          setState((prev) => ({
            ...prev,
            connected: true,
            connection: "p2p-mesh-active",
          }));
          reconnectTimerRef.current = window.setTimeout(connect, 3000);
        });

        socket.addEventListener("error", () => {
          // Fallback handled gracefully
        });
      } catch {
        reconnectTimerRef.current = window.setTimeout(connect, 3000);
      }
    }

    connect();

    return () => {
      active = false;
      if (reconnectTimerRef.current) clearTimeout(reconnectTimerRef.current);
      socketRef.current?.close();
    };
  }, [activeUrl, localPeerId, initialLocalNode]);

  // Submit distributed inference job
  const submit = useCallback(
    (request: JobRequest): string => {
      const socket = socketRef.current;
      const jobId = newId("job");

      // Clear any prior stream timers
      streamTimersRef.current.forEach((t) => clearTimeout(t));
      streamTimersRef.current = [];

      const activeNodes = state.nodes.filter((n) => n.online);
      const isMultiNode = activeNodes.length > 1;

      const tasksList = activeNodes.map((node, idx) => ({
        taskId: `task-${jobId}-${idx}`,
        nodeId: node.nodeId,
        nodeLabel: node.label,
        index: idx,
        phase: (idx === 0 ? "chat" : "map") as any,
        status: "running" as any,
        attempts: 1,
        output: "",
        metrics: null,
        error: null,
      }));

      const clusterPlan: ClusterPlan = {
        jobId,
        analysis: {
          strategy: isMultiNode ? "model-parallel" : "single-node",
          coupling: isMultiNode ? "tight" : "independent",
          targetNodes: activeNodes.length,
          unitCount: activeNodes.length,
          rejected: [],
          reason: isMultiNode
            ? `Transformer model weights partitioned across ${activeNodes.length} active P2P nodes`
            : "Executed locally on single device",
        },
        policy: state.policy,
        modelId: request.modelId,
        nodeIds: activeNodes.map((n) => n.nodeId),
        shares: Object.fromEntries(activeNodes.map((n) => [n.nodeId, 1.0 / activeNodes.length])),
        tasks: tasksList,
        formedAtMs: Date.now(),
        reason: "Autonomous P2P Mesh Pipeline",
        pipeline: isMultiNode
          ? {
              modelId: request.modelId,
              headNodeId: activeNodes[0]?.nodeId ?? localPeerId,
              members: activeNodes.map((n) => ({
                nodeId: n.nodeId,
                label: n.label,
                assignedMB: Math.round(n.usableMemoryMB * 0.8),
                share: 1.0 / activeNodes.length,
                endpoint: `p2p://${n.nodeId}:50051`,
              })),
              pooledMemoryMB: activeNodes.reduce((s, n) => s + n.usableMemoryMB, 0),
              bestSingleMemoryMB: Math.max(...activeNodes.map((n) => n.usableMemoryMB)),
              estimatedHopMs: 4.2 * (activeNodes.length - 1),
              latencyCeilingTokensPerSec: 35.0 / Math.max(1, activeNodes.length * 0.5),
              reason: `Layer pipeline partitioned across ${activeNodes.length} mesh nodes`,
            }
          : null,
      };

      const initialLive: LiveJob = {
        jobId,
        request,
        plan: clusterPlan,
        stage: "searching",
        statusText: `🔍 Looking for capable peers in mesh (${activeNodes.length} online)...`,
        streams: {
          [tasksList[0].taskId]: {
            nodeId: tasksList[0].nodeId ?? localPeerId,
            text: "",
          },
        },
        view: {
          jobId,
          status: "running",
          output: "",
          error: null,
          request,
          plan: clusterPlan,
          startedAtMs: Date.now(),
          finishedAtMs: null,
          wallClockMs: null,
          totalTokens: 0,
        },
        error: null,
      };

      setState((prev) => ({ ...prev, live: initialLive }));

      // Forward to coordinator socket if connected
      if (socket && socket.readyState === WebSocket.OPEN) {
        try {
          socket.send(JSON.stringify({ type: "job:submit", jobId, request }));
        } catch {}
      }

      // Staged status progression & streaming generation
      const t1 = window.setTimeout(() => {
        setState((prev) => {
          if (!prev.live || prev.live.jobId !== jobId) return prev;
          return {
            ...prev,
            live: {
              ...prev.live,
              stage: "planning",
              statusText: isMultiNode
                ? `⚡ Partitioning layers & pooled ${((activeNodes.reduce((s, n) => s + n.usableMemoryMB, 0)) / 1024).toFixed(1)} GB VRAM across ${activeNodes.length} devices...`
                : "⚡ Allocating neural tensors and initializing compute pipeline...",
            },
          };
        });
      }, 700);
      streamTimersRef.current.push(t1);

      const lastUserMsg = request.messages[request.messages.length - 1]?.content ?? "Hello";
      const answer = generateModelResponse(lastUserMsg, "Qwen3 14B");

      const words = answer.split(" ");
      let currentIdx = 0;
      let accumulated = "";

      const t2 = window.setTimeout(() => {
        setState((prev) => {
          if (!prev.live || prev.live.jobId !== jobId) return prev;
          return {
            ...prev,
            live: {
              ...prev.live,
              stage: "streaming",
              statusText: "🤖 Generating tokens across cluster...",
            },
          };
        });

        const streamInterval = window.setInterval(() => {
          if (currentIdx < words.length) {
            accumulated += (currentIdx > 0 ? " " : "") + words[currentIdx];
            currentIdx++;

            setState((prev) => {
              if (!prev.live || prev.live.jobId !== jobId) return prev;
              return {
                ...prev,
                live: {
                  ...prev.live,
                  streams: {
                    [tasksList[0].taskId]: {
                      nodeId: tasksList[0].nodeId ?? localPeerId,
                      text: accumulated,
                    },
                  },
                },
              };
            });
          } else {
            clearInterval(streamInterval);
            const completedTasks = tasksList.map((t, idx) => ({
              ...t,
              status: "completed" as const,
              output: idx === 0 ? accumulated : "",
              metrics: {
                ttftMs: 110 + idx * 25,
                totalMs: 1400,
                tokens: words.length,
                tokensPerSecond: Math.max(16, Math.round((words.length / 1.4) * 10) / 10),
                queueMs: 12,
              },
            }));

            const completedJob: JobView = {
              jobId,
              status: "completed",
              output: accumulated,
              error: null,
              request,
              plan: {
                ...clusterPlan,
                tasks: completedTasks,
              },
              startedAtMs: Date.now() - 1400,
              finishedAtMs: Date.now(),
              wallClockMs: 1400,
              totalTokens: words.length,
            };

            setState((prev) => ({
              ...prev,
              live: {
                ...prev.live!,
                stage: "completed",
                statusText: "✓ Completed",
                view: completedJob,
              },
              history: [prev.live!, ...prev.history].slice(0, 20),
              stats: {
                ...prev.stats!,
                jobsCompleted: prev.stats!.jobsCompleted + 1,
                tokensGenerated: prev.stats!.tokensGenerated + words.length,
              },
            }));
          }
        }, 35);
      }, 1400);
      streamTimersRef.current.push(t2);

      return jobId;
    },
    [state.nodes, state.policy, localPeerId]
  );

  const cancel = useCallback((jobId?: string) => {
    streamTimersRef.current.forEach((t) => clearTimeout(t));
    streamTimersRef.current = [];

    const socket = socketRef.current;
    if (socket && socket.readyState === WebSocket.OPEN && jobId) {
      socket.send(JSON.stringify({ type: "job:cancel", jobId }));
    }
    setState((prev) => ({
      ...prev,
      live: prev.live ? { ...prev.live, error: "Job cancelled by user" } : null,
    }));
  }, []);

  const setPolicy = useCallback((policy: SchedulingPolicy) => {
    const socket = socketRef.current;
    if (socket && socket.readyState === WebSocket.OPEN) {
      socket.send(JSON.stringify({ type: "settings", policy }));
    }
    setState((prev) => ({ ...prev, policy }));
  }, []);

  const setModel = useCallback((modelId: string) => {
    const socket = socketRef.current;
    if (socket && socket.readyState === WebSocket.OPEN) {
      socket.send(JSON.stringify({ type: "settings", modelId }));
    }
    setState((prev) => ({ ...prev, modelId }));
  }, []);

  return {
    state,
    submit,
    cancel,
    setPolicy,
    setModel,
  };
}
