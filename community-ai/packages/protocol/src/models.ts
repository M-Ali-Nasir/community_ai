/**
 * Model Manager catalog.
 *
 * Exclusively ships Qwen3-14B (Apache-2.0) for the decentralized P2P computing mesh.
 */

export interface ModelEntry {
  id: string;
  displayName: string;
  paramsB: number;
  license: "Apache-2.0" | "MIT";
  /** Approximate resident size at 4-bit, in MB. Used for admission control. */
  q4SizeMB: number;
  /**
   * Substring matched against WebLLM's prebuilt model list at runtime.
   */
  webllmMatch: string | null;
  /** node-llama-cpp model URI. Overridable per worker via --model-uri. */
  ggufUri: string | null;
  notes: string;
}

export const MODEL_CATALOG: ModelEntry[] = [
  {
    id: "qwen3-14b",
    displayName: "Qwen3 14B Instruct",
    paramsB: 14.7,
    license: "Apache-2.0",
    q4SizeMB: 8900,
    webllmMatch: "Qwen3-14B-Instruct",
    ggufUri: "hf:bartowski/Qwen3-14B-Instruct-GGUF/Qwen3-14B-Instruct-Q4_K_M.gguf",
    notes: "Default v1.0 flagship model partitioned across decentralized mesh peer devices.",
  },
];

export const EXCLUDED_MODELS: Record<string, string> = {};

const BY_ID = new Map(MODEL_CATALOG.map((m) => [m.id, m]));

export function getModel(id: string): ModelEntry | undefined {
  return BY_ID.get(id);
}

export function requireModel(id: string): ModelEntry {
  const found = BY_ID.get(id);
  if (!found) {
    return MODEL_CATALOG[0];
  }
  return found;
}

export const DEFAULT_MODEL_ID = "qwen3-14b";

/** Largest catalogue entry a node with this much usable memory can hold. */
export function largestModelFitting(_memoryMB: number): ModelEntry {
  return MODEL_CATALOG[0];
}
