// Golden tests: the CLI's human output for a fixed project (`testdata/demo.json`), served by a fake
// server. Update after an intended change with `BOARD_UPDATE_GOLDEN=1 pnpm test`.

import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, test } from "node:test";
import { run } from "./main.ts";

const DATA = JSON.parse(readFileSync(join(import.meta.dirname, "testdata/demo.json"), "utf8"));
const GOLDEN = join(import.meta.dirname, "testdata/golden");
const out = mkdtempSync(join(tmpdir(), "board-golden-"));
after(() => rmSync(out, { recursive: true, force: true }));

const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
const JPEG = new Uint8Array([0xff, 0xd8, 0xff, 0xd9]);

/** A fake board server over the fixture. */
async function fakeFetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
  const request = input instanceof Request ? input : new Request(input, init);
  const url = new URL(request.url);
  const path = url.pathname.replace(/^\/api\/projects\/demo/, "");
  const route = `${request.method} ${path}`;
  switch (route) {
    case "GET /api/system":
      return json(DATA.system);
    case "GET /manifest":
      return json(DATA.manifest);
    case "GET /captures":
      return json(DATA.captures);
    case "POST /captures":
      return json(DATA.capture);
    case "POST /check":
      return json(DATA.check);
    case "GET /comments": {
      const status = url.searchParams.get("status") ?? "open";
      const target = url.searchParams.get("target");
      const comments = (DATA.comments as { status: string; target: string }[]).filter(
        (c) => (status === "all" || c.status === status) && (!target || c.target === target),
      );
      return json({ comments });
    }
    case "GET /file": {
      const text = DATA.files[url.searchParams.get("path") ?? ""];
      return text === undefined
        ? json({ code: "not_found", message: "file not found" }, 404)
        : new Response(text, { headers: { "content-type": "text/plain; charset=utf-8" } });
    }
    case "GET /sheet":
      return new Response(JPEG, { headers: { "content-type": "image/jpeg" } });
  }
  const comment = /^GET \/comments\/(\d+)(\/snapshot)?$/.exec(route);
  if (comment) {
    const found = (DATA.comments as { id: number }[]).find((c) => c.id === Number(comment[1]));
    if (!found) return json({ code: "not_found", message: "comment not found" }, 404);
    return comment[2]
      ? new Response(JPEG, { headers: { "content-type": "image/jpeg" } })
      : json(found);
  }
  if (/^GET \/captures\/.+\/image$/.test(route))
    return new Response(JPEG, { headers: { "content-type": "image/jpeg" } });
  return json({ code: "not_found", message: `no fake for ${route}` }, 404);
}

async function board(args: string[]) {
  const lines: string[] = [];
  const code = await run([...args, "-p", "demo"], {
    locale: "en",
    cwd: out,
    env: {},
    fetch: fakeFetch,
    out: (line) => lines.push(line),
    err: (line) => lines.push(`[stderr] ${line}`),
  });
  return `$ board ${args.join(" ")}\n${lines.join("\n").replaceAll(out, "<out>")}\n[exit ${code}]\n`;
}

const CASES: Record<string, string[]> = {
  ls: ["ls"],
  "ls-stale": ["ls", "--stale"],
  "show-screen": ["show", "tasks-list"],
  "show-page": ["show", "tasks"],
  "show-flow": ["show", "triage"],
  "show-component": ["show", "Button"],
  "show-story": ["show", "button-default"],
  "show-unknown": ["show", "nope"],
  url: ["url", "tasks-open"],
  "url-comment": ["url", "c2"],
  connections: ["connections"],
  "connections-from": ["connections", "--from", "tasks-list"],
  path: ["path", "tasks-list", "settings-main"],
  "path-none": ["path", "settings-main", "tasks-list"],
  map: ["map"],
  "map-mermaid": ["map", "--format", "mermaid"],
  flows: ["flows"],
  flow: ["flow", "triage", "--sheet", "flow.jpg"],
  sheet: ["sheet", "tasks-list", "button-default", "--out", "s.jpg"],
  "source-line": ["source", "src/main.tsx:3", "--context", "1"],
  "source-screen": ["source", "tasks-list"],
  check: ["check"],
  capture: ["capture", "tasks-list", "--theme", "both", "--out", "shots"],
  comments: ["comments"],
  "comments-all": ["comments", "--all"],
  "comments-show": ["comments", "show", "c1", "--out", "notes"],
  "comments-show-detached": ["show", "c2", "--out", "notes"],
};

for (const [name, args] of Object.entries(CASES)) {
  test(`golden: ${name}`, async () => {
    const actual = await board(args);
    const file = join(GOLDEN, `${name}.txt`);
    if (process.env.BOARD_UPDATE_GOLDEN) writeFileSync(file, actual);
    assert.equal(actual, readFileSync(file, "utf8"));
  });
}
