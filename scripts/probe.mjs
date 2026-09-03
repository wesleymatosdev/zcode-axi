#!/usr/bin/env node
// Probe for the `zcode app-server` stdio protocol ("ZCode Protocol").
// The server REJECTS the JSON-RPC "jsonrpc" envelope key; messages are
// bare {id, method, params} / {method, params} / {id, result} / {id, error}.
// Usage: node scripts/probe.mjs [method ...]
import { spawn } from "node:child_process";

const args = process.argv.slice(2);
const methods = args.length ? args : ["initialize"];

const child = spawn("zcode", ["app-server"], { stdio: ["pipe", "pipe", "pipe"] });
let nextId = 1;
const sendLog = [];

function request(method, params) {
  const id = nextId++;
  const msg = { id, method, params: params ?? {} };
  const line = JSON.stringify(msg) + "\n";
  sendLog.push("-> " + line.trim());
  child.stdin.write(line);
}

for (const m of methods) {
  const at = m.indexOf("@");
  const method = at >= 0 ? m.slice(0, at) : m;
  const params = at >= 0 ? JSON.parse(m.slice(at + 1)) : {};
  request(method, params);
}

let buf = "";
const timeout = setTimeout(() => {
  console.error(`PROBE: timeout after ${MS}ms, killing server`);
  child.kill("SIGKILL");
}, (globalThis.MS = Number(process.env.PROBE_MS || 4000)));

child.stdout.on("data", (d) => {
  buf += d.toString();
  let nl;
  while ((nl = buf.indexOf("\n")) >= 0) {
    const line = buf.slice(0, nl).trim();
    buf = buf.slice(nl + 1);
    if (line) console.log("<- " + line);
  }
});
child.stderr.on("data", (d) => process.stderr.write("ERR " + d));
child.on("exit", (code, sig) => {
  clearTimeout(timeout);
  console.error(`PROBE: server exited code=${code} sig=${sig}`);
  for (const l of sendLog) console.log(l);
  process.exit(0);
});
