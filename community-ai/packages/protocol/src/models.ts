/**
 * Model Manager catalog.
 *
 * Only ungated Apache-2.0 / MIT weights are listed as usable. This project
 * ships weights onto other people's machines, so anything gated (accept-terms
 * on Hugging Face) or research-only is excluded from the defaults: every
 * volunteer would otherwise have to accept a licence individually.
 *
 * See docs/LICENSES.md for the audit.
 */

export interface ModelEntry {
  id: string;
  displayName: string;
  paramsB: number;
  license: "Apache-2.0" | "MIT";
  /** Approximate resident size at 4-bit, in MB. Used for admission control. */
  q4SizeMB: number;
  /**
   * Substring matched against WebLLM's prebuilt model list at runtime, so a
   * renamed upstream id degrades to "unavailable in browser" rather than a crash.
   */
  webllmMatch: string | null;
  /** node-llama-cpp model URI. Overridable per worker via --model-uri. */
  ggufUri: string | null;
  notes: string;
}

export const MODEL_CATALOG: ModelEntry[] = [
  {
    id: "smollm2-360m",
    displayName: "SmolLM2 360M Instruct",
    paramsB: 0.36,
    license: "Apache-2.0",
    // Upstream only publishes q8_0 for this one, so the native side is larger
    // than the 4-bit browser build.
    q4SizeMB: 400,
    webllmMatch: "SmolLM2-360M-Instruct",
    ggufUri: "hf:HuggingFaceTB/SmolLM2-360M-Instruct-GGUF/smollm2-360m-instruct-q8_0.gguf",
    notes: "Smallest entry in the catalog. Good for phones and for smoke tests.",
  },
  {
    id: "qwen2.5-0.5b",
    displayName: "Qwen2.5 0.5B Instruct",
    paramsB: 0.49,
    license: "Apache-2.0",
    q4SizeMB: 420,
    webllmMatch: "Qwen2.5-0.5B-Instruct",
    ggufUri: "hf:Qwen/Qwen2.5-0.5B-Instruct-GGUF/qwen2.5-0.5b-instruct-q4_k_m.gguf",
    notes: "Default. Runs on a CPU-only laptop and on browser contributors.",
  },
  {
    id: "qwen2.5-1.5b",
    displayName: "Qwen2.5 1.5B Instruct",
    paramsB: 1.54,
    license: "Apache-2.0",
    q4SizeMB: 1100,
    webllmMatch: "Qwen2.5-1.5B-Instruct",
    ggufUri: "hf:Qwen/Qwen2.5-1.5B-Instruct-GGUF/qwen2.5-1.5b-instruct-q4_k_m.gguf",
    notes: "Comfortable on any discrete GPU and on most modern phones via WebGPU.",
  },
  {
    id: "smollm2-1.7b",
    displayName: "SmolLM2 1.7B Instruct",
    paramsB: 1.7,
    license: "Apache-2.0",
    q4SizeMB: 1200,
    webllmMatch: "SmolLM2-1.7B-Instruct",
    ggufUri: "hf:HuggingFaceTB/SmolLM2-1.7B-Instruct-GGUF/smollm2-1.7b-instruct-q4_k_m.gguf",
    notes: "Apache-2.0 alternative in the same size class as Qwen 1.5B.",
  },
  {
    id: "phi-3.5-mini",
    displayName: "Phi-3.5 Mini Instruct",
    paramsB: 3.8,
    license: "MIT",
    q4SizeMB: 2400,
    webllmMatch: "Phi-3.5-mini-instruct",
    ggufUri: "hf:bartowski/Phi-3.5-mini-instruct-GGUF/Phi-3.5-mini-instruct-Q4_K_M.gguf",
    notes: "MIT. Strongest small model here; needs ~3 GB of headroom.",
  },
  {
    id: "mistral-7b",
    displayName: "Mistral 7B Instruct v0.3",
    paramsB: 7.25,
    license: "Apache-2.0",
    q4SizeMB: 4500,
    webllmMatch: "Mistral-7B-Instruct-v0.3",
    ggufUri: "hf:bartowski/Mistral-7B-Instruct-v0.3-GGUF/Mistral-7B-Instruct-v0.3-Q4_K_M.gguf",
    notes: "Ungated Apache-2.0 7B.",
  },
  {
    id: "qwen2.5-7b",
    displayName: "Qwen2.5 7B Instruct",
    paramsB: 7.61,
    license: "Apache-2.0",
    q4SizeMB: 4700,
    webllmMatch: "Qwen2.5-7B-Instruct",
    // Qwen's own repo ships this split across two shards; bartowski's is one file.
    ggufUri: "hf:bartowski/Qwen2.5-7B-Instruct-GGUF/Qwen2.5-7B-Instruct-Q4_K_M.gguf",
    notes: "Target model once friends with 8 GB+ GPUs join the testnet.",
  },
];

/**
 * Kept in the source tree on purpose: the licence decision has to stay
 * auditable, and "why not Llama" is the most common question this project gets.
 */
export const EXCLUDED_MODELS: Record<string, string> = {
  "Qwen/Qwen2.5-3B-Instruct":
    "Hugging Face licence is `other` (qwen-research), not Apache-2.0. Research-only terms do not cover redistributing shards to volunteers.",
  "Qwen/Qwen2.5-72B-Instruct":
    "Hugging Face licence is `other` (Qwen License). The Qwen2.5 README names 3B and 72B as the two Apache-2.0 exceptions.",
  "meta-llama/Llama-3.1-8B-Instruct":
    "The Llama 3.1 Community License does permit this scale (far below 700M MAU) with attribution, a licence copy, the NOTICE file and the Acceptable Use Policy. It is gated on Hugging Face, so every volunteer holding weights must individually accept Meta's terms. Allowed, but not a default.",
  "google/gemma-2-2b-it":
    "Gemma Terms of Use, gated on Hugging Face. The prohibited-use policy must be passed through to every node. Not a default.",
};

const BY_ID = new Map(MODEL_CATALOG.map((m) => [m.id, m]));

export function getModel(id: string): ModelEntry | undefined {
  return BY_ID.get(id);
}

export function requireModel(id: string): ModelEntry {
  const found = BY_ID.get(id);
  if (!found) {
    throw new Error(
      `Unknown model '${id}'. Available: ${MODEL_CATALOG.map((m) => m.id).join(", ")}`
    );
  }
  return found;
}

export const DEFAULT_MODEL_ID = "qwen2.5-0.5b";

/** Largest catalogue entry a node with this much usable memory can hold. */
export function largestModelFitting(memoryMB: number): ModelEntry {
  const ranked = [...MODEL_CATALOG].sort((a, b) => b.q4SizeMB - a.q4SizeMB);
  for (const entry of ranked) {
    // Weights plus KV cache and runtime overhead.
    if (entry.q4SizeMB * 1.25 <= memoryMB) return entry;
  }
  return requireModel("smollm2-360m");
}
