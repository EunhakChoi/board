#!/usr/bin/env node
// charet-agent: runs the web agent for charet-server, one container per run.

import pkg from "../package.json" with { type: "json" };
import { readConfig } from "./config.ts";
import { DockerLauncher, type Launcher, ProcessLauncher } from "./launch.ts";
import { createDoorProxy, createModelProxy } from "./proxy.ts";
import { Runs } from "./runs.ts";
import { createAgentServer } from "./server.ts";

let config: ReturnType<typeof readConfig>;
try {
  config = readConfig(process.env);
} catch (error) {
  console.error(`charet-agent: ${(error as Error).message}`);
  process.exit(2);
}

const docker = config.sandbox === "docker" ? new DockerLauncher(config) : undefined;
const launcher: Launcher = docker ?? new ProcessLauncher(config);
await launcher.cleanup().catch((error: unknown) => {
  console.error(`charet-agent: cannot remove old runs: ${(error as Error).message}`);
});
const runs = new Runs(launcher, { prices: config.prices, timeoutMs: config.timeoutMs });
const server = createAgentServer({
  runs,
  token: config.token,
  version: pkg.version,
  ready: async () => docker?.ready(),
});
server.listen(config.bind.port, config.bind.host, () => {
  console.log(
    `charet-agent ${pkg.version} listening on ${config.bind.host}:${config.bind.port} (${
      config.sandbox === "docker"
        ? `runs in ${config.docker.image}`
        : "runs as plain processes: tests only"
    })`,
  );
});
const servers = [server];
if (docker) {
  // Runs reach the server and the model APIs only through these.
  const door = createDoorProxy(config.doorUrl);
  door.listen(config.doorBind.port, config.doorBind.host);
  const proxy = createModelProxy(config.allowHosts);
  proxy.listen(config.proxyBind.port, config.proxyBind.host);
  servers.push(door, proxy);
}

for (const signal of ["SIGINT", "SIGTERM"] as const) {
  process.once(signal, () => {
    for (const s of servers) s.close();
    process.exit(0);
  });
}
