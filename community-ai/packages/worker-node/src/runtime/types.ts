import type { AcceleratorBackend, ChatMessage, TaskMetrics } from "@community-ai/protocol";

/**
 * AI Runtime.
 *
 * The deliberate boundary between this project's contribution (discovery,
 * cluster formation, governing, scheduling) and model execution, which is
 * somebody else's solved problem. node-llama-cpp sits *behind* this interface
 * and is never allowed to leak into the scheduler. Swapping in another engine
 * later must not touch anything above this file.
 */

export interface GenerateRequest {
  modelId: string;
  messages: ChatMessage[];
  maxTokens: number;
  temperature: number;
  signal?: AbortSignal;
}

export interface GenerateResult {
  text: string;
  metrics: TaskMetrics;
}

export interface RuntimeVram {
  backend: AcceleratorBackend;
  totalVramMB: number;
  freeVramMB: number;
  name: string;
}

export type ModelProgress = (event: {
  modelId: string;
  phase: "downloading" | "loading" | "ready" | "error";
  progress: number;
  detail: string;
}) => void;

export interface AiRuntime {
  readonly engine: "node-llama-cpp" | "llama.cpp-rpc" | "none";
  /** Human-readable reason when the engine is unavailable. */
  readonly unavailableReason: string | null;
  init(): Promise<void>;
  isReady(): boolean;
  loadedModels(): string[];
  supportedModels(): string[];
  vram(): RuntimeVram | null;
  /** Load a model so the node can be scheduled work for it. */
  prepare(modelId: string, onProgress?: ModelProgress): Promise<void>;
  generate(request: GenerateRequest, onToken: (text: string) => void): Promise<GenerateResult>;
  dispose(): Promise<void>;
}
