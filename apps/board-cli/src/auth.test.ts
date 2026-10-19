import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { run } from "./main.ts";

const SERVER = "http://board.test";

/** A team server in miniature: device login approved on the second poll. */
function fakeServer() {
  const seen: { path: string; auth: string | null }[] = [];
  let polls = 0;
  let revoked = false;
  const fetch = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const request = input instanceof Request ? input : new Request(input, init);
    const path = new URL(request.url).pathname;
    const auth = request.headers.get("authorization");
    seen.push({ path, auth });
    const json = (body: unknown, status = 200) => Response.json(body, { status });
    switch (path) {
      case "/api/system":
        return json({ version: "0", localMode: false, services: {} });
      case "/api/auth/device":
        return json({
          deviceCode: "dev",
          userCode: "BCDF-GHJK",
          verificationUrl: `${SERVER}/device`,
          verificationUrlComplete: `${SERVER}/device?code=BCDF-GHJK`,
          interval: 3,
          expiresIn: 600,
        });
      case "/api/auth/device/token":
        polls += 1;
        return polls < 2
          ? json({ code: "authorization_pending", message: "wait" }, 400)
          : json({ token: "board_tok" });
      case "/api/auth/me":
        return auth === "Bearer board_tok" && !revoked
          ? json({
              kind: "user",
              name: "Ann",
              email: "ann@example.com",
              role: "editor",
              localMode: false,
            })
          : json({ code: "unauthorized", message: "Sign in first" }, 401);
      case "/api/auth/logout":
        revoked = true;
        return new Response(null, { status: 204 });
      default:
        return json({ code: "not_found", message: path }, 404);
    }
  };
  return { fetch, seen };
}

async function board(args: string[], env: Record<string, string>, fetch: typeof globalThis.fetch) {
  const out: string[] = [];
  const err: string[] = [];
  const code = await run(args, {
    env: { ...env, BOARD_URL: SERVER },
    locale: "en",
    fetch,
    out: (line) => out.push(line),
    err: (line) => err.push(line),
    pollMs: 1,
  });
  return { code, out: out.join("\n"), err: err.join("\n") };
}

test("login waits for browser approval, saves the token and uses it", async () => {
  const dir = mkdtempSync(join(tmpdir(), "board-auth-"));
  const env = { BOARD_CONFIG_DIR: dir };
  const server = fakeServer();
  try {
    const login = await board(["login", "--name", "laptop"], env, server.fetch);
    assert.equal(login.code, 0, login.err);
    assert.match(login.err, /open http:\/\/board\.test\/device and approve the code BCDF-GHJK/);
    assert.match(login.out, /Signed in to http:\/\/board\.test as Ann <ann@example\.com>/);
    const file = join(dir, "credentials.json");
    assert.equal(statSync(file).mode & 0o777, 0o600);
    assert.equal(JSON.parse(readFileSync(file, "utf8"))[SERVER].token, "board_tok");

    const who = await board(["whoami"], env, server.fetch);
    assert.equal(who.code, 0, who.err);
    assert.match(who.out, /Ann <ann@example\.com>, editor/);
    assert.equal(server.seen.at(-1)?.auth, "Bearer board_tok", "later commands send the token");

    const out = await board(["logout"], env, server.fetch);
    assert.match(out.out, /Signed out/);
    assert.deepEqual(JSON.parse(readFileSync(file, "utf8")), {});
    const after = await board(["whoami"], env, server.fetch);
    assert.equal(after.code, 1);
    assert.match(after.err, /Not signed in/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("BOARD_TOKEN wins and login --token checks the token first", async () => {
  const dir = mkdtempSync(join(tmpdir(), "board-auth-"));
  const server = fakeServer();
  try {
    const bad = await board(["login", "--token", "nope"], { BOARD_CONFIG_DIR: dir }, server.fetch);
    assert.equal(bad.code, 1);
    assert.match(bad.err, /did not accept/);
    const who = await board(
      ["whoami"],
      { BOARD_CONFIG_DIR: dir, BOARD_TOKEN: "board_tok" },
      server.fetch,
    );
    assert.equal(who.code, 0, who.err);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
