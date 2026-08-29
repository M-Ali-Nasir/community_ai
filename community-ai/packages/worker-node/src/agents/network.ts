import type { NetworkProfile } from "@community-ai/protocol";

/**
 * Network Agent.
 *
 * Latency and jitter come from real round-trips against the coordinator; the
 * coordinator also measures them independently and its numbers win in the
 * registry. Bandwidth is measured once against a fixed-size probe endpoint
 * rather than guessed, and re-measured occasionally.
 */

const MAX_SAMPLES = 20;
const BANDWIDTH_REFRESH_MS = 5 * 60 * 1000;
const PROBE_BYTES = 512 * 1024;

export class NetworkAgent {
  private rtt: number[] = [];
  private bandwidthMbps = 0;
  private lastBandwidthAtMs = 0;
  private probing = false;

  constructor(private readonly httpBase: string) {}

  recordRoundTrip(ms: number): void {
    if (!Number.isFinite(ms) || ms < 0) return;
    this.rtt.push(ms);
    if (this.rtt.length > MAX_SAMPLES) this.rtt.shift();
  }

  get profile(): NetworkProfile {
    if (this.rtt.length === 0) {
      return { latency: 0, bandwidthMbps: this.bandwidthMbps, jitter: 0 };
    }
    const mean = this.rtt.reduce((a, b) => a + b, 0) / this.rtt.length;
    const variance = this.rtt.reduce((a, b) => a + (b - mean) ** 2, 0) / this.rtt.length;
    return {
      latency: round1(mean),
      jitter: round1(Math.sqrt(variance)),
      bandwidthMbps: round1(this.bandwidthMbps),
    };
  }

  /** Measure download throughput from the coordinator. Cheap and infrequent. */
  async maybeMeasureBandwidth(): Promise<void> {
    const now = Date.now();
    if (this.probing) return;
    if (this.lastBandwidthAtMs !== 0 && now - this.lastBandwidthAtMs < BANDWIDTH_REFRESH_MS) return;
    this.probing = true;
    try {
      const started = performance.now();
      const response = await fetch(
        `${this.httpBase}/api/probe?bytes=${PROBE_BYTES}&t=${Date.now()}`
      );
      if (!response.ok) return;
      const buffer = await response.arrayBuffer();
      const elapsedSec = (performance.now() - started) / 1000;
      if (elapsedSec > 0) {
        this.bandwidthMbps = (buffer.byteLength * 8) / elapsedSec / 1_000_000;
        this.lastBandwidthAtMs = Date.now();
      }
    } catch {
      // Probe endpoint unavailable: leave bandwidth at 0 rather than inventing one.
    } finally {
      this.probing = false;
    }
  }
}

function round1(value: number): number {
  return Math.round(value * 10) / 10;
}
