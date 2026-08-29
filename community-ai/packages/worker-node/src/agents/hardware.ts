import { execFile } from "node:child_process";
import { readFile, readdir } from "node:fs/promises";
import { arch, cpus, freemem, hostname, platform, release, totalmem } from "node:os";
import { promisify } from "node:util";
import type { AcceleratorBackend, GpuProfile, ThermalState } from "@community-ai/protocol";

const run = promisify(execFile);

/**
 * Hardware Agent.
 *
 * Reports what the machine is, and crucially how much of it the *owner* is
 * currently using. Owner load is total system busy minus this worker's own
 * CPU time, so contributing work never looks like the owner being busy and
 * never causes the governor to throttle itself into a loop.
 */

export interface CpuSample {
  totalBusyPct: number;
  ownerBusyPct: number;
}

interface CpuTimes {
  idle: number;
  total: number;
}

function readCpuTimes(): CpuTimes {
  let idle = 0;
  let total = 0;
  for (const cpu of cpus()) {
    idle += cpu.times.idle;
    total += cpu.times.user + cpu.times.nice + cpu.times.sys + cpu.times.idle + cpu.times.irq;
  }
  return { idle, total };
}

export class CpuMonitor {
  private lastTimes = readCpuTimes();
  private lastProcess = process.cpuUsage();
  private lastAtMs = Date.now();
  private readonly coreCount = Math.max(cpus().length, 1);

  sample(): CpuSample {
    const now = Date.now();
    const times = readCpuTimes();
    const proc = process.cpuUsage();

    const idleDelta = times.idle - this.lastTimes.idle;
    const totalDelta = times.total - this.lastTimes.total;
    const wallMs = Math.max(now - this.lastAtMs, 1);
    // cpuUsage is microseconds of CPU across all cores.
    const procMs = (proc.user - this.lastProcess.user + (proc.system - this.lastProcess.system)) / 1000;

    this.lastTimes = times;
    this.lastProcess = proc;
    this.lastAtMs = now;

    if (totalDelta <= 0) return { totalBusyPct: 0, ownerBusyPct: 0 };

    const totalBusyPct = Math.min(100, Math.max(0, (1 - idleDelta / totalDelta) * 100));
    const ourPct = Math.min(100, (procMs / (wallMs * this.coreCount)) * 100);
    return {
      totalBusyPct,
      ownerBusyPct: Math.min(100, Math.max(0, totalBusyPct - ourPct)),
    };
  }
}

function cpuModel(): string {
  const list = cpus();
  return list[0]?.model?.trim().replace(/\s+/g, " ") ?? "Unknown CPU";
}

export interface HardwareSnapshot {
  cpuModel: string;
  cpuCores: number;
  totalMemoryMB: number;
  availableMemoryMB: number;
  gpu: GpuProfile | null;
  thermalState: ThermalState;
  onBattery: boolean;
  batteryPct: number | null;
}

async function readNvidia(): Promise<Partial<GpuProfile> & { temperature?: number } | null> {
  try {
    const { stdout } = await run(
      "nvidia-smi",
      [
        "--query-gpu=name,memory.total,memory.used,utilization.gpu,temperature.gpu",
        "--format=csv,noheader,nounits",
      ],
      { timeout: 4000 }
    );
    const line = stdout.trim().split("\n")[0];
    if (!line) return null;
    const parts = line.split(",").map((p) => p.trim());
    const name = parts[0] ?? "NVIDIA GPU";
    const total = Number.parseFloat(parts[1] ?? "0");
    const used = Number.parseFloat(parts[2] ?? "0");
    const util = Number.parseFloat(parts[3] ?? "0");
    const temp = Number.parseFloat(parts[4] ?? "0");
    if (!Number.isFinite(total) || total <= 0) return null;
    const memFree = Math.max(0, (total - used) / total);
    const computeFree = Math.max(0, 1 - util / 100);
    return {
      vendor: "NVIDIA",
      model: name,
      vram: Math.round(total),
      available: Math.min(memFree, computeFree),
      backend: "cuda",
      temperature: Number.isFinite(temp) ? temp : undefined,
    };
  } catch {
    return null;
  }
}

async function readLinuxThermal(): Promise<number | null> {
  try {
    const zones = await readdir("/sys/class/thermal");
    let hottest = 0;
    for (const zone of zones) {
      if (!zone.startsWith("thermal_zone")) continue;
      try {
        const raw = await readFile(`/sys/class/thermal/${zone}/temp`, "utf8");
        const milli = Number.parseInt(raw.trim(), 10);
        if (Number.isFinite(milli)) hottest = Math.max(hottest, milli / 1000);
      } catch {
        /* zone unreadable */
      }
    }
    return hottest > 0 ? hottest : null;
  } catch {
    return null;
  }
}

async function readLinuxBattery(): Promise<{ onBattery: boolean; pct: number | null }> {
  try {
    const entries = await readdir("/sys/class/power_supply");
    for (const entry of entries) {
      if (!entry.startsWith("BAT")) continue;
      const [capacityRaw, statusRaw] = await Promise.all([
        readFile(`/sys/class/power_supply/${entry}/capacity`, "utf8").catch(() => ""),
        readFile(`/sys/class/power_supply/${entry}/status`, "utf8").catch(() => ""),
      ]);
      const pct = Number.parseInt(capacityRaw.trim(), 10);
      const status = statusRaw.trim().toLowerCase();
      return {
        onBattery: status === "discharging",
        pct: Number.isFinite(pct) ? pct : null,
      };
    }
  } catch {
    /* no battery subsystem */
  }
  return { onBattery: false, pct: null };
}

function classifyThermal(celsius: number | null): ThermalState {
  if (celsius === null) return "unknown";
  if (celsius >= 92) return "critical";
  if (celsius >= 82) return "hot";
  if (celsius >= 70) return "warm";
  return "normal";
}

/**
 * VRAM reported by the inference runtime is authoritative when available,
 * because it accounts for the backend actually in use (CUDA/Vulkan/Metal).
 */
export interface RuntimeGpuHint {
  backend: AcceleratorBackend;
  totalVramMB: number;
  freeVramMB: number;
  name: string;
}

export class HardwareAgent {
  private cpu = new CpuMonitor();
  private lastSample: CpuSample = { totalBusyPct: 0, ownerBusyPct: 0 };

  async snapshot(runtimeHint: RuntimeGpuHint | null): Promise<HardwareSnapshot> {
    this.lastSample = this.cpu.sample();

    const nvidia = await readNvidia();
    let gpu: GpuProfile | null = null;

    if (runtimeHint && runtimeHint.totalVramMB > 0) {
      gpu = {
        vendor: nvidia?.vendor ?? vendorFromBackend(runtimeHint.backend),
        model: nvidia?.model ?? runtimeHint.name,
        vram: Math.round(runtimeHint.totalVramMB),
        available: clamp01(runtimeHint.freeVramMB / Math.max(runtimeHint.totalVramMB, 1)),
        backend: runtimeHint.backend,
      };
      // nvidia-smi also knows compute utilisation, which VRAM state cannot see.
      if (nvidia?.available !== undefined) {
        gpu.available = Math.min(gpu.available, nvidia.available);
      }
    } else if (nvidia && nvidia.vram) {
      gpu = {
        vendor: nvidia.vendor ?? "NVIDIA",
        model: nvidia.model ?? "NVIDIA GPU",
        vram: nvidia.vram,
        available: nvidia.available ?? 0.8,
        backend: "cuda",
      };
    }

    const thermalC =
      nvidia?.temperature ?? (platform() === "linux" ? await readLinuxThermal() : null);
    const battery =
      platform() === "linux" ? await readLinuxBattery() : { onBattery: false, pct: null };

    return {
      cpuModel: cpuModel(),
      cpuCores: Math.max(cpus().length, 1),
      totalMemoryMB: Math.round(totalmem() / 1024 / 1024),
      availableMemoryMB: Math.round(freemem() / 1024 / 1024),
      gpu,
      thermalState: classifyThermal(thermalC),
      onBattery: battery.onBattery,
      batteryPct: battery.pct,
    };
  }

  get ownerBusyPct(): number {
    return this.lastSample.ownerBusyPct;
  }

  get totalBusyPct(): number {
    return this.lastSample.totalBusyPct;
  }
}

function vendorFromBackend(backend: AcceleratorBackend): string {
  switch (backend) {
    case "cuda":
      return "NVIDIA";
    case "metal":
      return "Apple";
    case "vulkan":
      return "Vulkan";
    default:
      return "Unknown";
  }
}

function clamp01(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.min(1, Math.max(0, value));
}

export function defaultNodeLabel(): string {
  return hostname();
}

export function platformInfo(): { os: string; arch: string; version: string } {
  return { os: platform(), arch: arch(), version: release() };
}
