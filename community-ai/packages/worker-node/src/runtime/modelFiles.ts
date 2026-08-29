import { createWriteStream } from "node:fs";
import { access, mkdir, rename, stat, unlink } from "node:fs/promises";
import { join } from "node:path";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";
import { requireModel } from "@community-ai/protocol";

/**
 * GGUF cache shared by both runtimes.
 *
 * The head of a pipeline needs the weight file on disk even though it holds
 * almost none of it in memory: llama.cpp reads the GGUF locally and streams
 * each tensor to whichever RPC peer owns that layer. Disk is the cheap
 * resource here, memory is the scarce one, and that asymmetry is the whole
 * reason layer-splitting buys anything.
 *
 * Filenames match node-llama-cpp's convention on purpose so the two runtimes
 * share one cache instead of each downloading its own copy.
 */

export interface ModelFileProgress {
  (event: { phase: "downloading" | "ready"; progress: number; detail: string }): void;
}

/** `hf:org/repo/file.gguf` -> a direct download URL. */
export function resolveModelUrl(uri: string): { url: string; fileName: string } {
  if (uri.startsWith("http://") || uri.startsWith("https://")) {
    const fileName = uri.split("/").pop() ?? "model.gguf";
    return { url: uri, fileName };
  }
  if (uri.startsWith("hf:")) {
    const rest = uri.slice(3);
    const parts = rest.split("/");
    if (parts.length < 3) throw new Error(`Malformed Hugging Face URI: ${uri}`);
    const [org, repo, ...tail] = parts;
    const file = tail.join("/");
    return {
      url: `https://huggingface.co/${org}/${repo}/resolve/main/${file}?download=true`,
      fileName: `hf_${org}_${file.split("/").pop()}`,
    };
  }
  throw new Error(`Unsupported model URI: ${uri}`);
}

export function modelFilePath(modelsDir: string, modelId: string, override?: string): string {
  if (override && !override.startsWith("hf:") && !override.startsWith("http")) return override;
  const uri = override ?? requireModel(modelId).ggufUri;
  if (!uri) throw new Error(`No GGUF source configured for ${modelId}`);
  return join(modelsDir, resolveModelUrl(uri).fileName);
}

export async function isModelPresent(path: string): Promise<boolean> {
  try {
    await access(path);
    return (await stat(path)).size > 0;
  } catch {
    return false;
  }
}

/**
 * Fetch a model into the cache if it is not already there. Downloads land in a
 * `.part` file and are renamed on success, so an interrupted transfer can never
 * be mistaken for a usable model by the next run.
 */
export async function ensureModelFile(options: {
  modelsDir: string;
  modelId: string;
  modelUri?: string;
  onProgress?: ModelFileProgress;
}): Promise<string> {
  const entry = requireModel(options.modelId);
  const uri = options.modelUri ?? entry.ggufUri;
  if (!uri) throw new Error(`No GGUF source configured for ${options.modelId}`);

  if (!uri.startsWith("hf:") && !uri.startsWith("http")) {
    if (!(await isModelPresent(uri))) throw new Error(`Model file not found: ${uri}`);
    options.onProgress?.({ phase: "ready", progress: 1, detail: uri });
    return uri;
  }

  await mkdir(options.modelsDir, { recursive: true });
  const { url, fileName } = resolveModelUrl(uri);
  const destination = join(options.modelsDir, fileName);

  if (await isModelPresent(destination)) {
    options.onProgress?.({ phase: "ready", progress: 1, detail: entry.displayName });
    return destination;
  }

  const partial = `${destination}.part`;
  await unlink(partial).catch(() => undefined);

  const response = await fetch(url, { redirect: "follow" });
  if (!response.ok || !response.body) {
    throw new Error(`Could not download ${entry.displayName}: HTTP ${response.status}`);
  }

  const total = Number(response.headers.get("content-length") ?? 0);
  let done = 0;
  let lastTick = 0;
  const source = Readable.fromWeb(response.body as Parameters<typeof Readable.fromWeb>[0]);
  source.on("data", (chunk: Buffer) => {
    done += chunk.length;
    const now = Date.now();
    if (now - lastTick > 250) {
      lastTick = now;
      options.onProgress?.({
        phase: "downloading",
        progress: total > 0 ? done / total : 0,
        detail: `${Math.round(done / 1024 / 1024)} / ${Math.round(total / 1024 / 1024)} MB`,
      });
    }
  });

  try {
    await pipeline(source, createWriteStream(partial));
  } catch (err) {
    await unlink(partial).catch(() => undefined);
    throw err;
  }

  await rename(partial, destination);
  options.onProgress?.({ phase: "ready", progress: 1, detail: entry.displayName });
  return destination;
}
