import type { GovernorReport, GovernorState, UserActivity } from "@community-ai/protocol";
import type { HardwareSnapshot } from "./hardware.js";

/**
 * Resource Governor.
 *
 * This is the owner's side of the contract and it lives inside the worker, never
 * on the coordinator. The coordinator decides WHAT a node should do; the
 * governor decides HOW MUCH of the machine may be used to do it, and can refuse
 * outright.
 *
 *   owner idle          -> capacity climbs toward the configured ceiling
 *   owner opens an IDE  -> capacity steps down
 *   owner starts a game -> capacity goes to zero, running work is abandoned
 *   owner stops         -> capacity is restored gradually, never in one jump
 *
 * Capacity rises slowly and falls quickly on purpose: being a little slow to
 * reclaim spare cycles is invisible, being slow to release them is not.
 */

export interface GovernorOptions {
  /** Owner CPU load above which we start backing off, in percent. */
  throttleCpuPct: number;
  /** Owner CPU load above which we stop entirely, in percent. */
  pauseCpuPct: number;
  /** Stop if free system memory drops below this, in MB. */
  minFreeMemoryMB: number;
  /** Never hold more than this much model weight, in MB. 0 = derive from hardware. */
  maxModelMemoryMB: number;
  /** Ceiling on capacity even when the machine is completely idle, 0..1. */
  maxCapacity: number;
  maxConcurrentTasks: number;
  pauseOnBattery: boolean;
  batteryFloorPct: number;
  /** Capacity ceiling while discharging, when contributing on battery is allowed. */
  batteryCapacity: number;
}

export const defaultGovernorOptions: GovernorOptions = {
  throttleCpuPct: 55,
  pauseCpuPct: 80,
  minFreeMemoryMB: 1536,
  maxModelMemoryMB: 0,
  maxCapacity: 0.8,
  maxConcurrentTasks: 1,
  pauseOnBattery: true,
  batteryFloorPct: 25,
  batteryCapacity: 0.3,
};

const RAMP_UP_PER_TICK = 0.12;
const RAMP_DOWN_PER_TICK = 0.5;

export class ResourceGovernor {
  private capacity = 0;
  private state: GovernorState = "idle";
  private manualPause = false;
  private reasons: string[] = [];

  constructor(private options: GovernorOptions = defaultGovernorOptions) {}

  setManualPause(paused: boolean): void {
    this.manualPause = paused;
    if (paused) {
      this.capacity = 0;
      this.state = "paused";
      this.reasons = ["owner paused contribution"];
    }
  }

  get isManuallyPaused(): boolean {
    return this.manualPause;
  }

  update(hardware: HardwareSnapshot, ownerBusyPct: number, runtimeReady: boolean): GovernorReport {
    const reasons: string[] = [];
    let target = this.options.maxCapacity;

    if (this.manualPause) {
      this.capacity = 0;
      this.state = "paused";
      this.reasons = ["owner paused contribution"];
      return this.report(hardware);
    }

    if (!runtimeReady) {
      reasons.push("inference runtime not ready");
      target = 0;
    }

    if (ownerBusyPct >= this.options.pauseCpuPct) {
      reasons.push(`owner is using ${Math.round(ownerBusyPct)}% CPU`);
      target = 0;
    } else if (ownerBusyPct >= this.options.throttleCpuPct) {
      const span = this.options.pauseCpuPct - this.options.throttleCpuPct || 1;
      const overshoot = (ownerBusyPct - this.options.throttleCpuPct) / span;
      reasons.push(`owner CPU at ${Math.round(ownerBusyPct)}%`);
      target = Math.min(target, this.options.maxCapacity * (1 - overshoot) * 0.6);
    }

    if (hardware.availableMemoryMB < this.options.minFreeMemoryMB) {
      reasons.push(
        `only ${hardware.availableMemoryMB} MB RAM free (floor ${this.options.minFreeMemoryMB} MB)`
      );
      target = 0;
    } else if (hardware.availableMemoryMB < this.options.minFreeMemoryMB * 2) {
      reasons.push("system memory is tight");
      target = Math.min(target, 0.3);
    }

    switch (hardware.thermalState) {
      case "critical":
        reasons.push("thermal state critical");
        target = 0;
        break;
      case "hot":
        reasons.push("running hot");
        target = Math.min(target, 0.25);
        break;
      case "warm":
        target = Math.min(target, this.options.maxCapacity * 0.75);
        break;
      default:
        break;
    }

    if (hardware.onBattery) {
      if (this.options.pauseOnBattery) {
        reasons.push("on battery power");
        target = 0;
      } else if (hardware.batteryPct !== null && hardware.batteryPct <= this.options.batteryFloorPct) {
        reasons.push(`battery at ${hardware.batteryPct}%`);
        target = 0;
      } else {
        reasons.push(`on battery, capped at ${Math.round(this.options.batteryCapacity * 100)}%`);
        target = Math.min(target, this.options.batteryCapacity);
      }
    }

    target = Math.max(0, Math.min(target, this.options.maxCapacity));

    if (target > this.capacity) {
      this.capacity = Math.min(target, this.capacity + RAMP_UP_PER_TICK);
      this.state = this.capacity < target ? "resuming" : "available";
    } else if (target < this.capacity) {
      this.capacity = Math.max(target, this.capacity - RAMP_DOWN_PER_TICK);
      this.state = this.capacity <= 0.01 ? "paused" : "throttling";
    } else {
      this.state = this.capacity <= 0.01 ? "paused" : "available";
    }

    if (this.capacity <= 0.01) {
      this.capacity = 0;
      this.state = "paused";
    }

    this.reasons = reasons;
    return this.report(hardware);
  }

  /** Called by the Worker Scheduler while work is actually executing. */
  markContributing(active: boolean): void {
    if (this.state === "available" && active) this.state = "contributing";
  }

  private report(hardware: HardwareSnapshot): GovernorReport {
    return {
      state: this.state,
      capacity: Math.round(this.capacity * 100) / 100,
      maxConcurrentTasks: this.capacity <= 0 ? 0 : this.options.maxConcurrentTasks,
      maxModelMemoryMB: this.modelMemoryCeiling(hardware),
      reasons: this.reasons,
      manualPause: this.manualPause,
    };
  }

  private modelMemoryCeiling(hardware: HardwareSnapshot): number {
    if (this.options.maxModelMemoryMB > 0) return this.options.maxModelMemoryMB;
    if (hardware.gpu && hardware.gpu.vram > 0) {
      // Leave a slice of VRAM for the owner's desktop compositor and browser.
      return Math.max(0, hardware.gpu.vram * hardware.gpu.available * 0.85);
    }
    return Math.max(0, hardware.availableMemoryMB - this.options.minFreeMemoryMB);
  }
}

export function classifyActivity(ownerBusyPct: number): UserActivity {
  if (ownerBusyPct < 10) return "idle";
  if (ownerBusyPct < 35) return "light";
  if (ownerBusyPct < 70) return "active";
  return "busy";
}
