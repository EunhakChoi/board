// End to end: builds the recommended template (with its workspace dependency @board/sdk) the way
// board-server sends it, then reads the result back.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, before, test } from "node:test";
import { fileURLToPath } from "node:url";
import { Builds } from "./builds.ts";
import { createWorkspaceServer } from "./server.ts";

const repo = fileURLToPath(new URL("../../..", import.meta.url));
const TOKEN = "test-token";
const COMMIT = "a".repeat(40);
const BROKEN = "b".repeat(40);
let home: string;
let base: string;
let close: () => void;
let builds: Builds;

before(async () => {
  home = mkdtempSync(join(tmpdir(), "board-workspace-"));
  builds = new Builds({ home, concurrency: 2 });
  const server = createWorkspaceServer({
    builds,
    token: TOKEN,
    maxUploadBytes: 64 * 1024 * 1024,
    version: "0",
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  close = () => server.close();
});

after(() => {
  close();
  rmSync(home, { recursive: true, force: true });
});

const auth = { authorization: `Bearer ${TOKEN}` };

function sourceTar(files: string[], boardJson: object | undefined): Buffer {
  const staging = mkdtempSync(join(tmpdir(), "board-src-"));
  try {
    execFileSync(
      "tar",
      ["-c", "--exclude=node_modules", "--exclude=dist", "-f", join(staging, "a.tar"), ...files],
      {
        cwd: repo,
      },
    );
    if (boardJson) {
      writeFileSync(join(staging, "board.json"), JSON.stringify(boardJson));
      execFileSync("tar", ["-r", "-f", join(staging, "a.tar"), "board.json"], { cwd: staging });
    }
    execFileSync("gzip", [join(staging, "a.tar")]);
    return readFileSync(join(staging, "a.tar.gz"));
  } finally {
    rmSync(staging, { recursive: true, force: true });
  }
}

async function put(commit: string, body: Buffer, query = "") {
  const response = await fetch(`${base}/v1/builds/demo/${commit}${query}`, {
    method: "PUT",
    headers: auth,
    body: new Uint8Array(body),
  });
  return { status: response.status, body: await response.json() };
}

async function waitDone(commit: string) {
  await builds.idle();
  const response = await fetch(`${base}/v1/builds/demo/${commit}`, { headers: auth });
  return response.json();
}

test("rejects calls without the token", async () => {
  assert.equal((await fetch(`${base}/health`)).status, 200);
  assert.equal((await fetch(`${base}/v1/builds/demo/${COMMIT}`)).status, 401);
  const wrong = await fetch(`${base}/v1/builds/demo/${COMMIT}`, {
    headers: { authorization: "Bearer nope" },
  });
  assert.equal(wrong.status, 401);
  assert.equal((await fetch(`${base}/v1/builds/Demo/${COMMIT}`, { headers: auth })).status, 400);
});

test("builds and verifies a commit, then serves it", { timeout: 300_000 }, async () => {
  const tar = sourceTar(
    [
      "package.json",
      "pnpm-workspace.yaml",
      "pnpm-lock.yaml",
      "tsconfig.base.json",
      "packages/sdk",
      "templates/recommended",
    ],
    { app: "templates/recommended" },
  );
  const started = await put(COMMIT, tar, "?verify=1");
  assert.equal(started.status, 202);
  assert.equal(started.body.base, `/demo/${COMMIT}/`);

  const build = await waitDone(COMMIT);
  assert.equal(build.state, "ready", `${build.error}\n${JSON.stringify(build.steps, null, 2)}`);
  assert.deepEqual(
    build.steps.map((s: { name: string; state: string }) => `${s.name}:${s.state}`),
    ["extract:ok", "install:ok", "typecheck:ok", "lint:skipped", "build:ok"],
  );

  const manifest = await fetch(`${base}/v1/files/demo/${COMMIT}/.board/manifest.json`, {
    headers: auth,
  });
  assert.equal(manifest.status, 200);
  assert.equal((await manifest.json()).title, "Tasks");
  const frame = await (
    await fetch(`${base}/v1/files/demo/${COMMIT}/.board/frame.html`, { headers: auth })
  ).text();
  assert.match(frame, new RegExp(`src="/demo/${COMMIT}/assets/frame-[\\w-]+\\.js"`));
  const traversal = await fetch(`${base}/v1/files/demo/${COMMIT}/..%2F..%2Fbuild.json`, {
    headers: auth,
  });
  assert.equal(traversal.status, 400);

  // Same commit again: nothing runs.
  const again = await put(COMMIT, Buffer.from("ignored"));
  assert.equal(again.status, 200);
  assert.equal(again.body.state, "ready");
  const log = await (await fetch(`${base}/v1/builds/demo/${COMMIT}/log`, { headers: auth })).text();
  assert.match(log, /vite build/);
});

test("a tree without a lockfile fails with the reason, and can be retried", async () => {
  const started = await put(BROKEN, sourceTar(["templates/recommended/package.json"], undefined));
  assert.equal(started.status, 202);
  const build = await waitDone(BROKEN);
  assert.equal(build.state, "failed");
  assert.match(build.error, /no package.json|no lockfile/);
  assert.equal(
    (await fetch(`${base}/v1/files/demo/${BROKEN}/index.html`, { headers: auth })).status,
    404,
  );

  assert.equal((await put(BROKEN, Buffer.from("x"))).status, 200);
  const retried = await put(BROKEN, Buffer.from("not a tar"), "?retry=1");
  assert.equal(retried.status, 202);
  assert.equal((await waitDone(BROKEN)).error, "could not extract the source");

  const removed = await fetch(`${base}/v1/builds/demo/${BROKEN}`, {
    method: "DELETE",
    headers: auth,
  });
  assert.equal(removed.status, 204);
  assert.equal((await fetch(`${base}/v1/builds/demo/${BROKEN}`, { headers: auth })).status, 404);
});
