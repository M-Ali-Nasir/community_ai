import { DEFAULT_MODEL_ID } from "@community-ai/protocol";

function envInt(name: string, fallback: number): number {
  const raw = process.env[name];
  if (!raw) return fallback;
  const parsed = Number.parseInt(raw, 10);
  return Number.isFinite(parsed) ? parsed : fallback;
}

export const config = {
  host: process.env.HOST ?? "0.0.0.0",
  port: envInt("PORT", 8787),

  /**
   * Shared secret for workers and clients. The primary security boundary is the
   * private overlay network (Tailscale/ZeroTier); this is defence in depth so a
   * stray listener on the tailnet cannot enrol itself.
   * Empty string disables the check, which is fine on a laptop-only LAN test.
   */
  joinToken: process.env.JOIN_TOKEN ?? "",

  heartbeatMs: envInt("HEARTBEAT_MS", 3000),
  /** A node that misses this long is considered gone and its tasks reassigned. */
  nodeTimeoutMs: envInt("NODE_TIMEOUT_MS", 12000),
  pingIntervalMs: envInt("PING_INTERVAL_MS", 5000),

  taskTimeoutMs: envInt("TASK_TIMEOUT_MS", 180000),
  maxTaskAttempts: envInt("MAX_TASK_ATTEMPTS", 3),

  defaultModelId: process.env.DEFAULT_MODEL_ID ?? DEFAULT_MODEL_ID,

  /**
   * Browser contributors are experimental and may be suspended by the OS at any
   * moment. They are never required: set to false to prove the desktop-only path.
   */
  allowBrowserContributors: process.env.ALLOW_BROWSER_CONTRIBUTORS !== "false",
  /** Never hand the reduce step to a tab if a native worker is available. */
  preferDesktopForReduce: true,

  /**
   * Layer-split inference across machines. Every extra member adds a round-trip
   * to every token, so the ceiling is low by construction; the point is running
   * a model the network could otherwise not run at all.
   */
  enableModelParallel: process.env.ENABLE_MODEL_PARALLEL !== "false",
  /**
   * Hard cap on pipeline length. Four members over typical home links already
   * puts the latency ceiling near single-digit tokens/second; beyond that the
   * result stops being usable even though it still technically runs.
   */
  maxPipelineMembers: envInt("MAX_PIPELINE_MEMBERS", 4),

  /**
   * Serve HTTPS with a local self-signed certificate. If false, serves standard HTTP.
   * WAN tunnels (like Cloudflare) automatically terminate trusted global HTTPS.
   */
  https: process.env.HTTPS === "true",
} as const;

export type Config = typeof config;
