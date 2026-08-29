import { spawn, type ChildProcess } from "node:child_process";
import { execFile } from "node:child_process";
import { networkInterfaces } from "node:os";
import { promisify } from "node:util";
import { binaryEnv, type BinaryPaths } from "../runtime/llamaBinaries.js";

const execFileAsync = promisify(execFile);

/**
 * RPC Server Agent.
 *
 * Publishes this machine's accelerators to the network so a pipeline head can
 * place model layers on them. This is the piece that lets the community hold a
 * model no single member could load: the node contributes *memory and compute*
 * without ever owning the whole weight file.
 *
 * The ggml RPC protocol has no authentication whatsoever and upstream is
 * explicit that it must never face an open network. The server therefore binds
 * to an explicit address that is expected to be an overlay-network interface
 * (Tailscale, ZeroTier) or a LAN, never 0.0.0.0 on a public host.
 */

export interface RpcDevice {
  name: string;
  description: string;
  totalMB: number;
  freeMB: number;
}

export interface RpcAgentOptions {
  binaries: BinaryPaths;
  port: number;
  /** Address to bind. Defaults to the best private/overlay address found. */
  bindHost?: string;
  /** Address peers should dial. Defaults to the bind address. */
  advertiseHost?: string;
  threads: number;
  /** Expose only these devices, e.g. "CPU" to keep the GPU for the owner. */
  device?: string;
}

export class RpcServerAgent {
  private child: ChildProcess | null = null;
  private stopping = false;
  private restarts = 0;
  private lastError: string | null = null;

  readonly host: string;
  readonly advertised: string;
  devices: RpcDevice[] = [];

  constructor(private readonly options: RpcAgentOptions) {
    this.host = options.bindHost ?? preferredHost();
    this.advertised = options.advertiseHost ?? this.host;
  }

  /** `host:port` other nodes dial, or null while the server is down. */
  get endpoint(): string | null {
    return this.child && !this.child.killed
      ? `${this.advertised}:${this.options.port}`
      : null;
  }

  get running(): boolean {
    return this.child !== null && !this.child.killed;
  }

  get error(): string | null {
    return this.lastError;
  }

  /** Total memory this node offers to a pipeline, across every exposed device. */
  get offeredMemoryMB(): number {
    return this.devices.reduce((sum, d) => sum + d.freeMB, 0);
  }

  async start(): Promise<void> {
    if (this.running) return;
    this.stopping = false;

    const args = [
      "-H", this.host,
      "-p", String(this.options.port),
      "-t", String(Math.max(1, this.options.threads)),
      // Cache tensors on disk so rejoining a pipeline skips the weight transfer.
      "-c",
    ];
    if (this.options.device) args.push("-d", this.options.device);

    const child = spawn(this.options.binaries.rpcServer, args, {
      env: binaryEnv(this.options.binaries.dir),
      stdio: ["ignore", "pipe", "pipe"],
    });
    this.child = child;

    child.stderr?.on("data", (buf: Buffer) => {
      const text = buf.toString();
      if (/error|failed|cannot/i.test(text)) this.lastError = text.trim().slice(0, 200);
    });

    child.on("exit", (code) => {
      this.child = null;
      this.devices = [];
      if (this.stopping) return;
      this.lastError = `ggml-rpc-server exited with code ${code}`;
      // The owner's machine may have suspended, or a peer may have wedged the
      // socket. Come back with backoff rather than silently dropping out.
      if (this.restarts < 10) {
        this.restarts += 1;
        setTimeout(() => void this.start(), Math.min(30_000, 1000 * 2 ** this.restarts));
      }
    });

    await waitForPort(this.host, this.options.port, 10_000);
    this.restarts = 0;
    this.lastError = null;
    await this.probeDevices();
  }

  /**
   * Ask llama.cpp what it sees on the other end of our own socket. Reporting
   * the number the pipeline head will actually observe avoids a whole class of
   * planning failure where the coordinator promises memory that isn't there.
   */
  async probeDevices(): Promise<void> {
    const endpoint = `${this.host}:${this.options.port}`;
    try {
      const { stdout } = await execFileAsync(
        this.options.binaries.llamaServer,
        ["--rpc", endpoint, "--list-devices"],
        { env: binaryEnv(this.options.binaries.dir), timeout: 15_000 }
      );
      this.devices = parseDevices(stdout).filter((d) => d.name.startsWith("RPC"));
    } catch (err) {
      this.lastError = err instanceof Error ? err.message.slice(0, 200) : String(err);
      this.devices = [];
    }
  }

  async stop(): Promise<void> {
    this.stopping = true;
    const child = this.child;
    if (!child) return;
    this.child = null;
    this.devices = [];
    child.kill("SIGTERM");
    await new Promise((r) => setTimeout(r, 300));
    if (!child.killed) child.kill("SIGKILL");
  }
}

/** `  RPC0: 127.0.0.1:50052 (7660 MiB, 7660 MiB free)` */
export function parseDevices(stdout: string): RpcDevice[] {
  const out: RpcDevice[] = [];
  for (const line of stdout.split("\n")) {
    const match = /^\s*(\w+):\s*(.+?)\s*\((\d+)\s*MiB,\s*(\d+)\s*MiB free\)/.exec(line);
    if (!match) continue;
    out.push({
      name: match[1] as string,
      description: match[2] as string,
      totalMB: Number(match[3]),
      freeMB: Number(match[4]),
    });
  }
  return out;
}

/**
 * Pick the address peers can actually dial. A Tailscale address wins because
 * it is the one interface reachable from outside the LAN, which is how friends
 * on different networks form a pipeline at all.
 */
export function preferredHost(): string {
  const candidates: { address: string; score: number }[] = [];
  for (const [name, addrs] of Object.entries(networkInterfaces())) {
    for (const addr of addrs ?? []) {
      if (addr.family !== "IPv4" || addr.internal) continue;
      let score = 1;
      if (/^100\.(6[4-9]|[7-9]\d|1[01]\d|12[0-7])\./.test(addr.address)) score = 100; // CGNAT: Tailscale
      else if (/^tailscale|^ts\d|^zt/i.test(name)) score = 90;
      else if (/^192\.168\./.test(addr.address)) score = 50;
      else if (/^10\./.test(addr.address)) score = 45;
      else if (/^172\.(1[6-9]|2\d|3[01])\./.test(addr.address)) score = 40;
      candidates.push({ address: addr.address, score });
    }
  }
  candidates.sort((a, b) => b.score - a.score);
  return candidates[0]?.address ?? "127.0.0.1";
}

async function waitForPort(host: string, port: number, timeoutMs: number): Promise<void> {
  const { connect } = await import("node:net");
  const deadline = Date.now() + timeoutMs;
  let lastError = "";
  while (Date.now() < deadline) {
    const ok = await new Promise<boolean>((resolveWait) => {
      const socket = connect({ host, port });
      socket.setTimeout(1000);
      socket.on("connect", () => {
        socket.destroy();
        resolveWait(true);
      });
      const fail = (err?: Error) => {
        lastError = err?.message ?? "timeout";
        socket.destroy();
        resolveWait(false);
      };
      socket.on("error", fail);
      socket.on("timeout", () => fail());
    });
    if (ok) return;
    await new Promise((r) => setTimeout(r, 250));
  }
  throw new Error(`ggml-rpc-server did not open ${host}:${port} (${lastError})`);
}
