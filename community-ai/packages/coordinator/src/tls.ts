import { execFile } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { homedir, networkInterfaces } from "node:os";
import { join } from "node:path";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);

/**
 * Self-signed TLS for the LAN testnet.
 *
 * Browsers only expose WebGPU in a secure context (HTTPS or localhost). Friends
 * opening http://192.168.x.x:8787 therefore see "No WebGPU" even in a current
 * Chrome. A local certificate fixes that after they click through the one-time
 * browser warning.
 */

export interface TlsMaterial {
  key: Buffer;
  cert: Buffer;
  dir: string;
}

export function tlsDir(): string {
  return process.env.TLS_DIR ?? join(homedir(), ".community-ai", "certs");
}

function collectSans(): string[] {
  const sans = new Set<string>(["DNS:localhost", "DNS:community-ai.local", "IP:127.0.0.1"]);
  for (const addrs of Object.values(networkInterfaces())) {
    for (const addr of addrs ?? []) {
      if (addr.family === "IPv4" && !addr.internal) sans.add(`IP:${addr.address}`);
    }
  }
  // Keep the OpenSSL command short; a dozen interfaces is plenty for a home network.
  return [...sans].slice(0, 16);
}

async function opensslAvailable(): Promise<boolean> {
  try {
    await execFileAsync("openssl", ["version"]);
    return true;
  } catch {
    return false;
  }
}

export async function loadOrCreateTls(): Promise<TlsMaterial> {
  const dir = tlsDir();
  await mkdir(dir, { recursive: true });
  const keyPath = join(dir, "key.pem");
  const certPath = join(dir, "cert.pem");
  const metaPath = join(dir, "sans.txt");
  const sans = collectSans();
  const sansLine = sans.join(",");

  const reusable =
    existsSync(keyPath) &&
    existsSync(certPath) &&
    existsSync(metaPath) &&
    (await readFile(metaPath, "utf8")).trim() === sansLine;

  if (!reusable) {
    if (!(await opensslAvailable())) {
      throw new Error(
        "HTTPS needs openssl to mint a local certificate. Install openssl, or set HTTPS=false " +
          "and have friends contribute with the native worker instead of the browser tab."
      );
    }
    // OpenSSL 3 accepts -addext; older builds need a config file. Prefer -addext
    // and fall back to a temporary config if that fails.
    try {
      await execFileAsync("openssl", [
        "req",
        "-x509",
        "-newkey",
        "rsa:2048",
        "-sha256",
        "-days",
        "825",
        "-nodes",
        "-keyout",
        keyPath,
        "-out",
        certPath,
        "-subj",
        "/CN=community-ai.local",
        "-addext",
        `subjectAltName=${sansLine}`,
      ]);
    } catch {
      const conf = join(dir, "openssl.cnf");
      await writeFile(
        conf,
        `[req]
distinguished_name=req
x509_extensions=v3
prompt=no
[req]
CN=community-ai.local
[v3]
subjectAltName=${sansLine}
`
      );
      await execFileAsync("openssl", [
        "req",
        "-x509",
        "-newkey",
        "rsa:2048",
        "-sha256",
        "-days",
        "825",
        "-nodes",
        "-keyout",
        keyPath,
        "-out",
        certPath,
        "-subj",
        "/CN=community-ai.local",
        "-config",
        conf,
        "-extensions",
        "v3",
      ]);
    }
    await writeFile(metaPath, `${sansLine}\n`);
  }

  return {
    key: await readFile(keyPath),
    cert: await readFile(certPath),
    dir,
  };
}
