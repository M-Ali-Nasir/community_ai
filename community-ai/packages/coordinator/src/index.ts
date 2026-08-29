import { networkInterfaces } from "node:os";
import { config } from "./config.js";
import { createCoordinator } from "./server.js";
import { loadOrCreateTls } from "./tls.js";

const tls = config.https ? await loadOrCreateTls() : null;
const scheme = tls ? "https" : "http";
const { server, close } = createCoordinator(tls);

function reachableAddresses(port: number): string[] {
  const out: string[] = [`${scheme}://localhost:${port}`];
  for (const [name, addrs] of Object.entries(networkInterfaces())) {
    for (const addr of addrs ?? []) {
      if (addr.family !== "IPv4" || addr.internal) continue;
      const isTailscale = name.startsWith("tailscale") || addr.address.startsWith("100.");
      out.push(
        `${scheme}://${addr.address}:${port}${isTailscale ? "   <- overlay network" : ""}`
      );
    }
  }
  return out;
}

server.listen(config.port, config.host, () => {
  console.log("");
  console.log("  Community AI coordinator");
  console.log("  ------------------------");
  for (const address of reachableAddresses(config.port)) {
    console.log(`  ${address}`);
  }
  console.log("");
  if (tls) {
    console.log(`  TLS          : self-signed cert in ${tls.dir}`);
    console.log("  Browser tip  : open the https:// URL, click Advanced → Proceed (once).");
    console.log("                 WebGPU Contribute only works over HTTPS from other PCs.");
  } else {
    console.log("  TLS          : off (HTTPS=false). Browser Contribute on other PCs will fail.");
  }
  console.log(`  join token   : ${config.joinToken ? "required" : "not set (open on this network)"}`);
  console.log(`  default model: ${config.defaultModelId}`);
  console.log(`  browser tabs : ${config.allowBrowserContributors ? "may contribute" : "client only"}`);
  console.log("");
  console.log(`  Friends (browser): open the https URL above in Chrome/Edge`);
  console.log(`  Friends (worker) : node packages/worker-node/dist/index.js --coordinator ${scheme}://<ip>:${config.port}`);
  console.log("");
});

let shuttingDown = false;
for (const signal of ["SIGINT", "SIGTERM"] as const) {
  process.on(signal, () => {
    if (shuttingDown) return;
    shuttingDown = true;
    console.log("\nshutting down...");
    void close().then(() => process.exit(0));
    setTimeout(() => process.exit(0), 3000).unref();
  });
}
