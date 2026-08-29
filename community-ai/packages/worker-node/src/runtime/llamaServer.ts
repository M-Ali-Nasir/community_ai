import { spawn, type ChildProcess } from "node:child_process";
import { MODEL_CATALOG, requireModel, type AcceleratorBackend } from "@community-ai/protocol";
import { binaryEnv, type BinaryPaths } from "./llamaBinaries.js";
import {
  ensureModelFile,
  isModelPresent,
  modelFilePath,
  type ModelFileProgress,
} from "./modelFiles.js";
import type {
  AiRuntime,
  GenerateRequest,
  GenerateResult,
  ModelProgress,
  RuntimeVram,
} from "./types.js";

/**
 * Pipeline-parallel runtime.
 *
 * Runs `llama-server` as the head of a pipeline whose layers live on remote
 * `ggml-rpc-server` peers. The head reads the GGUF from local disk and streams
 * each tensor to the peer that owns that layer, so the head's own memory
 * footprint stays near zero regardless of model size.
 *
 * It also serves the degenerate one-node case, where the only peer is this
 * machine. That keeps a single code path for both, which matters because the
 * research compares them directly and a second implementation would make the
 * comparison meaningless.
 */

export interface PipelineMember {
  nodeId: string;
  label: string;
  /** `host:port` of that node's ggml-rpc-server. */
  endpoint: string;
  /** Fraction of the model's layers this member should hold. */
  share: number;
}

export interface PipelineConfig {
  modelId: string;
  members: PipelineMember[];
}

export interface LlamaServerOptions {
  modelsDir: string;
  modelUri?: string;
  contextSize: number;
  /** Port for the local llama-server control plane. Bound to loopback only. */
  port: number;
  allowModels?: string[];
}

type Timings = {
  predicted_per_second?: number;
  predicted_n?: number;
  prompt_ms?: number;
};

export class PipelineRuntime implements AiRuntime {
  readonly engine = "llama.cpp-rpc" as const;
  unavailableReason: string | null = "not initialised";

  private child: ChildProcess | null = null;
  private signature = "";
  private current: PipelineConfig | null = null;
  private starting: Promise<void> | null = null;
  private generating = false;
  private lastStderr = "";
  private onDisk = new Set<string>();
  private binaries: BinaryPaths | null = null;

  constructor(private readonly options: LlamaServerOptions) {}

  async init(): Promise<void> {
    await this.refreshAvailable();
  }

  /**
   * Called once the llama.cpp release has been fetched. The worker registers
   * and reports hardware before this happens, so the dashboard shows it
   * downloading rather than missing.
   */
  attach(binaries: BinaryPaths): void {
    this.binaries = binaries;
    this.unavailableReason = null;
  }

  private requireBinaries(): BinaryPaths {
    if (!this.binaries) throw new Error(this.unavailableReason ?? "llama.cpp runtime not ready");
    return this.binaries;
  }

  /**
   * The head is "ready" for a model once the weight file is on disk. Bringing
   * the pipeline up on top of that takes seconds, whereas the download it would
   * otherwise hide takes minutes — and it is the minutes that make a node look
   * hung to the coordinator.
   */
  isReady(): boolean {
    return this.binaries !== null && this.onDisk.size > 0;
  }

  loadedModels(): string[] {
    return [...this.onDisk];
  }

  async refreshAvailable(): Promise<void> {
    const found = new Set<string>();
    for (const id of this.supportedModels()) {
      try {
        const path = modelFilePath(this.options.modelsDir, id, this.options.modelUri);
        if (await isModelPresent(path)) found.add(id);
      } catch {
        /* model has no local representation */
      }
    }
    this.onDisk = found;
  }

  supportedModels(): string[] {
    const allow = this.options.allowModels;
    const ids = MODEL_CATALOG.filter((m) => m.ggufUri !== null).map((m) => m.id);
    return allow && allow.length > 0 ? ids.filter((id) => allow.includes(id)) : ids;
  }

  /** Reported by the RPC agent instead; the head deliberately holds no weights. */
  vram(): RuntimeVram | null {
    return null;
  }

  get pipeline(): PipelineConfig | null {
    return this.current;
  }

  /** Members of the current pipeline, for the dashboard. */
  get members(): PipelineMember[] {
    return this.current?.members ?? [];
  }

  async prepare(modelId: string, onProgress?: ModelProgress): Promise<void> {
    await this.fetchWeights(modelId, onProgress);
  }

  private async fetchWeights(modelId: string, onProgress?: ModelProgress): Promise<string> {
    const relay: ModelFileProgress = (event) =>
      onProgress?.({
        modelId,
        phase: event.phase === "ready" ? "ready" : "downloading",
        progress: event.progress,
        detail: event.detail,
      });
    const path = await ensureModelFile({
      modelsDir: this.options.modelsDir,
      modelId,
      modelUri: this.options.modelUri,
      onProgress: relay,
    });
    this.onDisk.add(modelId);
    return path;
  }

  /**
   * Bring up (or reuse) a llama-server bound to exactly this set of peers.
   * Restarting is unavoidable when membership changes because llama.cpp fixes
   * the device list at model-load time.
   */
  async configure(config: PipelineConfig, onProgress?: ModelProgress): Promise<void> {
    const next = signatureOf(config);
    if (next === this.signature && this.child) return;
    if (this.starting) await this.starting.catch(() => undefined);
    if (next === this.signature && this.child) return;

    this.starting = this.launch(config, next, onProgress);
    try {
      await this.starting;
    } finally {
      this.starting = null;
    }
  }

  private async launch(
    config: PipelineConfig,
    signature: string,
    onProgress?: ModelProgress
  ): Promise<void> {
    const entry = requireModel(config.modelId);
    if (config.members.length === 0) throw new Error("pipeline has no members");
    const binaries = this.requireBinaries();

    const modelPath = await this.fetchWeights(config.modelId, onProgress);
    await this.shutdownChild();

    onProgress?.({
      modelId: config.modelId,
      phase: "loading",
      progress: 0,
      detail:
        config.members.length === 1
          ? `loading on ${config.members[0]?.label}`
          : `distributing layers across ${config.members.length} nodes`,
    });

    // Device order follows --rpc order, so RPCn lines up with members[n] and
    // --tensor-split can address each member by position.
    const endpoints = config.members.map((m) => m.endpoint).join(",");
    const devices = config.members.map((_, i) => `RPC${i}`).join(",");
    const split = normaliseShares(config.members.map((m) => m.share));

    const args = [
      "-m", modelPath,
      "--rpc", endpoints,
      "--device", devices,
      "--tensor-split", split.join(","),
      "-ngl", "999",
      "--host", "127.0.0.1",
      "--port", String(this.options.port),
      "-c", String(this.options.contextSize),
      "--jinja",
      // One pipeline serves one request at a time; concurrent slots would
      // interleave round-trips across the same links and distort every timing.
      "-np", "1",
    ];

    const child = spawn(binaries.llamaServer, args, {
      env: binaryEnv(binaries.dir),
      stdio: ["ignore", "pipe", "pipe"],
    });
    this.child = child;
    this.lastStderr = "";

    child.stderr?.on("data", (buf: Buffer) => {
      const text = buf.toString();
      this.lastStderr = `${this.lastStderr}${text}`.slice(-4000);
    });
    child.on("exit", (code) => {
      if (this.child === child) {
        this.child = null;
        this.current = null;
        this.signature = "";
        if (code !== 0) {
          this.unavailableReason = `llama-server exited with code ${code}`;
        }
      }
    });

    try {
      await this.waitForHealth(180_000);
    } catch (err) {
      await this.shutdownChild();
      const tail = this.lastStderr.split("\n").filter(Boolean).slice(-4).join(" | ");
      throw new Error(
        `${err instanceof Error ? err.message : String(err)}${tail ? ` — ${tail}` : ""}`
      );
    }

    this.current = config;
    this.signature = signature;
    this.unavailableReason = null;
    onProgress?.({
      modelId: config.modelId,
      phase: "ready",
      progress: 1,
      detail:
        config.members.length === 1
          ? entry.displayName
          : `${entry.displayName} across ${config.members.length} nodes`,
    });
  }

  private async waitForHealth(timeoutMs: number): Promise<void> {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      if (!this.child) throw new Error("llama-server stopped while starting");
      try {
        const response = await fetch(`${this.baseUrl}/health`);
        if (response.ok) return;
      } catch {
        /* not listening yet */
      }
      await new Promise((r) => setTimeout(r, 500));
    }
    throw new Error("llama-server did not become healthy in time");
  }

  private get baseUrl(): string {
    return `http://127.0.0.1:${this.options.port}`;
  }

  async generate(
    request: GenerateRequest,
    onToken: (text: string) => void
  ): Promise<GenerateResult> {
    if (this.generating) throw new Error("pipeline is already generating");
    if (!this.child || !this.current) throw new Error("pipeline is not configured");
    if (this.current.modelId !== request.modelId) {
      throw new Error(
        `pipeline is serving ${this.current.modelId}, not ${request.modelId}`
      );
    }

    this.generating = true;
    const queuedAt = performance.now();
    try {
      const startedAt = performance.now();
      let firstTokenAt: number | null = null;
      let text = "";
      let timings: Timings | null = null;

      const response = await fetch(`${this.baseUrl}/v1/chat/completions`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        signal: request.signal ?? null,
        body: JSON.stringify({
          messages: request.messages,
          max_tokens: request.maxTokens,
          temperature: request.temperature,
          stream: true,
          timings_per_token: true,
        }),
      });

      if (!response.ok || !response.body) {
        throw new Error(`llama-server returned HTTP ${response.status}`);
      }

      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      let buffer = "";

      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true });

        const lines = buffer.split("\n");
        buffer = lines.pop() ?? "";
        for (const line of lines) {
          if (!line.startsWith("data:")) continue;
          const payload = line.slice(5).trim();
          if (payload === "" || payload === "[DONE]") continue;
          let chunk: Record<string, any>;
          try {
            chunk = JSON.parse(payload);
          } catch {
            continue;
          }
          const delta = chunk?.choices?.[0]?.delta?.content;
          if (typeof delta === "string" && delta.length > 0) {
            if (firstTokenAt === null) firstTokenAt = performance.now();
            text += delta;
            onToken(delta);
          }
          if (chunk?.timings) timings = chunk.timings as Timings;
        }
      }

      const finishedAt = performance.now();
      const totalMs = finishedAt - startedAt;
      const decodeMs = Math.max(finishedAt - (firstTokenAt ?? startedAt), 1);
      const tokens = timings?.predicted_n ?? Math.max(1, Math.round(text.length / 4));
      const measured = timings?.predicted_per_second ?? tokens / (decodeMs / 1000);

      return {
        text,
        metrics: {
          ttftMs: Math.round(((firstTokenAt ?? finishedAt) - startedAt) * 10) / 10,
          totalMs: Math.round(totalMs * 10) / 10,
          tokens,
          tokensPerSecond: Math.round(measured * 100) / 100,
          queueMs: Math.round((startedAt - queuedAt) * 10) / 10,
        },
      };
    } finally {
      this.generating = false;
    }
  }

  private async shutdownChild(): Promise<void> {
    const child = this.child;
    this.child = null;
    this.current = null;
    this.signature = "";
    if (!child) return;
    child.kill("SIGTERM");
    await new Promise((r) => setTimeout(r, 400));
    if (!child.killed) child.kill("SIGKILL");
  }

  async teardown(): Promise<void> {
    await this.shutdownChild();
  }

  async dispose(): Promise<void> {
    await this.shutdownChild();
  }
}

function signatureOf(config: PipelineConfig): string {
  return [
    config.modelId,
    ...config.members.map((m) => `${m.endpoint}@${m.share.toFixed(3)}`),
  ].join("|");
}

/**
 * llama.cpp reads --tensor-split as relative weights. Rounding to three places
 * keeps the signature stable so a negligible share drift does not force a
 * needless restart of the whole pipeline.
 */
function normaliseShares(shares: number[]): number[] {
  const total = shares.reduce((a, b) => a + b, 0);
  if (total <= 0) return shares.map(() => Number((1 / shares.length).toFixed(3)));
  return shares.map((s) => Number((s / total).toFixed(3)));
}

export function backendOfVariant(variant: string): AcceleratorBackend {
  if (variant === "cuda") return "cuda";
  if (variant === "metal") return "metal";
  if (variant === "vulkan" || variant === "rocm") return "vulkan";
  return "cpu";
}
