import { spawn } from "node:child_process";
import { createWriteStream } from "node:fs";
import { access, chmod, mkdir, readdir, rename, rm, stat } from "node:fs/promises";
import { homedir, arch as osArch, platform as osPlatform } from "node:os";
import { join, resolve } from "node:path";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";

/**
 * llama.cpp binary manager.
 *
 * Layer-split inference needs two executables that node-llama-cpp does not
 * ship: `ggml-rpc-server` (exposes this machine's devices to the network) and
 * `llama-server` built with the RPC backend (drives a pipeline of them). Both
 * are published as prebuilt release artifacts, so a contributor never needs a
 * compiler.
 *
 * The build tag is pinned deliberately. The ggml RPC wire format carries a
 * version and peers that disagree fail at connect time with an unhelpful
 * error, so every node in a pipeline must run the same build.
 */

export const LLAMA_BUILD = "b10632";

const RELEASE_BASE = `https://github.com/ggml-org/llama.cpp/releases/download/${LLAMA_BUILD}`;

/**
 * Vulkan is the default accelerator artifact on Linux and Windows. It is a few
 * percent slower than CUDA but covers NVIDIA, AMD and Intel with one download
 * and no CUDA runtime install, which matters far more for a volunteer network
 * of unknown hardware. macOS builds carry Metal already.
 */
export type LlamaVariant = "auto" | "vulkan" | "cpu" | "cuda" | "rocm";

export interface BinaryPaths {
  /** Directory holding the executables and their shared libraries. */
  dir: string;
  rpcServer: string;
  llamaServer: string;
  variant: string;
}

export type BinaryProgress = (event: {
  phase: "downloading" | "extracting" | "ready";
  progress: number;
  detail: string;
}) => void;

interface Asset {
  name: string;
  variant: string;
}

function assetFor(variant: LlamaVariant): Asset {
  const p = osPlatform();
  const a = osArch();
  const tag = LLAMA_BUILD;
  const pick = (suffix: string, label: string): Asset => ({
    name: `llama-${tag}-bin-${suffix}`,
    variant: label,
  });

  if (p === "darwin") {
    // Metal is compiled into the macOS artifacts; there is no separate variant.
    return a === "arm64"
      ? pick("macos-arm64.tar.gz", "metal")
      : pick("macos-x64.tar.gz", "cpu");
  }

  if (p === "win32") {
    if (a === "arm64") return pick("win-cpu-arm64.zip", "cpu");
    if (variant === "cpu") return pick("win-cpu-x64.zip", "cpu");
    if (variant === "cuda") return pick("win-cuda-12.4-x64.zip", "cuda");
    if (variant === "rocm") return pick("win-rocm-7.14-x64.zip", "rocm");
    return pick("win-vulkan-x64.zip", "vulkan");
  }

  // Linux. Upstream publishes no CUDA artifact here, which is another reason
  // Vulkan is the default: it still reaches an NVIDIA GPU through its driver.
  if (a === "arm64") {
    return variant === "cpu"
      ? pick("ubuntu-arm64.tar.gz", "cpu")
      : pick("ubuntu-vulkan-arm64.tar.gz", "vulkan");
  }
  if (variant === "cpu") return pick("ubuntu-x64.tar.gz", "cpu");
  if (variant === "rocm") return pick("ubuntu-rocm-7.14-x64.tar.gz", "rocm");
  return pick("ubuntu-vulkan-x64.tar.gz", "vulkan");
}

function exeName(base: string): string {
  return osPlatform() === "win32" ? `${base}.exe` : base;
}

async function exists(path: string): Promise<boolean> {
  try {
    await access(path);
    return true;
  } catch {
    return false;
  }
}

export function defaultBinaryRoot(): string {
  return join(homedir(), ".community-ai", "llama");
}

/**
 * Resolve the two executables, downloading and extracting the release once and
 * caching it under ~/.community-ai/llama/<build>-<variant>/.
 */
export async function ensureLlamaBinaries(options: {
  root?: string;
  variant?: LlamaVariant;
  onProgress?: BinaryProgress;
}): Promise<BinaryPaths> {
  const root = options.root ?? defaultBinaryRoot();
  const asset = assetFor(options.variant ?? "auto");
  const target = join(root, `${LLAMA_BUILD}-${asset.variant}`);
  const rpcServer = join(target, exeName("ggml-rpc-server"));
  const llamaServer = join(target, exeName("llama-server"));

  if ((await exists(rpcServer)) && (await exists(llamaServer))) {
    options.onProgress?.({ phase: "ready", progress: 1, detail: asset.variant });
    return { dir: target, rpcServer, llamaServer, variant: asset.variant };
  }

  await mkdir(root, { recursive: true });
  // Two workers on one machine share this cache, so every intermediate path is
  // process-unique and only the final rename is shared. Without this they race
  // on the same archive and one extracts a half-written file.
  const unique = `${process.pid}-${Date.now()}`;
  const archive = join(root, `.${unique}-${asset.name}`);
  const url = `${RELEASE_BASE}/${asset.name}`;

  options.onProgress?.({ phase: "downloading", progress: 0, detail: asset.name });
  await downloadFile(url, archive, (done, total) => {
    options.onProgress?.({
      phase: "downloading",
      progress: total > 0 ? done / total : 0,
      detail: `llama.cpp ${LLAMA_BUILD} ${asset.variant} — ${mb(done)} / ${mb(total)} MB`,
    });
  });

  options.onProgress?.({ phase: "extracting", progress: 0.95, detail: asset.name });
  const staging = join(root, `.staging-${unique}`);
  await mkdir(staging, { recursive: true });
  try {
    await extract(archive, staging);
  } finally {
    await rm(archive, { force: true });
  }

  // Release archives nest everything one level down, in llama-<build>/.
  const inner = await soleDirectory(staging);
  const source = inner ?? staging;
  try {
    await rename(source, target);
  } catch {
    if (await exists(rpcServer)) {
      // Another process finished first. Its copy is as good as ours.
    } else {
      // Target exists but is incomplete, most likely a previous interrupted run.
      await rm(target, { recursive: true, force: true });
      await rename(source, target);
    }
  }
  await rm(staging, { recursive: true, force: true });

  if (osPlatform() !== "win32") {
    for (const entry of await readdir(target)) {
      if (entry.startsWith("llama") || entry.startsWith("ggml")) {
        if (!entry.includes(".so") && !entry.includes(".dylib")) {
          await chmod(join(target, entry), 0o755).catch(() => undefined);
        }
      }
    }
  }

  if (!(await exists(rpcServer)) || !(await exists(llamaServer))) {
    throw new Error(
      `llama.cpp ${LLAMA_BUILD} (${asset.variant}) extracted but ggml-rpc-server or llama-server is missing`
    );
  }

  options.onProgress?.({ phase: "ready", progress: 1, detail: asset.variant });
  return { dir: target, rpcServer, llamaServer, variant: asset.variant };
}

function mb(bytes: number): number {
  return Math.round(bytes / 1024 / 1024);
}

async function downloadFile(
  url: string,
  destination: string,
  onProgress: (done: number, total: number) => void
): Promise<void> {
  const response = await fetch(url, { redirect: "follow" });
  if (!response.ok || !response.body) {
    throw new Error(`Download failed for ${url}: HTTP ${response.status}`);
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
      onProgress(done, total);
    }
  });

  await pipeline(source, createWriteStream(destination));
  onProgress(done, total || done);
}

/**
 * bsdtar handles both .tar.gz and .zip and ships with macOS, every Linux
 * distribution we target, and Windows 10 and later, so one code path covers
 * every platform without adding an archive dependency.
 */
async function extract(archive: string, into: string): Promise<void> {
  await new Promise<void>((resolveExtract, rejectExtract) => {
    const child = spawn("tar", ["-xf", archive, "-C", into], { stdio: "ignore" });
    child.on("error", rejectExtract);
    child.on("exit", (code) =>
      code === 0 ? resolveExtract() : rejectExtract(new Error(`tar exited with ${code}`))
    );
  });
}

async function soleDirectory(parent: string): Promise<string | null> {
  const entries = await readdir(parent);
  if (entries.length !== 1) return null;
  const only = join(parent, entries[0] as string);
  return (await stat(only)).isDirectory() ? only : null;
}

/**
 * Both executables load their backends from sibling shared objects, so the
 * loader path has to point at the extracted directory.
 */
export function binaryEnv(dir: string): NodeJS.ProcessEnv {
  const absolute = resolve(dir);
  const env = { ...process.env };
  if (osPlatform() === "darwin") {
    env.DYLD_LIBRARY_PATH = [absolute, env.DYLD_LIBRARY_PATH].filter(Boolean).join(":");
  } else if (osPlatform() !== "win32") {
    env.LD_LIBRARY_PATH = [absolute, env.LD_LIBRARY_PATH].filter(Boolean).join(":");
  }
  return env;
}
