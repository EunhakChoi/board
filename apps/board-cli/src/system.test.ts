import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, test } from "node:test";
import { run } from "./main.ts";
import { newer } from "./system.ts";

const home = mkdtempSync(join(tmpdir(), "board-system-"));
after(() => rmSync(home, { recursive: true, force: true }));

const json = (body: unknown) =>
  new Response(JSON.stringify(body), { headers: { "content-type": "application/json" } });

function system(overrides: Record<string, unknown> = {}) {
  return {
    version: "0.0.0",
    latest: null,
    toolUrl: "http://localhost:5280",
    previewUrl: "http://127.0.0.1:5281",
    localMode: true,
    services: {
      workspace: { ok: true },
      render: { ok: false, error: "board-render: connection refused" },
    },
    ...overrides,
  };
}

async function board(
  args: string[],
  options: {
    answers?: string[];
    sh?: (c: string, a: string[]) => string | undefined;
    info?: unknown;
    env?: NodeJS.ProcessEnv;
  } = {},
) {
  const lines: string[] = [];
  const asked: string[] = [];
  const answers = [...(options.answers ?? [])];
  const code = await run(args, {
    locale: "en",
    cwd: home,
    env: { HOME: home, BOARD_CONFIG_DIR: join(home, "config"), ...options.env },
    fetch: async () =>
      options.info === undefined
        ? Promise.reject(new TypeError("fetch failed"))
        : json(options.info),
    out: (line) => lines.push(line),
    err: (line) => lines.push(`[stderr] ${line}`),
    ...(options.answers
      ? {
          ask: async (q: string) => {
            asked.push(q);
            return answers.shift() ?? "";
          },
        }
      : {}),
    sh: options.sh ?? (() => undefined),
  });
  return { code, out: lines.join("\n"), asked };
}

test("compares versions numerically", () => {
  assert.ok(newer("0.10.0", "0.9.1"));
  assert.ok(newer("1.0.0", "1.0.0-rc.1"));
  assert.ok(!newer("0.1.0", "0.1.0"));
  assert.ok(!newer("0.1.0", "0.2.0"));
});

test("setup asks to star once, stars with gh, and installs the skill", async () => {
  mkdirSync(join(home, ".claude"), { recursive: true });
  const calls: string[] = [];
  const sh = (command: string, args: string[]) => {
    calls.push([command, ...args].join(" "));
    return command === "gh" ? "" : undefined;
  };
  const first = await board(["setup"], { answers: ["", ""], sh, info: system() });
  assert.equal(first.code, 0, first.out);
  assert.deepEqual(first.asked, [
    "Star board on GitHub? [Y/n] ",
    "Install the board skill for claude? [Y/n] ",
  ]);
  assert.ok(calls.includes("gh api -X PUT /user/starred/EunhakChoi/board --silent"));
  assert.match(first.out, /Starred/);
  assert.ok(existsSync(join(home, ".claude/skills/board/SKILL.md")));
  assert.ok(existsSync(join(home, ".claude/skills/board/references/cli.md")));
  assert.ok(existsSync(join(home, ".claude/skills/board/references/import.md")));
  assert.ok(!existsSync(join(home, ".claude/skills/board/evals")));
  assert.equal(JSON.parse(readFileSync(join(home, "config/settings.json"), "utf8")).star, "yes");

  const again = await board(["setup"], { answers: ["n"], sh, info: system() });
  assert.deepEqual(again.asked, ["Install the board skill for claude? [Y/n] "]);
});

test("setup never asks without a terminal or with --yes", async () => {
  const quiet = await board(["setup", "--yes"], {
    answers: [],
    env: { BOARD_CONFIG_DIR: join(home, "c2") },
  });
  assert.deepEqual(quiet.asked, []);
  assert.match(quiet.out, /No server at/);
  assert.match(quiet.out, /Installed the board skill for claude/);
});

test("skill install picks agents", async () => {
  const result = await board(["skill", "install", "--codex"]);
  assert.equal(result.code, 0);
  assert.ok(existsSync(join(home, ".codex/skills/board/SKILL.md")));
  assert.equal((await board(["skill", "nope"])).code, 1);
});

test("doctor reports failing services and newer releases", async () => {
  const sh = (command: string) => (command === "git" ? "git version 2.43.0" : undefined);
  const result = await board(["doctor"], { sh, info: system({ latest: "0.2.0" }) });
  assert.equal(result.code, 1);
  assert.match(result.out, /✓ git version 2\.43\.0/);
  assert.match(result.out, /✓ board-workspace/);
  assert.match(result.out, /✗ board-render: board-render: connection refused\n {4}Start it/);
  assert.match(
    result.out,
    /✓ web tool http:\/\/localhost:5280, previews http:\/\/127\.0\.0\.1:5281/,
  );
  assert.match(result.out, /! board 0\.2\.0 is out/);
  assert.match(result.out, /- docker: not found/);

  const shared = await board(["doctor", "--json"], {
    sh,
    info: system({
      previewUrl: "http://localhost:5281",
      services: { workspace: { ok: true }, render: { ok: true } },
    }),
  });
  const report = JSON.parse(shared.out);
  assert.equal(report.ok, false);
  assert.ok(report.checks.some((c: { text: string }) => /share a host/.test(c.text)));

  const down = await board(["doctor"], { sh });
  assert.match(down.out, /✗ server http:\/\/127\.0\.0\.1:5280: unreachable/);
});
