import type { CapabilityProfile, GpuProfile } from "@community-ai/protocol";

/**
 * Browser Hardware Agent.
 *
 * The web platform deliberately hides most hardware detail, so this reports
 * what it can actually observe and is explicit about the rest. In particular
 * WebGPU exposes no VRAM figure at all, so a browser contributor advertises a
 * memory *budget it is willing to commit* rather than pretending to know the
 * size of the card. That is the honest reading of "what can this node
 * contribute right now".
 */

export interface BrowserGpuInfo {
  available: boolean;
  shaderF16: boolean;
  vendor: string;
  architecture: string;
  description: string;
  reason: string;
}

export async function probeGpu(): Promise<BrowserGpuInfo> {
  const gpu = (navigator as Navigator & { gpu?: GPU }).gpu;
  const empty = {
    available: false,
    shaderF16: false,
    vendor: "",
    architecture: "",
    description: "",
  };
  if (!gpu) {
    return {
      ...empty,
      reason: diagnoseMissingWebGpu(),
    };
  }
  try {
    const adapter = await gpu.requestAdapter({ powerPreference: "high-performance" });
    if (!adapter) {
      return {
        ...empty,
        reason:
          "This browser supports WebGPU, but no GPU adapter was returned. Common causes: remote desktop, " +
          "a disabled GPU, outdated drivers, or Firefox with webgpu disabled. Try Chrome/Edge, or run the native worker.",
      };
    }
    const info = (adapter as GPUAdapter & { info?: GPUAdapterInfo }).info;
    return {
      available: true,
      shaderF16: adapter.features.has("shader-f16"),
      vendor: info?.vendor ?? "unknown",
      architecture: info?.architecture ?? "",
      description: info?.description || info?.architecture || "WebGPU device",
      reason: "",
    };
  } catch (err) {
    return { ...empty, reason: err instanceof Error ? err.message : String(err) };
  }
}

/**
 * Browser contribution needs WebGPU, and browsers only expose WebGPU in a
 * *secure context* (HTTPS or localhost). Opening http://192.168.x.x from another
 * PC is the most common reason friends see "No WebGPU" even in current Chrome.
 */
function diagnoseMissingWebGpu(): string {
  const host = typeof location !== "undefined" ? location.hostname : "";
  const insecureLan =
    typeof window !== "undefined" &&
    !window.isSecureContext &&
    host !== "localhost" &&
    host !== "127.0.0.1";

  if (insecureLan) {
    return (
      `This page is on http://${host} — browsers block WebGPU outside HTTPS/localhost. ` +
      "Open the https:// URL of the coordinator instead (accept the one-time certificate warning), " +
      "or run the native desktop worker on that PC."
    );
  }

  const ua = navigator.userAgent;
  if (/Firefox\//.test(ua)) {
    return (
      "Firefox does not enable WebGPU for this yet. Open this page in Chrome or Edge 113+, " +
      "or run the native desktop worker on that PC (recommended)."
    );
  }
  if (/Edg\//.test(ua) || /Chrome\//.test(ua) || /CriOS\//.test(ua)) {
    return (
      "Chrome/Edge is installed, but WebGPU is not available here. Update to 113+, check that " +
      "hardware acceleration is on, or run the native desktop worker instead."
    );
  }
  if (/Safari\//.test(ua) && !/Chrome\//.test(ua)) {
    return (
      "Safari only exposes WebGPU on recent macOS / iOS 18+. Update Safari, or run the " +
      "native desktop worker on a Mac."
    );
  }
  return (
    "No WebGPU in this browser. Browser contribution needs Chrome/Edge 113+, Chrome on Android, " +
    "or Safari on iOS 18+. On a desktop PC, prefer the native worker."
  );
}

export function deviceMemoryGB(): number | null {
  const value = (navigator as Navigator & { deviceMemory?: number }).deviceMemory;
  return typeof value === "number" && value > 0 ? value : null;
}

/** Conservative default for how much this tab may spend on weights. */
export function defaultBudgetMB(): number {
  const gb = deviceMemoryGB();
  if (gb === null) return 2048;
  return Math.round(Math.min(Math.max(gb * 1024 * 0.3, 1024), 6144));
}

export function isMobileLike(): boolean {
  const ua = navigator.userAgent;
  if (/Android|iPhone|iPad|iPod/i.test(ua)) return true;
  const touchPoints = (navigator as Navigator & { maxTouchPoints?: number }).maxTouchPoints ?? 0;
  return touchPoints > 1 && Math.min(window.screen.width, window.screen.height) < 820;
}

export function osName(): string {
  const ua = navigator.userAgent;
  if (/Android/i.test(ua)) return "android";
  if (/iPhone|iPad|iPod/i.test(ua)) return "ios";
  if (/Mac OS X/i.test(ua)) return "macos";
  if (/Windows/i.test(ua)) return "windows";
  if (/Linux/i.test(ua)) return "linux";
  return "browser";
}

export interface BuildProfileInput {
  nodeId: string;
  label: string;
  gpuInfo: BrowserGpuInfo;
  budgetMB: number;
  /** 0..1, from the frame-timing monitor. */
  ownerBusy: number;
  memoryUsedMB: number;
  latencyMs: number;
  jitterMs: number;
  battery: { onBattery: boolean; level: number | null };
  loadedModels: string[];
  supportedModels: string[];
  runtimeReady: boolean;
}

export function buildBrowserProfile(input: BuildProfileInput): CapabilityProfile {
  const gpu: GpuProfile | null = input.gpuInfo.available
    ? {
        vendor: input.gpuInfo.vendor || "unknown",
        // The description is all WebGPU will tell us about the actual chip.
        model: input.gpuInfo.description || "WebGPU device",
        vram: input.budgetMB,
        available: Math.max(0, Math.min(1, 1 - input.memoryUsedMB / Math.max(input.budgetMB, 1))),
        backend: "webgpu",
      }
    : null;

  const totalMemoryMB = (deviceMemoryGB() ?? 4) * 1024;

  return {
    nodeId: input.nodeId,
    label: input.label,
    kind: "browser-contributor",
    platform: {
      os: osName(),
      arch: isMobileLike() ? "mobile" : "desktop",
      version: navigator.userAgent.slice(0, 80),
    },
    cpu: {
      model: `${navigator.hardwareConcurrency ?? 2} logical cores (browser)`,
      cores: navigator.hardwareConcurrency ?? 2,
      available: Math.max(0, Math.min(1, 1 - input.ownerBusy)),
    },
    gpu,
    memory: {
      total: Math.round(totalMemoryMB),
      available: Math.max(0, Math.round(input.budgetMB - input.memoryUsedMB)),
    },
    network: { latency: input.latencyMs, bandwidthMbps: 0, jitter: input.jitterMs },
    userState: {
      activity:
        input.ownerBusy < 0.1
          ? "idle"
          : input.ownerBusy < 0.35
            ? "light"
            : input.ownerBusy < 0.7
              ? "active"
              : "busy",
      thermalState: "unknown",
      onBattery: input.battery.onBattery,
      batteryPct: input.battery.level === null ? null : Math.round(input.battery.level * 100),
    },
    runtime: {
      engine: "webllm",
      ready: input.runtimeReady,
      loadedModels: input.loadedModels,
      supportedModels: input.supportedModels,
      // A tab cannot expose its GPU as a raw device to a remote process, so a
      // browser contributor can run whole small models but can never hold a
      // slice of a large one.
      rpc: null,
    },
  };
}
