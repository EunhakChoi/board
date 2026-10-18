// Settings from environment variables.

import { readFileSync } from "node:fs";
import { isIP } from "node:net";
import { homedir } from "node:os";
import { join } from "node:path";

export interface Config {
  /** Sources, builds and dependency caches (`BOARD_WORKSPACE_HOME`, default `$BOARD_HOME/workspace`). */
  home: string;
  host: string;
  port: number;
  /** Bearer token board-server sends (`BOARD_WORKSPACE_TOKEN`, or the file `BOARD_WORKSPACE_TOKEN_FILE`). Required off loopback. */
  token: string | undefined;
  /** Builds running at once (`BOARD_WORKSPACE_JOBS`, default 2). */
  jobs: number;
  maxUploadBytes: number;
}

const DEFAULT_BIND = "127.0.0.1:5282";

export function readConfig(env: NodeJS.ProcessEnv): Config {
  const boardHome = env.BOARD_HOME ?? join(env.HOME ?? homedir(), ".board");
  const home = env.BOARD_WORKSPACE_HOME ?? join(boardHome, "workspace");
  const bind = env.BOARD_WORKSPACE_BIND ?? DEFAULT_BIND;
  const match = /^(.+):(\d+)$/.exec(bind);
  const host = match?.[1]?.replace(/^\[(.*)\]$/, "$1");
  const port = Number(match?.[2]);
  if (!host || !isIP(host) || !Number.isInteger(port) || port > 65535) {
    throw new Error(`BOARD_WORKSPACE_BIND "${bind}" is not an address like ${DEFAULT_BIND}`);
  }
  const token = secret(env, "BOARD_WORKSPACE_TOKEN");
  if (!token && !loopback(host)) {
    throw new Error(`${bind} is reachable from other machines; set BOARD_WORKSPACE_TOKEN`);
  }
  const jobs = Number(env.BOARD_WORKSPACE_JOBS ?? "2");
  if (!Number.isInteger(jobs) || jobs < 1)
    throw new Error("BOARD_WORKSPACE_JOBS must be a positive integer");
  return { home, host, port, token, jobs, maxUploadBytes: 512 * 1024 * 1024 };
}

/** A secret from `NAME`, or from the file `NAME_FILE` names (docker secrets, shared volumes). */
function secret(env: NodeJS.ProcessEnv, name: string): string | undefined {
  const value = env[name];
  if (value) return value;
  const file = env[`${name}_FILE`];
  if (!file) return undefined;
  return readFileSync(file, "utf8").trim() || undefined;
}

function loopback(host: string): boolean {
  return host === "::1" || host.startsWith("127.");
}
