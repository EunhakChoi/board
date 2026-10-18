import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { readLayout } from "./project.ts";

function tree(files: Record<string, string>): string {
  const root = mkdtempSync(join(tmpdir(), "board-layout-"));
  for (const [path, content] of Object.entries(files)) {
    mkdirSync(join(root, path, ".."), { recursive: true });
    writeFileSync(join(root, path), content);
  }
  return root;
}

test("finds the app, the package manager and a board-only vite config", () => {
  const root = tree({
    "pnpm-lock.yaml": "",
    "board.json": JSON.stringify({ app: "apps/web" }),
    "apps/web/package.json": JSON.stringify({ scripts: { typecheck: "tsc -b" } }),
    "apps/web/.board/vite.config.ts": "",
  });
  try {
    const layout = readLayout(root);
    assert.equal(layout.app, join(root, "apps/web"));
    assert.equal(layout.manager, "pnpm");
    assert.equal(layout.viteConfig, join(root, "apps/web/.board/vite.config.ts"));
    assert.equal(layout.scripts.typecheck, "tsc -b");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("rejects a missing lockfile and an app outside the project", () => {
  const noLock = tree({ "package.json": "{}" });
  const outside = tree({ "package-lock.json": "", "board.json": JSON.stringify({ app: "../x" }) });
  try {
    assert.throws(() => readLayout(noLock), /no lockfile/);
    assert.throws(() => readLayout(outside), /leaves the project/);
  } finally {
    rmSync(noLock, { recursive: true, force: true });
    rmSync(outside, { recursive: true, force: true });
  }
});

test("finds board-only dependencies in .board/package.json, which need a lockfile", () => {
  const root = tree({
    "pnpm-lock.yaml": "",
    "package.json": "{}",
    ".board/package.json": "{}",
    ".board/package-lock.json": "",
  });
  const noLock = tree({ "pnpm-lock.yaml": "", "package.json": "{}", ".board/package.json": "{}" });
  try {
    assert.deepEqual(readLayout(root).boardPackage, { dir: join(root, ".board"), manager: "npm" });
    assert.equal(
      readLayout(tree({ "pnpm-lock.yaml": "", "package.json": "{}" })).boardPackage,
      undefined,
    );
    assert.throws(() => readLayout(noLock), /no lockfile next to .board\/package.json/);
  } finally {
    rmSync(root, { recursive: true, force: true });
    rmSync(noLock, { recursive: true, force: true });
  }
});
