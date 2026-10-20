import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { listFiles, parsePnpmPackages, resolveScope } from "./scope.ts";

function fixture(files: Record<string, string>): string {
  const root = mkdtempSync(join(tmpdir(), "charet-scope-"));
  for (const [path, content] of Object.entries(files)) {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), content);
  }
  return root;
}

const pkg = (name: string, deps: Record<string, string> = {}) =>
  JSON.stringify({ name, dependencies: deps });

test("parses the packages list of pnpm-workspace.yaml", () => {
  const yaml =
    "packages:\n  - apps/*\n  - 'packages/*' # libs\n  - \"!packages/old\"\nonlyBuilt: []\n";
  assert.deepEqual(parsePnpmPackages(yaml), ["apps/*", "packages/*", "!packages/old"]);
});

test("takes the app, its workspace dependencies and root config", () => {
  const root = fixture({
    "pnpm-workspace.yaml": "packages:\n  - apps/*\n  - packages/*\n",
    "package.json": "{}",
    "pnpm-lock.yaml": "",
    "tsconfig.base.json": "{}",
    "README.md": "not config",
    "apps/web/package.json": pkg("web", { "@x/api": "workspace:*", react: "^19.0.0" }),
    "apps/web/src/main.tsx": "",
    "apps/server/package.json": pkg("server"),
    "packages/api/package.json": pkg("@x/api", { "@x/util": "workspace:^" }),
    "packages/util/package.json": pkg("@x/util"),
    "packages/unused/package.json": pkg("@x/unused"),
  });
  try {
    const scope = resolveScope(join(root, "apps/web"));
    assert.equal(scope.app, "apps/web");
    assert.deepEqual(scope.packages, ["apps/web", "packages/api", "packages/util"]);
    assert.deepEqual(scope.rootFiles, [
      "package.json",
      "pnpm-lock.yaml",
      "pnpm-workspace.yaml",
      "tsconfig.base.json",
    ]);
    const files = listFiles(scope);
    assert.ok(files.includes("apps/web/src/main.tsx"));
    assert.ok(!files.some((f) => f.startsWith("apps/server")));
    assert.ok(!files.includes("README.md"));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("a standalone app is its own root", () => {
  const root = fixture({ "package.json": pkg("solo"), "src/a.ts": "", "node_modules/x/i.js": "" });
  try {
    const scope = resolveScope(root);
    assert.deepEqual([scope.app, scope.packages, scope.rootFiles], [".", ["."], []]);
    assert.deepEqual(listFiles(scope), ["package.json", "src/a.ts"]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("honours .gitignore inside a git repo and adds --include paths", () => {
  const root = fixture({
    "package.json": JSON.stringify({ workspaces: ["apps/*"] }),
    ".gitignore": "*.log\n",
    "apps/web/package.json": pkg("web"),
    "apps/web/a.ts": "",
    "apps/web/debug.log": "",
    "shared/types.ts": "",
  });
  try {
    execFileSync("git", ["init", "-q"], { cwd: root });
    const scope = resolveScope(join(root, "apps/web"), [join(root, "shared")]);
    assert.deepEqual(scope.packages, ["apps/web", "shared"]);
    const files = listFiles(scope);
    assert.ok(files.includes("apps/web/a.ts"));
    assert.ok(files.includes("shared/types.ts"));
    assert.ok(!files.includes("apps/web/debug.log"));
    assert.throws(() => resolveScope(join(root, "apps/web"), [tmpdir()]), /outside the workspace/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
