// End to end without Docker: charet-server asks charet-agent (plain-process runs) and the script
// agent works on a branch through the agent door with the charet CLI.

import assert from "node:assert/strict";
import { type ChildProcess, spawn } from "node:child_process";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { after, before, test } from "node:test";

const SERVER_BIN =
  process.env.CHARET_SERVER_BIN ??
  resolve(import.meta.dirname, "../../../target/debug/charet-server");
const CLI = resolve(import.meta.dirname, "../../charet-cli/src/main.ts");
const TOKEN = "internal-test-token";

async function freePort(): Promise<number> {
  return new Promise((done) => {
    const server = createServer().listen(0, "127.0.0.1", () => {
      const address = server.address();
      server.close(() => done(typeof address === "object" && address ? address.port : 0));
    });
  });
}

const children: ChildProcess[] = [];
let url = "";
let work = "";

async function waitFor(check: () => Promise<boolean>, what: string, ms = 20_000): Promise<void> {
  const until = Date.now() + ms;
  while (Date.now() < until) {
    try {
      if (await check()) return;
    } catch {}
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error(`timed out waiting for ${what}`);
}

before(async () => {
  assert.ok(existsSync(SERVER_BIN), `build charet-server first (${SERVER_BIN})`);
  work = mkdtempSync(join(tmpdir(), "charet-agent-"));
  const [port, preview, remote, door, agent] = await Promise.all(
    [0, 1, 2, 3, 4].map(() => freePort()),
  );
  url = `http://127.0.0.1:${port}`;
  children.push(
    spawn(process.execPath, [resolve(import.meta.dirname, "main.ts")], {
      env: {
        PATH: process.env.PATH,
        HOME: work,
        CHARET_AGENT_BIND: `127.0.0.1:${agent}`,
        CHARET_AGENT_TOKEN: TOKEN,
        CHARET_AGENT_SANDBOX: "none",
        CHARET_AGENT_DOOR_URL: `http://127.0.0.1:${door}`,
        CHARET_CLI: CLI,
      },
      stdio: "ignore",
    }),
    spawn(SERVER_BIN, [], {
      env: {
        ...process.env,
        CHARET_HOME: join(work, "home"),
        CHARET_BIND: `127.0.0.1:${port}`,
        CHARET_PREVIEW_BIND: `127.0.0.1:${preview}`,
        CHARET_REMOTE_BIND: `127.0.0.1:${remote}`,
        CHARET_UPDATE_CHECK: "false",
        CHARET_MERGE_CHECK: "false",
        CHARET_AGENT_URL: `http://127.0.0.1:${agent}`,
        CHARET_AGENT_TOKEN: TOKEN,
        CHARET_AGENT_BIND: `127.0.0.1:${door}`,
        CHARET_AGENT_SCRIPT: "true",
      },
      stdio: "ignore",
    }),
  );
  await waitFor(
    async () => (await api("GET", "/api/agent")).enabled === true,
    "the agent to be on",
  );
});

after(() => {
  for (const child of children) child.kill();
  rmSync(work, { recursive: true, force: true });
});

interface Run {
  id: number;
  branch: number;
  state: string;
  error: string | null;
}

interface Branch {
  status: string;
  author: { name: string };
  changes: { path: string }[];
}

// biome-ignore lint/suspicious/noExplicitAny: JSON from the API, typed by each caller.
type Json = any;

async function api<T = Json>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`${url}${path}`, {
    method,
    headers: { "content-type": "application/json" },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  const text = await res.text();
  if (!res.ok) throw new Error(`${method} ${path}: ${res.status} ${text}`);
  return (text ? JSON.parse(text) : null) as T;
}

async function finished(id: number, branch: number): Promise<Run> {
  let run: Run | undefined;
  await waitFor(async () => {
    const runs = await api<Run[]>("GET", `/api/projects/web/agent-runs?branch=${branch}`);
    run = runs.find((r) => r.id === id);
    return ["done", "failed", "canceled"].includes(run?.state ?? "");
  }, `run ${id} to end`);
  return run as Run;
}

test("the script agent changes a new draft through the agent door", async () => {
  await api("POST", "/api/projects", {
    key: "web",
    import: {
      source: "test",
      files: [{ path: "a.txt", content: Buffer.from("a\n").toString("base64") }],
    },
  });
  const started = await api<Run>("POST", "/api/projects/web/agent-runs", {
    prompt: "write src/hello.txt: hi\nsay All done.",
    agent: "script",
    context: { screen: "home" },
  });
  const run = await finished(started.id, started.branch);
  assert.equal(run.state, "done", run.error ?? undefined);
  const branch = await api<Branch>("GET", `/api/projects/web/branches/${started.branch}`);
  assert.equal(branch.status, "open");
  assert.deepEqual(
    branch.changes.map((c) => c.path),
    ["src/hello.txt"],
  );
  const messages = await api<{ kind: string; text: string }[]>(
    "GET",
    `/api/projects/web/agent-runs/${started.id}/messages`,
  );
  const kinds = messages.map((m) => `${m.kind}:${m.text}`);
  assert.equal(kinds[0], "user:write src/hello.txt: hi\nsay All done.");
  assert.ok(kinds.includes("assistant:All done."), kinds.join("\n"));
  assert.match(kinds.at(-1) ?? "", /^status:Done/);
  const versions = await api<Branch>("GET", `/api/projects/web/branches/${started.branch}`);
  assert.equal(versions.author.name, "Admin");
});

test("a canceled run stops and keeps what it saved", async () => {
  const started = await api<Run>("POST", "/api/projects/web/agent-runs", {
    prompt: "write b.txt: kept\nwait 60000",
    agent: "script",
  });
  await waitFor(async () => {
    const branch = await api<Branch>("GET", `/api/projects/web/branches/${started.branch}`);
    return branch.changes.length === 1;
  }, "the first save");
  await api("POST", `/api/projects/web/agent-runs/${started.id}/cancel`, {});
  const run = await finished(started.id, started.branch);
  assert.equal(run.state, "canceled");
  const branch = await api<Branch>("GET", `/api/projects/web/branches/${started.branch}`);
  assert.equal(branch.status, "open", "a draft with a save stays");
});

test("a failing run reports why", async () => {
  const started = await api<Run>("POST", "/api/projects/web/agent-runs", {
    prompt: "fail on purpose",
    agent: "script",
  });
  const run = await finished(started.id, started.branch);
  assert.equal(run.state, "failed");
  assert.match(run.error ?? "", /on purpose/);
  const branch = await api<Branch>("GET", `/api/projects/web/branches/${started.branch}`);
  assert.equal(branch.status, "archived", "an empty new draft is not left behind");
});
