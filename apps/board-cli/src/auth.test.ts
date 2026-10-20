import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { run } from "./main.ts";

const SERVER = "http://board.test";

/** A shared board in miniature: `board_tok` is Ann's token; without one, a visitor. */
function fakeServer() {
  const seen: { path: string; auth: string | null }[] = [];
  const fetch = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const request = input instanceof Request ? input : new Request(input, init);
    const path = new URL(request.url).pathname;
    const auth = request.headers.get("authorization");
    seen.push({ path, auth });
    const json = (body: unknown, status = 200) => Response.json(body, { status });
    switch (path) {
      case "/api/system":
        return json({ version: "0", localMode: false, services: {} });
      case "/api/auth/me":
        if (auth === "Bearer board_tok")
          return json({ kind: "user", id: "u1", name: "Ann", role: "editor", localMode: false });
        if (auth) return json({ code: "invalid_token", message: "This token was reissued" }, 401);
        return json({
          kind: "visitor",
          id: "0000000000000000",
          name: "Curious Otter",
          role: "viewer",
          localMode: false,
        });
      default:
        return json({ code: "not_found", message: path }, 404);
    }
  };
  return { fetch, seen };
}

async function board(
  args: string[],
  env: Record<string, string>,
  fetch: typeof globalThis.fetch,
  ask?: (question: string) => Promise<string>,
) {
  const out: string[] = [];
  const err: string[] = [];
  const code = await run(args, {
    env: { ...env, BOARD_URL: SERVER },
    locale: "en",
    fetch,
    out: (line) => out.push(line),
    err: (line) => err.push(line),
    ...(ask ? { ask } : {}),
  });
  return { code, out: out.join("\n"), err: err.join("\n") };
}

test("login takes the token an admin handed over, saves it and uses it", async () => {
  const dir = mkdtempSync(join(tmpdir(), "board-auth-"));
  const env = { BOARD_CONFIG_DIR: dir };
  const server = fakeServer();
  try {
    const none = await board(["login"], env, server.fetch);
    assert.equal(none.code, 1);
    assert.match(none.err, /board login --token/);

    // At a terminal the CLI asks for it.
    const login = await board(["login"], env, server.fetch, async () => " board_tok ");
    assert.equal(login.code, 0, login.err);
    assert.match(login.out, /Signed in to http:\/\/board\.test as Ann \(editor\)/);
    const file = join(dir, "credentials.json");
    assert.equal(statSync(file).mode & 0o777, 0o600);
    assert.equal(JSON.parse(readFileSync(file, "utf8"))[SERVER].token, "board_tok");

    const who = await board(["whoami"], env, server.fetch);
    assert.equal(who.code, 0, who.err);
    assert.match(who.out, /^Ann, editor on/);
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
