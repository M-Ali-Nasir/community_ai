import { MODEL_CATALOG } from "@community-ai/protocol";
import { defaultGovernorOptions, type GovernorOptions } from "./agents/governor.js";
import { defaultNodeLabel } from "./agents/hardware.js";
import type { LlamaVariant } from "./runtime/llamaBinaries.js";

export interface WorkerArgs {
  coordinator: string;
  token: string;
  label: string;
  modelId: string | null;
  modelUri: string | null;
  modelsDir: string;
  gpuLayers: number | undefined;
  contextSize: number;
  governor: GovernorOptions;
  plain: boolean;
  /** Serve this machine's devices to pipelines. Off makes it a solo node only. */
  rpcEnabled: boolean;
  rpcPort: number;
  rpcHost: string | null;
  rpcAdvertise: string | null;
  /** Restrict exposed devices, e.g. "CPU" to keep the GPU for the owner. */
  rpcDevice: string | null;
  /** Local llama-server control port, bound to loopback. */
  serverPort: number;
  llamaVariant: LlamaVariant;
  /** Refuse to be a pipeline head, e.g. on a metered or slow uplink. */
  noHead: boolean;
}

const HELP = `
Community AI Worker

Usage
  community-ai-worker [options]

Connection
  --coordinator <url>     Coordinator base URL            (default http://localhost:8787)
  --token <secret>        Join token, if the network sets one
  --name <label>          How this machine appears        (default hostname)

Model
  --model <id>            Preload one model at startup    (${MODEL_CATALOG.map((m) => m.id).join(", ")})
  --model-uri <uri>       Override the GGUF source, e.g. a local .gguf path
  --models-dir <path>     Where weights are cached        (default ./models)
  --gpu-layers <n>        Layers to offload to the GPU    (default: let llama.cpp decide)
  --context <n>           Context window                  (default 4096)

Pipeline (layer-splitting: hold part of a model that is too big for one machine)
  --rpc-port <n>          Port that serves this machine's devices  (default 50052)
  --rpc-host <ip>         Address to bind    (default: your Tailscale or LAN address)
  --rpc-advertise <ip>    Address peers dial, if it differs from the bind address
  --rpc-device <list>     Expose only these devices, e.g. CPU
  --no-rpc                Do not contribute memory to pipelines
  --no-head               Contribute layers, but never drive a pipeline
  --server-port <n>       Local llama-server port, loopback only   (default 8080)
  --llama-variant <v>     auto | vulkan | cpu | cuda | rocm        (default auto)

Resource Governor (all limits are enforced locally, the coordinator cannot override them)
  --max-capacity <0-1>    Ceiling on spare capacity offered   (default ${defaultGovernorOptions.maxCapacity})
  --throttle-cpu <pct>    Owner CPU load that starts backoff  (default ${defaultGovernorOptions.throttleCpuPct})
  --pause-cpu <pct>       Owner CPU load that stops work      (default ${defaultGovernorOptions.pauseCpuPct})
  --min-free-memory <mb>  Never leave the owner below this    (default ${defaultGovernorOptions.minFreeMemoryMB})
  --max-model-memory <mb> Hard cap on resident weights        (default: derived from hardware)
  --concurrency <n>       Tasks at once                       (default ${defaultGovernorOptions.maxConcurrentTasks})
  --run-on-battery        Contribute while on battery         (default: pause on battery)
  --battery-capacity <n>  Ceiling while discharging, 0-1      (default ${defaultGovernorOptions.batteryCapacity})

Display
  --plain                 Log lines instead of a live status panel
  --help                  Show this message

While running:  p = pause / resume contribution,  q = quit
`;

function readFlag(argv: string[], name: string): boolean {
  return argv.includes(`--${name}`);
}

function readValue(argv: string[], name: string): string | null {
  const index = argv.indexOf(`--${name}`);
  if (index === -1) return null;
  return argv[index + 1] ?? null;
}

function readNumber(argv: string[], name: string, fallback: number): number {
  const raw = readValue(argv, name);
  if (raw === null) return fallback;
  const value = Number.parseFloat(raw);
  return Number.isFinite(value) ? value : fallback;
}

export function parseArgs(argv: string[]): WorkerArgs | null {
  if (readFlag(argv, "help") || argv.includes("-h")) {
    process.stdout.write(HELP);
    return null;
  }

  const governor: GovernorOptions = {
    ...defaultGovernorOptions,
    maxCapacity: Math.min(
      1,
      Math.max(0, readNumber(argv, "max-capacity", defaultGovernorOptions.maxCapacity))
    ),
    throttleCpuPct: readNumber(argv, "throttle-cpu", defaultGovernorOptions.throttleCpuPct),
    pauseCpuPct: readNumber(argv, "pause-cpu", defaultGovernorOptions.pauseCpuPct),
    minFreeMemoryMB: readNumber(argv, "min-free-memory", defaultGovernorOptions.minFreeMemoryMB),
    maxModelMemoryMB: readNumber(argv, "max-model-memory", 0),
    maxConcurrentTasks: Math.max(
      1,
      Math.round(readNumber(argv, "concurrency", defaultGovernorOptions.maxConcurrentTasks))
    ),
    pauseOnBattery: !readFlag(argv, "run-on-battery"),
    batteryCapacity: Math.min(
      1,
      Math.max(0, readNumber(argv, "battery-capacity", defaultGovernorOptions.batteryCapacity))
    ),
  };

  const gpuLayersRaw = readValue(argv, "gpu-layers");

  return {
    coordinator: (
      readValue(argv, "coordinator") ??
      process.env.COORDINATOR_URL ??
      "http://localhost:8787"
    ).replace(/\/+$/, ""),
    token: readValue(argv, "token") ?? process.env.JOIN_TOKEN ?? "",
    label: readValue(argv, "name") ?? process.env.WORKER_NAME ?? defaultNodeLabel(),
    modelId: readValue(argv, "model"),
    modelUri: readValue(argv, "model-uri"),
    modelsDir: readValue(argv, "models-dir") ?? "./models",
    gpuLayers: gpuLayersRaw === null ? undefined : Number.parseInt(gpuLayersRaw, 10),
    contextSize: Math.round(readNumber(argv, "context", 4096)),
    governor,
    plain: readFlag(argv, "plain"),
    rpcEnabled: !readFlag(argv, "no-rpc"),
    rpcPort: Math.round(readNumber(argv, "rpc-port", 50052)),
    rpcHost: readValue(argv, "rpc-host"),
    rpcAdvertise: readValue(argv, "rpc-advertise"),
    rpcDevice: readValue(argv, "rpc-device"),
    serverPort: Math.round(readNumber(argv, "server-port", 8080)),
    llamaVariant: (readValue(argv, "llama-variant") ?? "auto") as LlamaVariant,
    noHead: readFlag(argv, "no-head"),
  };
}

export function toWebSocketUrl(base: string): string {
  const url = new URL(base);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  url.pathname = "/ws/worker";
  url.search = "";
  return url.toString();
}

/* -------------------------------------------------------------------- *
 * Live status panel
 * -------------------------------------------------------------------- */

export interface StatusModel {
  coordinator: string;
  connection: string;
  connected: boolean;
  label: string;
  engine: string;
  device: string;
  vram: string;
  cpuOwnerPct: number;
  gpuUsedPct: number | null;
  memory: string;
  governorState: string;
  capacityPct: number;
  reasons: string[];
  model: string;
  modelPhase: string;
  activeTasks: number;
  tasksCompleted: number;
  tokensPerSecond: number | null;
  paused: boolean;
  lastEvent: string;
  /** `host:port` this machine serves its devices on, or why it is not serving. */
  rpcEndpoint: string | null;
  rpcSummary: string;
  /** Describes the pipeline this node is currently head of, if any. */
  pipeline: string | null;
}

const ESC = "\u001b";
const dim = (s: string) => `${ESC}[2m${s}${ESC}[0m`;
const bold = (s: string) => `${ESC}[1m${s}${ESC}[0m`;
const green = (s: string) => `${ESC}[32m${s}${ESC}[0m`;
const yellow = (s: string) => `${ESC}[33m${s}${ESC}[0m`;
const red = (s: string) => `${ESC}[31m${s}${ESC}[0m`;

function bar(fraction: number, width = 18): string {
  const filled = Math.round(Math.min(1, Math.max(0, fraction)) * width);
  return `${"█".repeat(filled)}${"░".repeat(width - filled)}`;
}

export class StatusPanel {
  private lastLineCount = 0;
  private lastPlainLine = "";

  constructor(private readonly plain: boolean) {}

  render(status: StatusModel): void {
    if (this.plain) {
      // Log on change rather than every tick, so piping to a file stays readable.
      const line =
        `${status.connected ? "connected" : status.connection} ` +
        `governor=${status.governorState} capacity=${status.capacityPct}% ` +
        `active=${status.activeTasks} model=${status.model}/${status.modelPhase}` +
        `${status.reasons.length ? ` reasons=${status.reasons.join("; ")}` : ""}` +
        ` | ${status.lastEvent}`;
      if (line !== this.lastPlainLine) {
        this.lastPlainLine = line;
        console.log(`[worker] ${line}`);
      }
      return;
    }

    const contribution = status.paused
      ? red("PAUSED")
      : status.capacityPct <= 0
        ? yellow("HOLDING")
        : status.activeTasks > 0
          ? green("ACTIVE")
          : green("READY");

    const lines = [
      "",
      `  ${bold("Community AI Worker")}   ${dim(status.label)}`,
      "",
      `  Status        ${status.connected ? green("Connected") : yellow(status.connection)}   ${dim(status.coordinator)}`,
      `  Engine        ${status.engine}`,
      `  Device        ${status.device}`,
      `  VRAM          ${status.vram}`,
      `  Memory        ${status.memory}`,
      "",
      `  Owner CPU     ${bar(status.cpuOwnerPct / 100)} ${String(Math.round(status.cpuOwnerPct)).padStart(3)}%`,
      status.gpuUsedPct === null
        ? `  GPU load      ${dim("n/a")}`
        : `  GPU load      ${bar(status.gpuUsedPct / 100)} ${String(Math.round(status.gpuUsedPct)).padStart(3)}%`,
      `  Capacity      ${bar(status.capacityPct / 100)} ${String(status.capacityPct).padStart(3)}%   ${dim(status.governorState)}`,
      "",
      `  Contribution  ${contribution}`,
      `  Model         ${status.model} ${dim(status.modelPhase)}`,
      `  Serving       ${status.rpcEndpoint ? `${green(status.rpcEndpoint)} ${dim(status.rpcSummary)}` : dim(status.rpcSummary)}`,
      status.pipeline ? `  Pipeline      ${status.pipeline}` : `  Pipeline      ${dim("idle")}`,
      `  Tasks         ${status.activeTasks} running, ${status.tasksCompleted} done` +
        (status.tokensPerSecond ? dim(`   ${status.tokensPerSecond.toFixed(1)} tok/s avg`) : ""),
      status.reasons.length > 0 ? `  Governor      ${dim(status.reasons.join("; "))}` : `  Governor      ${dim("no limits active")}`,
      "",
      `  ${dim(status.lastEvent)}`,
      `  ${dim("p = pause/resume    q = quit")}`,
      "",
    ];

    if (this.lastLineCount > 0) {
      process.stdout.write(`${ESC}[${this.lastLineCount}A${ESC}[0J`);
    }
    process.stdout.write(`${lines.join("\n")}\n`);
    this.lastLineCount = lines.length + 1;
  }

  note(message: string): void {
    if (this.plain) {
      console.log(`[worker] ${message}`);
      return;
    }
    this.lastLineCount = 0;
    process.stdout.write(`${message}\n`);
  }
}
