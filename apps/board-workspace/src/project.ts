// What the workspace needs to know about a project tree: where the app is, which package manager
// installs it, and which commands check it.

import { existsSync, readFileSync } from "node:fs";
import { dirname, join, normalize, resolve, sep } from "node:path";

export type PackageManager = "pnpm" | "npm";

export interface Layout {
  root: string;
  /** App directory (where vite runs), from `board.json` `app`; the root by default. */
  app: string;
  manager: PackageManager;
  /** `.board/vite.config.*` when the project keeps a board-only vite config. */
  viteConfig: string | undefined;
  scripts: Record<string, string>;
}

export class LayoutError extends Error {}

export function readLayout(root: string): Layout {
  const app = appDir(root);
  if (!existsSync(join(app, "package.json")))
    throw new LayoutError(`no package.json in ${rel(root, app)}`);
  const manager = existsSync(join(root, "pnpm-lock.yaml"))
    ? "pnpm"
    : existsSync(join(root, "package-lock.json"))
      ? "npm"
      : undefined;
  if (!manager)
    throw new LayoutError("no lockfile (pnpm-lock.yaml or package-lock.json) at the project root");
  const viteConfig = ["ts", "mts", "js", "mjs"]
    .map((ext) => join(app, ".board", `vite.config.${ext}`))
    .find((file) => existsSync(file));
  const pkg = JSON.parse(readFileSync(join(app, "package.json"), "utf8")) as {
    scripts?: Record<string, string>;
  };
  return { root, app, manager, viteConfig, scripts: pkg.scripts ?? {} };
}

function appDir(root: string): string {
  const file = join(root, "board.json");
  if (!existsSync(file)) return root;
  const { app } = JSON.parse(readFileSync(file, "utf8")) as { app?: unknown };
  if (typeof app !== "string" || app === "" || app === ".") return root;
  const dir = resolve(root, normalize(app));
  if (dir !== root && !dir.startsWith(root + sep))
    throw new LayoutError(`board.json app "${app}" leaves the project`);
  return dir;
}

/** Finds `node_modules/.bin/<name>` from the app up to the root (monorepos hoist tools). */
export function findBin(layout: Layout, name: string): string | undefined {
  for (let dir = layout.app; ; dir = dirname(dir)) {
    const bin = join(dir, "node_modules", ".bin", name);
    if (existsSync(bin)) return bin;
    if (dir === layout.root || dirname(dir) === dir) return undefined;
  }
}

function rel(root: string, dir: string): string {
  return dir === root ? "." : dir.slice(root.length + 1);
}
