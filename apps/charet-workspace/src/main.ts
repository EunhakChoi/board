#!/usr/bin/env node
// charet-workspace: installs, checks and builds project commits for charet-server.

import pkg from "../package.json" with { type: "json" };
import { Builds } from "./builds.ts";
import { readConfig } from "./config.ts";
import { createWorkspaceServer } from "./server.ts";

let config: ReturnType<typeof readConfig>;
try {
  config = readConfig(process.env);
} catch (error) {
  console.error(`charet-workspace: ${(error as Error).message}`);
  process.exit(2);
}

const builds = new Builds({ home: config.home, concurrency: config.jobs });
const server = createWorkspaceServer({
  builds,
  token: config.token,
  maxUploadBytes: config.maxUploadBytes,
  version: pkg.version,
});
server.listen(config.port, config.host, () => {
  console.log(`charet-workspace ${pkg.version} listening on ${config.host}:${config.port}`);
});

for (const signal of ["SIGINT", "SIGTERM"] as const) {
  process.once(signal, () => {
    server.close();
    process.exit(0);
  });
}
