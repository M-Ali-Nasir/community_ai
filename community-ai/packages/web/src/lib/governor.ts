import type { GovernorReport, GovernorState } from "@community-ai/protocol";

/**
 * Browser Resource Governor.
 *
 * Same contract as the desktop worker: the owner's device decides how much it
 * will give, and the coordinator cannot override it. The signals available in a
 * browser are different, so this measures what it actually can:
 *
 *   main-thread jank  the inference itself runs in a Web Worker, so stalls on
 *                     the main thread are the *owner* doing something, which is
 *                     exactly the signal we want
 *   tab visibility    a backgrounded tab gets throttled by the OS anyway, and a
 *                     phone may suspend it outright
 *   battery           discharging phones stop contributing
 */

const TARGET_FRAME_MS = 1000 / 60;

export class FrameMonitor {
  private samples: number[] = [];
  private last = performance.now();
  private running = false;

  start(): void {
    if (this.running) return;
    this.running = true;
    const tick = () => {
      if (!this.running) return;
      const now = performance.now();
      this.samples.push(now - this.last);
      this.last = now;
      if (this.samples.length > 60) this.samples.shift();
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  }

  stop(): void {
    this.running = false;
  }

  /** 0 = smooth, 1 = the main thread is saturated. */
  get ownerBusy(): number {
    if (this.samples.length < 10) return 0;
    const sorted = [...this.samples].sort((a, b) => a - b);
    const median = sorted[Math.floor(sorted.length / 2)] ?? TARGET_FRAME_MS;
    // A hidden tab stops painting entirely; that is not the owner being busy.
    if (document.visibilityState === "hidden") return 0;
    const overshoot = (median - TARGET_FRAME_MS) / (TARGET_FRAME_MS * 3);
    return Math.max(0, Math.min(1, overshoot));
  }
}

export interface BatteryState {
  onBattery: boolean;
  level: number | null;
}

export async function watchBattery(onChange: (state: BatteryState) => void): Promise<void> {
  const nav = navigator as Navigator & {
    getBattery?: () => Promise<{
      charging: boolean;
      level: number;
      addEventListener: (type: string, handler: () => void) => void;
    }>;
  };
  if (!nav.getBattery) {
    onChange({ onBattery: false, level: null });
    return;
  }
  try {
    const battery = await nav.getBattery();
    const emit = () => onChange({ onBattery: !battery.charging, level: battery.level });
    battery.addEventListener("levelchange", emit);
    battery.addEventListener("chargingchange", emit);
    emit();
  } catch {
    onChange({ onBattery: false, level: null });
  }
}

export interface BrowserGovernorOptions {
  maxCapacity: number;
  budgetMB: number;
  pauseWhenHidden: boolean;
  pauseOnBattery: boolean;
}

export const defaultBrowserGovernorOptions: BrowserGovernorOptions = {
  maxCapacity: 0.6,
  budgetMB: 2048,
  // A hidden tab on a phone can be killed mid-task, so stopping is the honest default.
  pauseWhenHidden: true,
  pauseOnBattery: true,
};

const RAMP_UP = 0.15;
const RAMP_DOWN = 0.5;

export class BrowserGovernor {
  private capacity = 0;
  private state: GovernorState = "idle";
  private manualPause = true; // contributing is opt-in
  private reasons: string[] = ["contribution is off"];

  constructor(private options: BrowserGovernorOptions = defaultBrowserGovernorOptions) {}

  setOptions(options: Partial<BrowserGovernorOptions>): void {
    this.options = { ...this.options, ...options };
  }

  setManualPause(paused: boolean): void {
    this.manualPause = paused;
    if (paused) {
      this.capacity = 0;
      this.state = "paused";
      this.reasons = ["contribution is off"];
    }
  }

  get isPaused(): boolean {
    return this.manualPause;
  }

  update(input: {
    ownerBusy: number;
    battery: BatteryState;
    hidden: boolean;
    runtimeReady: boolean;
  }): GovernorReport {
    if (this.manualPause) {
      this.capacity = 0;
      this.state = "paused";
      this.reasons = ["contribution is off"];
      return this.report();
    }

    const reasons: string[] = [];
    let target = this.options.maxCapacity;

    if (!input.runtimeReady) {
      reasons.push("model not loaded yet");
      target = 0;
    }
    if (input.hidden && this.options.pauseWhenHidden) {
      reasons.push("tab is in the background");
      target = 0;
    }
    if (input.battery.onBattery && this.options.pauseOnBattery) {
      reasons.push("running on battery");
      target = 0;
    } else if (input.battery.onBattery && input.battery.level !== null && input.battery.level < 0.3) {
      reasons.push(`battery at ${Math.round(input.battery.level * 100)}%`);
      target = 0;
    }
    if (input.ownerBusy > 0.5) {
      reasons.push("the page is busy, backing off");
      target = Math.min(target, this.options.maxCapacity * 0.25);
    } else if (input.ownerBusy > 0.2) {
      target = Math.min(target, this.options.maxCapacity * 0.6);
    }

    target = Math.max(0, Math.min(target, this.options.maxCapacity));

    if (target > this.capacity) {
      this.capacity = Math.min(target, this.capacity + RAMP_UP);
      this.state = this.capacity < target ? "resuming" : "available";
    } else if (target < this.capacity) {
      this.capacity = Math.max(target, this.capacity - RAMP_DOWN);
      this.state = this.capacity <= 0.01 ? "paused" : "throttling";
    } else {
      this.state = this.capacity <= 0.01 ? "paused" : "available";
    }

    if (this.capacity <= 0.01) {
      this.capacity = 0;
      this.state = "paused";
    }

    this.reasons = reasons;
    return this.report();
  }

  markContributing(active: boolean): void {
    if (active && this.state === "available") this.state = "contributing";
  }

  private report(): GovernorReport {
    return {
      state: this.state,
      capacity: Math.round(this.capacity * 100) / 100,
      // A tab is single-GPU and unpredictable; never take more than one task.
      maxConcurrentTasks: this.capacity > 0 ? 1 : 0,
      maxModelMemoryMB: this.options.budgetMB,
      reasons: this.reasons,
      manualPause: this.manualPause,
    };
  }
}
