// Runs one tool (pnpm, vite, tsc …) inside a project tree. Project code is untrusted: the child gets a
// minimal environment (never the service token), a time limit, and its output only goes to the log.

import { spawn } from "node:child_process";
import { appendFileSync } from "node:fs";

export interface RunResult {
  code: number | null;
  /** Last part of the combined output, for the job summary. */
  tail: string;
  timedOut: boolean;
}

const TAIL_BYTES = 4096;

export function childEnv(home: string, extra: Record<string, string> = {}): NodeJS.ProcessEnv {
  const env: NodeJS.ProcessEnv = { PATH: process.env.PATH ?? "/usr/bin:/bin", HOME: home, CI: "1" };
  for (const key of ["LANG", "TMPDIR", "COREPACK_HOME"]) {
    if (process.env[key]) env[key] = process.env[key];
  }
  return { ...env, ...extra };
}

export function run(
  command: string,
  args: string[],
  options: { cwd: string; env: NodeJS.ProcessEnv; log: string; timeoutMs: number },
): Promise<RunResult> {
  appendFileSync(options.log, `$ ${[command, ...args].join(" ")}\n`);
  return new Promise((resolve) => {
    const child = spawn(command, args, {
      cwd: options.cwd,
      env: options.env,
      stdio: ["ignore", "pipe", "pipe"],
      detached: true, // own process group, so a timeout kills the whole tree
    });
    let tail = "";
    let timedOut = false;
    const onData = (chunk: Buffer) => {
      appendFileSync(options.log, chunk);
      tail = (tail + chunk.toString("utf8")).slice(-TAIL_BYTES);
    };
    child.stdout.on("data", onData);
    child.stderr.on("data", onData);
    const timer = setTimeout(() => {
      timedOut = true;
      kill(child.pid);
    }, options.timeoutMs);
    child.on("error", (error) => {
      clearTimeout(timer);
      appendFileSync(options.log, `${error.message}\n`);
      resolve({ code: null, tail: error.message, timedOut });
    });
    child.on("close", (code) => {
      clearTimeout(timer);
      if (timedOut) appendFileSync(options.log, `timed out after ${options.timeoutMs} ms\n`);
      resolve({ code, tail, timedOut });
    });
  });
}

function kill(pid: number | undefined): void {
  if (pid === undefined) return;
  try {
    process.kill(-pid, "SIGKILL");
  } catch {
    // Already gone.
  }
}
