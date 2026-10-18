// Starts board-render, board-workspace and board-server on free ports with a scratch home,
// imports templates/recommended as project `demo` and waits for its preview.

import { type ChildProcess, execFileSync, spawn } from "node:child_process";
import { mkdtempSync, openSync, rmSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const root = resolve(import.meta.dirname, "../../..");

function freePort(): Promise<number> {
  return new Promise((done, fail) => {
    const server = createServer();
    server.once("error", fail);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      server.close(() => done(typeof address === "object" && address ? address.port : 0));
    });
  });
}

async function waitFor(
  url: string,
  ok: (body: unknown) => boolean,
  seconds: number,
): Promise<unknown> {
  const until = Date.now() + seconds * 1000;
  let last: unknown;
  while (Date.now() < until) {
    const response = await fetch(url).catch(() => undefined);
    if (response) {
      last = response.headers.get("content-type")?.includes("json")
        ? await response.json()
        : response.status;
      if (response.ok && ok(last)) return last;
    }
    await new Promise((r) => setTimeout(r, 300));
  }
  throw new Error(`timed out waiting for ${url}: ${JSON.stringify(last)}`);
}

export default async function setup() {
  const home = mkdtempSync(join(tmpdir(), "board-e2e-"));
  const [api, preview, workspace, render] = [
    await freePort(),
    await freePort(),
    await freePort(),
    await freePort(),
  ];
  const log = openSync(join(home, "stack.log"), "a");
  const start = (command: string, args: string[], env: Record<string, string>): ChildProcess =>
    spawn(command, args, { env: { ...process.env, ...env }, stdio: ["ignore", log, log] });
  const token = "e2e";
  const children = [
    start(join(root, "target/debug/board-render"), [], {
      BOARD_RENDER_BIND: `127.0.0.1:${render}`,
    }),
    start(process.execPath, [join(root, "apps/board-workspace/src/main.ts")], {
      BOARD_WORKSPACE_HOME: join(home, "workspace"),
      BOARD_WORKSPACE_BIND: `127.0.0.1:${workspace}`,
      BOARD_WORKSPACE_TOKEN: token,
    }),
    start(join(root, "target/debug/board-server"), [], {
      BOARD_HOME: join(home, "server"),
      BOARD_BIND: `127.0.0.1:${api}`,
      BOARD_PREVIEW_BIND: `127.0.0.1:${preview}`,
      BOARD_PUBLIC_URL: `http://localhost:${api}`,
      BOARD_WEB_DIR: join(root, "apps/board-web/dist"),
      BOARD_WORKSPACE_URL: `http://127.0.0.1:${workspace}`,
      BOARD_WORKSPACE_TOKEN: token,
      BOARD_RENDER_URL: `http://127.0.0.1:${render}`,
      BOARD_UPDATE_CHECK: "false",
    }),
  ];
  const teardown = async () => {
    for (const child of children) child.kill();
    if (!process.env.BOARD_E2E_KEEP) rmSync(home, { recursive: true, force: true });
  };
  try {
    const base = `http://127.0.0.1:${api}`;
    await waitFor(`${base}/api/health`, () => true, 20);
    await waitFor(`http://127.0.0.1:${workspace}/health`, () => true, 20);
    await waitFor(`http://127.0.0.1:${render}/health`, () => true, 20);
    execFileSync(
      process.execPath,
      [join(root, "apps/board-cli/src/main.ts"), "import", "--key", "demo"],
      {
        cwd: join(root, "templates/recommended"),
        env: { ...process.env, BOARD_URL: base },
        stdio: ["ignore", log, log],
      },
    );
    await waitFor(
      `${base}/api/projects/demo/preview`,
      (body) => {
        const state = (body as { state?: string }).state;
        if (state === "failed") throw new Error(`preview failed: ${JSON.stringify(body)}`);
        return state === "ready";
      },
      180,
    );
  } catch (error) {
    for (const child of children) child.kill();
    throw new Error(`stack did not start (logs: ${join(home, "stack.log")}): ${error}`);
  }
  process.env.BOARD_E2E_URL = `http://localhost:${api}`;
  return teardown;
}
