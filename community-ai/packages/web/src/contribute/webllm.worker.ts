/// <reference lib="webworker" />
import * as webllm from "@mlc-ai/web-llm";

/**
 * WebGPU execution engine, isolated in a Web Worker.
 *
 * Inference on the main thread would freeze the page the owner is looking at,
 * which defeats the whole point of a resource governor. Everything heavy stays
 * here; the main thread only ever handles messages.
 *
 * This is the browser implementation of the same AI Runtime boundary the
 * desktop worker uses: it executes models and knows nothing about scheduling.
 */

type WorkerIn =
  | { type: "probe" }
  | { type: "load"; catalogId: string; match: string }
  | { type: "generate"; taskId: string; catalogId: string; messages: ChatTurn[]; maxTokens: number; temperature: number }
  | { type: "cancel" }
  | { type: "unload" };

interface ChatTurn {
  role: "system" | "user" | "assistant";
  content: string;
}

export type WorkerOut =
  | { type: "probe:result"; available: boolean; shaderF16: boolean; reason: string }
  | { type: "progress"; catalogId: string; phase: "downloading" | "loading" | "ready" | "error"; progress: number; detail: string }
  | { type: "loaded"; catalogId: string; webllmId: string }
  | { type: "token"; taskId: string; token: string }
  | { type: "done"; taskId: string; text: string; ttftMs: number; totalMs: number; tokens: number; tokensPerSecond: number }
  | { type: "error"; taskId: string | null; message: string };

const ctx = self as unknown as DedicatedWorkerGlobalScope;

let engine: webllm.MLCEngineInterface | null = null;
let loadedCatalogId: string | null = null;
let shaderF16 = false;
let cancelled = false;

function post(message: WorkerOut): void {
  ctx.postMessage(message);
}

async function probe(): Promise<void> {
  const gpu = (navigator as Navigator & { gpu?: GPU }).gpu;
  if (!gpu) {
    post({
      type: "probe:result",
      available: false,
      shaderF16: false,
      reason:
        "This browser has no WebGPU. Chrome or Edge 113+ on desktop, Chrome on Android, or Safari on iOS 18+ can contribute.",
    });
    return;
  }
  try {
    const adapter = await gpu.requestAdapter();
    if (!adapter) {
      post({
        type: "probe:result",
        available: false,
        shaderF16: false,
        reason: "WebGPU exists but no adapter was returned; the GPU may be blocklisted by the browser.",
      });
      return;
    }
    shaderF16 = adapter.features.has("shader-f16");
    post({ type: "probe:result", available: true, shaderF16, reason: "" });
  } catch (err) {
    post({
      type: "probe:result",
      available: false,
      shaderF16: false,
      reason: err instanceof Error ? err.message : String(err),
    });
  }
}

/**
 * Match our catalogue id to whatever WebLLM actually ships in this version,
 * rather than hard-coding upstream ids that drift between releases.
 */
function resolveWebllmId(match: string): string | null {
  const list = webllm.prebuiltAppConfig.model_list;
  const candidates = list
    .map((m) => m.model_id)
    .filter((id) => id.toLowerCase().startsWith(match.toLowerCase()));
  if (candidates.length === 0) return null;

  const preferred = shaderF16 ? "q4f16_1" : "q4f32_1";
  const exact = candidates.find((id) => id.includes(preferred) && id.endsWith("-MLC"));
  if (exact) return exact;
  const anyQuant = candidates.find((id) => id.includes("q4f") && id.endsWith("-MLC"));
  return anyQuant ?? candidates[0] ?? null;
}

async function load(catalogId: string, match: string): Promise<void> {
  if (loadedCatalogId === catalogId && engine) {
    post({ type: "loaded", catalogId, webllmId: match });
    return;
  }

  const webllmId = resolveWebllmId(match);
  if (!webllmId) {
    post({
      type: "error",
      taskId: null,
      message: `WebLLM ${webllm.prebuiltAppConfig.model_list.length} prebuilt models contain nothing matching "${match}".`,
    });
    post({ type: "progress", catalogId, phase: "error", progress: 0, detail: "no matching build" });
    return;
  }

  try {
    if (engine) {
      await engine.unload();
      engine = null;
      loadedCatalogId = null;
    }
    engine = await webllm.CreateMLCEngine(webllmId, {
      initProgressCallback: (report) => {
        const phase = report.progress >= 1 ? "loading" : "downloading";
        post({
          type: "progress",
          catalogId,
          phase,
          progress: Math.min(1, Math.max(0, report.progress)),
          detail: report.text,
        });
      },
    });
    loadedCatalogId = catalogId;
    post({ type: "progress", catalogId, phase: "ready", progress: 1, detail: webllmId });
    post({ type: "loaded", catalogId, webllmId });
  } catch (err) {
    engine = null;
    loadedCatalogId = null;
    const message = err instanceof Error ? err.message : String(err);
    post({ type: "progress", catalogId, phase: "error", progress: 0, detail: message });
    post({ type: "error", taskId: null, message });
  }
}

async function generate(input: Extract<WorkerIn, { type: "generate" }>): Promise<void> {
  if (!engine || loadedCatalogId !== input.catalogId) {
    post({ type: "error", taskId: input.taskId, message: "model not loaded on this contributor" });
    return;
  }
  cancelled = false;

  try {
    // Tasks are independent by contract, so no history leaks between them.
    await engine.resetChat();

    const startedAt = performance.now();
    let firstTokenAt: number | null = null;
    let text = "";
    let reportedTokens = 0;

    const stream = await engine.chat.completions.create({
      messages: input.messages,
      stream: true,
      stream_options: { include_usage: true },
      max_tokens: input.maxTokens,
      temperature: input.temperature,
    });

    for await (const chunk of stream) {
      if (cancelled) {
        await engine.interruptGenerate();
        post({ type: "error", taskId: input.taskId, message: "cancelled" });
        return;
      }
      const delta = chunk.choices[0]?.delta?.content;
      if (delta) {
        if (firstTokenAt === null) firstTokenAt = performance.now();
        text += delta;
        post({ type: "token", taskId: input.taskId, token: delta });
      }
      const usage = (chunk as { usage?: { completion_tokens?: number } }).usage;
      if (usage?.completion_tokens) reportedTokens = usage.completion_tokens;
    }

    const finishedAt = performance.now();
    const tokens = reportedTokens > 0 ? reportedTokens : Math.max(1, Math.round(text.length / 4));
    const decodeMs = Math.max(finishedAt - (firstTokenAt ?? startedAt), 1);

    post({
      type: "done",
      taskId: input.taskId,
      text,
      ttftMs: Math.round(((firstTokenAt ?? finishedAt) - startedAt) * 10) / 10,
      totalMs: Math.round((finishedAt - startedAt) * 10) / 10,
      tokens,
      tokensPerSecond: Math.round((tokens / (decodeMs / 1000)) * 100) / 100,
    });
  } catch (err) {
    post({
      type: "error",
      taskId: input.taskId,
      message: err instanceof Error ? err.message : String(err),
    });
  }
}

ctx.addEventListener("message", (event: MessageEvent<WorkerIn>) => {
  const message = event.data;
  switch (message.type) {
    case "probe":
      void probe();
      break;
    case "load":
      void load(message.catalogId, message.match);
      break;
    case "generate":
      void generate(message);
      break;
    case "cancel":
      cancelled = true;
      void engine?.interruptGenerate();
      break;
    case "unload":
      cancelled = true;
      void engine?.unload();
      engine = null;
      loadedCatalogId = null;
      break;
  }
});
