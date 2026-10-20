import assert from "node:assert/strict";
import { test } from "node:test";
import { run } from "./main.ts";

const SERVER = "http://charet.test";

function fakeServer() {
  const notes = [
    {
      id: 2,
      kind: "comment.replied",
      project: "web",
      actor: "Boss",
      text: "Boss replied to c1 on home: On it",
      link: "/p/web/t/screen/home?comment=1",
      data: {},
      createdAt: "2026-10-19T09:30:00.000Z",
      readAt: null as string | null,
    },
  ];
  const reads: unknown[] = [];
  const fetch = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const request = input instanceof Request ? input : new Request(input, init);
    const url = new URL(request.url);
    if (url.pathname === "/api/me/notifications") {
      const unreadOnly = url.searchParams.get("unread") === "true";
      const list = notes.filter((n) => !unreadOnly || n.readAt === null);
      return Response.json({
        notifications: list,
        unread: notes.filter((n) => n.readAt === null).length,
      });
    }
    if (url.pathname === "/api/me/notifications/read") {
      reads.push(await request.json());
      for (const n of notes) n.readAt = "now";
      return Response.json({ unread: 0 });
    }
    return Response.json({ code: "not_found", message: url.pathname }, { status: 404 });
  };
  return { fetch, reads };
}

async function charet(args: string[], fetch: typeof globalThis.fetch) {
  const out: string[] = [];
  const err: string[] = [];
  const code = await run(args, {
    env: { CHARET_URL: SERVER, CHARET_TOKEN: "charet_tok" },
    locale: "en",
    fetch,
    out: (line) => out.push(line),
    err: (line) => err.push(line),
  });
  return { code, out: out.join("\n"), err: err.join("\n") };
}

test("inbox lists unread notifications with links and marks them read", async () => {
  const server = fakeServer();
  const listed = await charet(["inbox"], server.fetch);
  assert.equal(listed.code, 0, listed.err);
  assert.match(listed.out, /• #2 {2}2026-10-19 09:30 {2}Boss replied to c1 on home: On it/);
  assert.match(listed.out, /http:\/\/charet\.test\/p\/web\/t\/screen\/home\?comment=1/);
  assert.match(listed.out, /1 unread/);

  const read = await charet(["inbox", "read", "#2"], server.fetch);
  assert.equal(read.code, 0, read.err);
  assert.deepEqual(server.reads, [{ ids: [2] }]);
  assert.match((await charet(["inbox"], server.fetch)).out, /No unread notifications/);
  assert.match((await charet(["inbox", "--all"], server.fetch)).out, /#2/);
  assert.equal((await charet(["inbox", "read", "x"], server.fetch)).code, 1);
});
