import assert from "node:assert/strict";
import { type ChildProcess, spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { after, before, test } from "node:test";
import { defaultKey } from "./commands.ts";
import { parse, run } from "./main.ts";

const SERVER_BIN =
  process.env.BOARD_SERVER_BIN ??
  resolve(import.meta.dirname, "../../../target/debug/board-server");

async function freePort(): Promise<number> {
  return new Promise((done) => {
    const server = createServer().listen(0, "127.0.0.1", () => {
      const address = server.address();
      server.close(() => done(typeof address === "object" && address ? address.port : 0));
    });
  });
}

let server: ChildProcess | undefined;
let url = "";
let work = "";

before(async () => {
  assert.ok(existsSync(SERVER_BIN), `build board-server first (${SERVER_BIN})`);
  work = mkdtempSync(join(tmpdir(), "board-cli-"));
  const port = await freePort();
  const previewPort = await freePort();
  const remotePort = await freePort();
  url = `http://127.0.0.1:${port}`;
  server = spawn(SERVER_BIN, [], {
    env: {
      ...process.env,
      BOARD_HOME: join(work, "home"),
      BOARD_BIND: `127.0.0.1:${port}`,
      BOARD_PREVIEW_BIND: `127.0.0.1:${previewPort}`,
      BOARD_REMOTE_BIND: `127.0.0.1:${remotePort}`,
      BOARD_UPDATE_CHECK: "false",
    },
    stdio: "ignore",
  });
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(`${url}/api/health`)).ok) return;
    } catch {}
    await new Promise((r) => setTimeout(r, 50));
  }
  throw new Error("board-server did not start");
});

after(() => {
  server?.kill();
  rmSync(work, { recursive: true, force: true });
});

async function board(args: string[], cwd = work, env: Record<string, string> = { BOARD_URL: url }) {
  const out: string[] = [];
  const err: string[] = [];
  const code = await run(args, {
    cwd,
    locale: "en",
    env,
    out: (line) => out.push(line),
    err: (line) => err.push(line),
  });
  return { code, out: out.join("\n"), err: err.join("\n") };
}

function write(root: string, files: Record<string, string>) {
  for (const [path, content] of Object.entries(files)) {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), content);
  }
}

test("parses flags, values and repeated lists", () => {
  const args = parse(["import", "app", "--key=web", "--include", "a", "--include", "b", "--json"]);
  assert.deepEqual(args.positional, ["import", "app"]);
  assert.equal(args.flags.get("--key"), "web");
  assert.equal(args.flags.get("--json"), true);
  assert.deepEqual(args.lists.get("--include"), ["a", "b"]);
});

test("derives a project key from a folder", () => {
  assert.equal(defaultKey("/x/Admin Web_2"), "admin-web-2");
  assert.equal(defaultKey("/x/___"), "project");
});

test("prints the version and usage", async () => {
  assert.deepEqual(await board(["--version"]), { code: 0, out: "board 0.0.0", err: "" });
  assert.match((await board([])).out, /^Usage: board/);
  assert.equal((await board(["nope"])).code, 1);
  // --help on a command prints usage instead of running it.
  const help = await board(["import", "--help"]);
  assert.equal(help.code, 0);
  assert.match(help.out, /^Usage: board/);
  assert.doesNotMatch((await board(["projects"])).out, /^board-cli/m);
});

test("deletes a project only when confirmed", async () => {
  const app = join(work, "gone-app");
  write(app, { "package.json": "{}", "src/a.ts": "export {};\n" });
  assert.equal((await board(["import", app, "--key", "gone"])).code, 0);
  const refused = await board(["delete", "gone"]);
  assert.equal(refused.code, 1);
  assert.match(refused.err, /add --yes/);
  const deleted = await board(["delete", "gone", "--yes"]);
  assert.equal(deleted.code, 0, deleted.err);
  assert.match(deleted.out, /Deleted project gone/);
  assert.doesNotMatch((await board(["projects"])).out, /^gone\t/m);
  assert.equal((await board(["delete", "gone", "--yes"])).code, 1);
});

test("import, checkout, push, pull, versions and diff", async () => {
  const repo = join(work, "repo");
  write(repo, {
    "pnpm-workspace.yaml": "packages:\n  - apps/*\n  - packages/*\n",
    "package.json": "{}",
    "apps/web/package.json": JSON.stringify({
      name: "web",
      dependencies: { "@x/api": "workspace:*" },
    }),
    "apps/web/src/page.tsx": "export const title = 'one';\n",
    "apps/server/package.json": JSON.stringify({ name: "server" }),
    "packages/api/package.json": JSON.stringify({ name: "@x/api" }),
    "packages/api/index.ts": "export {};\n",
  });

  const plan = await board(["import", join(repo, "apps/web"), "--dry-run", "--json"]);
  const planned = JSON.parse(plan.out);
  assert.equal(planned.key, "web");
  assert.deepEqual(planned.packages, ["apps/web", "packages/api"]);

  const imported = await board(["import", join(repo, "apps/web"), "--key", "demo"]);
  assert.equal(imported.code, 0, imported.err);
  assert.match(imported.out, /Imported 6 files as project demo/);
  assert.match((await board(["projects"])).out, /^demo\t/);

  const a = join(work, "a");
  assert.equal((await board(["checkout", "demo", a])).code, 0);
  assert.equal(
    readFileSync(join(a, "apps/web/src/page.tsx"), "utf8"),
    "export const title = 'one';\n",
  );
  assert.ok(existsSync(join(a, "board.json")));
  assert.match((await board(["push"], a)).out, /Nothing to push/);
  // Inside a checkout, commands talk to its server, whatever BOARD_URL says.
  const listed = await board(["versions"], a, { BOARD_URL: "http://127.0.0.1:9" });
  assert.equal(listed.code, 0, listed.err);
  assert.match(listed.out, /v1/);

  // Ignored output never travels.
  write(a, {
    ".gitignore": "node_modules/\n",
    "node_modules/x/index.js": "",
    "apps/web/src/page.tsx": "export const title = 'two';\n",
  });
  const pushed = await board(["push", "-m", "retitle"], a);
  assert.equal(pushed.code, 0, pushed.err);
  assert.match(pushed.out, /Pushed 2 changed files as version 2/);

  const b = join(work, "b");
  await board(["checkout", "demo", b, "--rev", "v1"]);
  write(b, { "apps/web/src/page.tsx": "stale\n" });
  const conflict = await board(["push"], b);
  assert.equal(conflict.code, 1);
  assert.match(conflict.err, /Someone pushed first/);
  assert.match((await board(["pull"], b)).err, /unpushed changes/);

  const c = join(work, "c");
  await board(["checkout", "demo", c, "--rev", "v1"]);
  assert.match((await board(["pull"], c)).out, /Updated to/);
  assert.equal(
    readFileSync(join(c, "apps/web/src/page.tsx"), "utf8"),
    "export const title = 'two';\n",
  );

  assert.match((await board(["versions"], a)).out, /^v2\tpush\t\w+\tAdmin\tretitle/);
  assert.equal(
    (await board(["diff", "v1", "-p", "demo"])).out,
    "A\t.gitignore\nM\tapps/web/src/page.tsx",
  );
  assert.match((await board(["versions"])).err, /Which project/);

  const behind = (await board(["status"], b)).out;
  assert.match(behind, /^project {2}demo /);
  assert.match(behind, /base {5}v1 /);
  assert.match(behind, /main {5}v2 \w+ {2}newer than this checkout/);
  assert.match(behind, /changes {2}1\n {2}M apps\/web\/src\/page\.tsx$/);
  assert.match((await board(["status"], a)).out, /main {5}v2 \w+\nchanges {2}0$/);

  // A pull replaces the files of the base and leaves ignored ones (node_modules) alone.
  write(c, { "node_modules/kept.js": "" });
  rmSync(join(a, "apps/web/src/page.tsx"));
  assert.equal((await board(["push", "-m", "drop page"], a)).code, 0);
  assert.match((await board(["pull"], c)).out, /Updated to/);
  assert.ok(existsSync(join(c, "node_modules/kept.js")));
  assert.ok(!existsSync(join(c, "apps/web/src/page.tsx")));
});

test("init creates a project from a template and checks it out", async () => {
  const dir = join(work, "tasks");
  const result = await board(["init", "tasks", dir]);
  assert.equal(result.code, 0, result.err);
  assert.match(result.out, /Checked out tasks/);
  assert.ok(existsSync(join(dir, "templates/recommended/.board/project.tsx")));
  assert.match((await board(["status"], dir)).out, /changes {2}0$/);
  assert.match((await board(["init", "x", "--template", "nope"])).err, /No template called nope/);
});
