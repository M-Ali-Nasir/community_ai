import { mkdir } from "node:fs/promises";
import { resolve } from "node:path";
import { MODEL_CATALOG, requireModel, type AcceleratorBackend } from "@community-ai/protocol";
import type {
  AiRuntime,
  GenerateRequest,
  GenerateResult,
  ModelProgress,
  RuntimeVram,
} from "./types.js";

/**
 * node-llama-cpp adapter.
 *
 * Imported dynamically so that a machine where the native build is unavailable
 * still starts, registers, and shows up in the dashboard as "runtime not ready"
 * instead of crashing. The coordinator simply never schedules to it.
 */

/* The package ships its own types, but it is an optional dependency, so the
   surface we use is narrowed here rather than imported at type level. */
type AnyRecord = Record<string, any>;

export interface LlamaRuntimeOptions {
  modelsDir: string;
  /** Overrides the catalogue URI, e.g. a local .gguf path. */
  modelUri?: string;
  /** -1 lets llama.cpp decide how many layers to offload to the GPU. */
  gpuLayers?: number;
  contextSize?: number;
  /** Restricts which catalogue models this node advertises. */
  allowModels?: string[];
}

export class LlamaCppRuntime implements AiRuntime {
  readonly engine = "node-llama-cpp" as const;
  unavailableReason: string | null = "not initialised";

  private llama: AnyRecord | null = null;
  private module: AnyRecord | null = null;
  private model: AnyRecord | null = null;
  private context: AnyRecord | null = null;
  private currentModelId: string | null = null;
  private loading: Promise<void> | null = null;
  private generating = false;

  constructor(private readonly options: LlamaRuntimeOptions) {}

  async init(): Promise<void> {
    try {
      // Bare specifier kept in a variable so bundlers do not try to resolve it.
      const specifier = "node-llama-cpp";
      this.module = (await import(specifier)) as AnyRecord;
      this.llama = await this.module.getLlama({ build: "never", progressLogs: false });
      await mkdir(this.options.modelsDir, { recursive: true });
      this.unavailableReason = null;
    } catch (err) {
      this.unavailableReason =
        `node-llama-cpp unavailable (${err instanceof Error ? err.message : String(err)}). ` +
        "Install it with `npm i node-llama-cpp -w @community-ai/worker` to contribute compute.";
      this.llama = null;
      this.module = null;
    }
  }

  isReady(): boolean {
    return this.llama !== null && this.model !== null && this.currentModelId !== null;
  }

  loadedModels(): string[] {
    return this.currentModelId ? [this.currentModelId] : [];
  }

  supportedModels(): string[] {
    const allow = this.options.allowModels;
    const ids = MODEL_CATALOG.filter((m) => m.ggufUri !== null).map((m) => m.id);
    return allow && allow.length > 0 ? ids.filter((id) => allow.includes(id)) : ids;
  }

  vram(): RuntimeVram | null {
    if (!this.llama) return null;
    const backend = normaliseBackend(this.llama.gpu);
    const state = this.lastVramState;
    return {
      backend,
      totalVramMB: state ? Math.round(state.total / 1024 / 1024) : 0,
      freeVramMB: state ? Math.round(state.free / 1024 / 1024) : 0,
      name: this.deviceName ?? (backend === "cpu" ? "CPU" : backend.toUpperCase()),
    };
  }

  private lastVramState: { total: number; free: number } | null = null;
  private deviceName: string | null = null;

  /** Refresh VRAM state; called on the heartbeat tick by the worker loop. */
  async refreshDeviceState(): Promise<void> {
    if (!this.llama) return;
    try {
      const state = await this.llama.getVramState();
      this.lastVramState = { total: Number(state.total ?? 0), free: Number(state.free ?? 0) };
    } catch {
      this.lastVramState = null;
    }
    if (this.deviceName === null) {
      try {
        const names = await this.llama.getGpuDeviceNames?.();
        if (Array.isArray(names) && names.length > 0) this.deviceName = String(names[0]);
      } catch {
        this.deviceName = null;
      }
    }
  }

  async prepare(modelId: string, onProgress?: ModelProgress): Promise<void> {
    if (this.currentModelId === modelId && this.model) return;
    if (this.loading) await this.loading.catch(() => undefined);
    if (this.currentModelId === modelId && this.model) return;

    this.loading = this.loadModel(modelId, onProgress);
    try {
      await this.loading;
    } finally {
      this.loading = null;
    }
  }

  private async loadModel(modelId: string, onProgress?: ModelProgress): Promise<void> {
    if (!this.llama || !this.module) {
      throw new Error(this.unavailableReason ?? "runtime unavailable");
    }
    const entry = requireModel(modelId);
    const uri = this.options.modelUri ?? entry.ggufUri;
    if (!uri) throw new Error(`No GGUF source configured for ${modelId}`);

    await this.unload();

    onProgress?.({ modelId, phase: "downloading", progress: 0, detail: uri });
    const modelPath = await this.module.resolveModelFile(uri, {
      directory: resolve(this.options.modelsDir),
      // The library's own progress bar would fight the worker's status panel.
      cli: false,
      onProgress: (status: AnyRecord) => {
        const total = Number(status?.totalSize ?? 0);
        const done = Number(status?.downloadedSize ?? 0);
        if (total > 0) {
          onProgress?.({
            modelId,
            phase: "downloading",
            progress: Math.min(1, done / total),
            detail: `${Math.round(done / 1024 / 1024)} / ${Math.round(total / 1024 / 1024)} MB`,
          });
        }
      },
    });

    onProgress?.({ modelId, phase: "loading", progress: 0, detail: "loading into memory" });
    const model: AnyRecord = await this.llama.loadModel({
      modelPath,
      gpuLayers: this.options.gpuLayers ?? undefined,
      onLoadProgress: (progress: number) => {
        onProgress?.({ modelId, phase: "loading", progress, detail: "loading into memory" });
      },
    });
    this.model = model;
    this.context = await model.createContext({
      contextSize: this.options.contextSize ?? 4096,
    });
    this.currentModelId = modelId;
    await this.refreshDeviceState();
    onProgress?.({ modelId, phase: "ready", progress: 1, detail: entry.displayName });
  }

  async generate(
    request: GenerateRequest,
    onToken: (text: string) => void
  ): Promise<GenerateResult> {
    if (this.generating) throw new Error("runtime is already generating");
    await this.prepare(request.modelId);
    if (!this.model || !this.context || !this.module) throw new Error("model is not loaded");

    this.generating = true;
    const queuedAt = performance.now();
    try {
      const sequence = this.context.getSequence();
      const session = new this.module.LlamaChatSession({
        contextSequence: sequence,
        systemPrompt: systemPromptOf(request.messages),
      });

      const prompt = userPromptOf(request.messages);
      const startedAt = performance.now();
      let firstTokenAt: number | null = null;

      const text: string = await session.prompt(prompt, {
        maxTokens: request.maxTokens,
        temperature: request.temperature,
        signal: request.signal,
        onTextChunk: (chunk: string) => {
          if (firstTokenAt === null) firstTokenAt = performance.now();
          onToken(chunk);
        },
      });

      const finishedAt = performance.now();
      let tokens = 0;
      try {
        tokens = this.model.tokenize(text).length;
      } catch {
        tokens = Math.max(1, Math.round(text.length / 4));
      }
      const totalMs = finishedAt - startedAt;
      const decodeMs = Math.max(finishedAt - (firstTokenAt ?? startedAt), 1);

      try {
        sequence.dispose?.();
      } catch {
        /* sequence already released */
      }

      return {
        text,
        metrics: {
          ttftMs: Math.round(((firstTokenAt ?? finishedAt) - startedAt) * 10) / 10,
          totalMs: Math.round(totalMs * 10) / 10,
          tokens,
          tokensPerSecond: Math.round((tokens / (decodeMs / 1000)) * 100) / 100,
          queueMs: Math.round((startedAt - queuedAt) * 10) / 10,
        },
      };
    } finally {
      this.generating = false;
    }
  }

  private async unload(): Promise<void> {
    try {
      await this.context?.dispose?.();
    } catch {
      /* ignore */
    }
    try {
      await this.model?.dispose?.();
    } catch {
      /* ignore */
    }
    this.context = null;
    this.model = null;
    this.currentModelId = null;
  }

  async dispose(): Promise<void> {
    await this.unload();
    this.llama = null;
    this.module = null;
  }
}

function normaliseBackend(gpu: unknown): AcceleratorBackend {
  if (gpu === "cuda") return "cuda";
  if (gpu === "vulkan") return "vulkan";
  if (gpu === "metal") return "metal";
  return "cpu";
}

function systemPromptOf(messages: GenerateRequest["messages"]): string | undefined {
  const system = messages.filter((m) => m.role === "system").map((m) => m.content);
  return system.length > 0 ? system.join("\n\n") : undefined;
}

/**
 * The coordinator sends whole conversations. llama.cpp chat sessions own their
 * own history, so prior turns are folded into the prompt for this stateless
 * one-shot execution model. Tasks are independent by design: any node must be
 * able to run any task without shared state.
 */
function userPromptOf(messages: GenerateRequest["messages"]): string {
  const turns = messages.filter((m) => m.role !== "system");
  if (turns.length === 0) return "";
  if (turns.length === 1) return turns[0]?.content ?? "";
  return turns
    .map((m) => `${m.role === "user" ? "User" : "Assistant"}: ${m.content}`)
    .join("\n\n")
    .concat("\n\nAssistant:");
}
