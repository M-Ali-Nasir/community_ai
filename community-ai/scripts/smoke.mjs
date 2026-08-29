#!/usr/bin/env node
/**
 * End-to-end smoke test: connects as a client, submits a job, and prints what
 * the network actually did — which strategy the analyzer picked, which nodes
 * ran which task, and the measured timings.
 *
 * Usage:
 *   node scripts/smoke.mjs chat  "your prompt"
 *   node scripts/smoke.mjs batch "item one" "item two" "item three"
 */
import WebSocket from "ws";

const base = process.env.COORDINATOR_URL ?? "http://localhost:8787";
const token = process.env.JOIN_TOKEN ?? "";
const mode = process.argv[2] === "batch" ? "batch" : "chat";
const rest = process.argv.slice(3);
const policy = process.env.POLICY ?? "adaptive";
const modelId = process.env.MODEL_ID ?? "smollm2-360m";

const wsUrl = new URL(base);
wsUrl.protocol = wsUrl.protocol === "https:" ? "wss:" : "ws:";
wsUrl.pathname = "/ws/client";

const jobId = `job-smoke-${Date.now().toString(36)}`;
const socket = new WebSocket(wsUrl.toString());
const startedAt = Date.now();
let planPrinted = false;

const request =
  mode === "chat"
    ? {
        kind: "chat",
        modelId,
        policy,
        messages: [{ role: "user", content: rest.join(" ") || "In one sentence, what is a distributed system?" }],
        items: [],
        itemInstruction: "",
        reduceInstruction: "",
        maxTokens: Number(process.env.MAX_TOKENS ?? 120),
        temperature: 0.7,
      }
    : {
        kind: "batch",
        modelId,
        policy,
        messages: [],
        items: rest.length > 0 ? rest : [
          "Volunteer computing showed that idle consumer hardware adds up.",
          "Consumer GPUs now hold quantised models that once needed a datacentre.",
          "The hard part of distributed inference is link latency, not raw compute.",
          "Peer-to-peer systems grow capacity with demand instead of against it.",
        ],
        itemInstruction: "Summarise this in one short sentence.",
        reduceInstruction: process.env.NO_REDUCE ? "" : "Combine these into one sentence.",
        maxTokens: Number(process.env.MAX_TOKENS ?? 80),
        temperature: 0.7,
      };

socket.on("open", () => {
  socket.send(JSON.stringify({ type: "subscribe", protocolVersion: 1, token, label: "smoke test" }));
  socket.send(JSON.stringify({ type: "job:submit", jobId, request }));
  console.log(`submitted ${mode} job with policy=${policy} model=${modelId}\n`);
});

socket.on("message", (raw) => {
  const message = JSON.parse(raw.toString());

  if (message.type === "snapshot" && !planPrinted) {
    const ready = message.nodes.filter((n) => n.profile.runtime.ready);
    if (ready.length > 0) {
      console.log(`nodes ready: ${ready.map((n) => `${n.label} [${n.profile.gpu?.backend ?? "cpu"}]`).join(", ")}\n`);
    }
  }

  if (message.type === "job:planned" && message.jobId === jobId) {
    planPrinted = true;
    const { analysis } = message.plan;
    console.log(`ANALYZER  strategy=${analysis.strategy} coupling=${analysis.coupling} units=${analysis.unitCount}`);
    console.log(`          ${analysis.reason}`);
    for (const r of analysis.rejected) console.log(`          rejected ${r.strategy}: ${r.reason}`);
    console.log(`CLUSTER   ${message.plan.nodeIds.length} node(s): ${message.plan.reason}`);
    for (const [nodeId, share] of Object.entries(message.plan.shares)) {
      console.log(`          ${nodeId} -> ${(share * 100).toFixed(0)}%`);
    }
    console.log("");
  }

  if (message.type === "job:token" && message.jobId === jobId) {
    process.stdout.write(message.token);
  }

  if (message.type === "job:task" && message.jobId === jobId) {
    const t = message.task;
    if (t.status === "completed" && t.metrics) {
      console.log(
        `\n  [task ${t.index + 1} ${t.phase}] ${t.nodeLabel} — ${t.metrics.tokens} tokens, ` +
          `${t.metrics.tokensPerSecond.toFixed(1)} tok/s, first token ${Math.round(t.metrics.ttftMs)} ms`
      );
    }
    if (t.status === "failed") console.log(`\n  [task ${t.index + 1}] FAILED: ${t.error}`);
    if (t.status === "reassigned") console.log(`\n  [task ${t.index + 1}] reassigned: ${t.error}`);
  }

  if (message.type === "job:completed" && message.job.jobId === jobId) {
    console.log(`\n\nRESULT\n${message.job.output}\n`);
    console.log(
      `status=${message.job.status} wall=${((message.job.wallClockMs ?? Date.now() - startedAt) / 1000).toFixed(2)}s tokens=${message.job.totalTokens}`
    );
    socket.close();
    process.exit(message.job.status === "completed" ? 0 : 1);
  }

  if (message.type === "job:failed" && message.jobId === jobId) {
    console.error(`\nJOB FAILED: ${message.error}`);
    socket.close();
    process.exit(1);
  }
});

socket.on("error", (err) => {
  console.error(`socket error: ${err.message}`);
  process.exit(1);
});

setTimeout(() => {
  console.error("\ntimed out after 5 minutes");
  process.exit(1);
}, 300000).unref();
