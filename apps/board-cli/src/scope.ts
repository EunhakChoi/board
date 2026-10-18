// Decides what `board import` takes from a product repo: the app, the workspace packages it depends
// on (followed through `workspace:` dependencies), and the workspace's root config files.
// Paths in the store keep the repo's relative layout (docs/design D21).

import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";

/** Root-level files that the app's build needs from the workspace root. */
const ROOT_FILES = [
  "package.json",
  "pnpm-workspace.yaml",
  "pnpm-lock.yaml",
  "package-lock.json",
  ".npmrc",
  ".nvmrc",
  ".node-version",
  ".gitignore",
];

/** Directories never imported when walking a tree that is not a git repository. */
const SKIP_DIRS = new Set(["node_modules", ".git", "dist", "build", ".turbo", ".next", "coverage"]);

export interface Scope {
  /** Workspace root (absolute). Store paths are relative to it. */
  root: string;
  /** The app, relative to `root` (`"."` when the app is the root). */
  app: string;
  /** Package directories taken, relative to `root`, the app first. */
  packages: string[];
  /** Root-level files taken, relative to `root`. */
  rootFiles: string[];
}

interface Manifest {
  name?: string;
  workspaces?: string[] | { packages?: string[] };
  dependencies?: Record<string, string>;
  devDependencies?: Record<string, string>;
  peerDependencies?: Record<string, string>;
  optionalDependencies?: Record<string, string>;
}

function readManifest(dir: string): Manifest | undefined {
  const file = join(dir, "package.json");
  if (!existsSync(file)) return undefined;
  return JSON.parse(readFileSync(file, "utf8")) as Manifest;
}

/** Workspace package globs declared at `dir`, or undefined when `dir` is not a workspace root. */
function workspaceGlobs(dir: string): string[] | undefined {
  const pnpm = join(dir, "pnpm-workspace.yaml");
  if (existsSync(pnpm)) return parsePnpmPackages(readFileSync(pnpm, "utf8"));
  const workspaces = readManifest(dir)?.workspaces;
  if (Array.isArray(workspaces)) return workspaces;
  if (workspaces?.packages) return workspaces.packages;
  return undefined;
}

/** Reads the `packages:` list of a pnpm-workspace.yaml (the only key that matters here). */
export function parsePnpmPackages(yaml: string): string[] {
  const globs: string[] = [];
  let inPackages = false;
  for (const line of yaml.split("\n")) {
    if (/^packages\s*:/.test(line)) {
      inPackages = true;
      continue;
    }
    if (inPackages) {
      const item = /^\s+-\s+["']?([^"'#]+?)["']?\s*(#.*)?$/.exec(line);
      if (item?.[1]) globs.push(item[1]);
      else if (/^\S/.test(line)) inPackages = false;
    }
  }
  return globs;
}

/** Expands `dir`, `dir/*` and `dir/**` globs to package directories (relative to `root`). */
function expand(root: string, globs: string[]): string[] {
  const found = new Set<string>();
  const excluded = new Set<string>();
  for (const raw of globs) {
    const negate = raw.startsWith("!");
    const glob = (negate ? raw.slice(1) : raw).replace(/\/$/, "");
    const target = negate ? excluded : found;
    if (glob.endsWith("/**")) {
      walkPackages(root, glob.slice(0, -3), target);
    } else if (glob.endsWith("/*")) {
      const base = join(root, glob.slice(0, -2));
      if (!existsSync(base)) continue;
      for (const entry of readdirSync(base, { withFileTypes: true })) {
        const rel = join(glob.slice(0, -2), entry.name);
        if (entry.isDirectory() && existsSync(join(root, rel, "package.json"))) target.add(rel);
      }
    } else if (existsSync(join(root, glob, "package.json"))) {
      target.add(glob);
    }
  }
  return [...found].filter((dir) => !excluded.has(dir)).map(toPosix);
}

function walkPackages(root: string, base: string, into: Set<string>): void {
  const dir = join(root, base);
  if (!existsSync(dir)) return;
  if (existsSync(join(dir, "package.json"))) into.add(base);
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.isDirectory() && !SKIP_DIRS.has(entry.name)) {
      walkPackages(root, join(base, entry.name), into);
    }
  }
}

/** Finds the workspace root at or above `appDir` (stopping at the git root), else `appDir`. */
function findRoot(appDir: string): string {
  const stop = gitRoot(appDir);
  let dir = appDir;
  for (;;) {
    if (workspaceGlobs(dir)) return dir;
    if (dir === stop) return appDir;
    const parent = dirname(dir);
    if (parent === dir) return appDir;
    dir = parent;
  }
}

/** Works out the import scope for the app at `appPath`, plus any `include` paths. */
export function resolveScope(appPath: string, include: string[] = []): Scope {
  const appDir = resolve(appPath);
  if (!readManifest(appDir)) throw new Error(`no package.json in ${appDir}`);
  const root = findRoot(appDir);
  const app = toPosix(relative(root, appDir)) || ".";
  const byName = new Map<string, string>();
  for (const dir of expand(root, workspaceGlobs(root) ?? [])) {
    const name = readManifest(join(root, dir))?.name;
    if (name) byName.set(name, dir);
  }
  const packages: string[] = [];
  const queue = [app];
  while (queue.length > 0) {
    const dir = queue.shift() as string;
    if (packages.includes(dir)) continue;
    packages.push(dir);
    const manifest = readManifest(join(root, dir)) ?? {};
    const deps = {
      ...manifest.dependencies,
      ...manifest.devDependencies,
      ...manifest.peerDependencies,
      ...manifest.optionalDependencies,
    };
    for (const [name, spec] of Object.entries(deps)) {
      const local = byName.get(name);
      // pnpm marks local packages with `workspace:`; npm/yarn workspaces link by name.
      if (local && (spec.startsWith("workspace:") || !/^[\d^~<>=]/.test(spec) || spec === "*")) {
        queue.push(local);
      }
    }
  }
  for (const extra of include) {
    const rel = toPosix(relative(root, resolve(extra)));
    if (rel.startsWith("..")) throw new Error(`${extra} is outside the workspace ${root}`);
    if (!packages.includes(rel)) packages.push(rel);
  }
  const rootFiles =
    app === "."
      ? []
      : readdirSync(root).filter(
          (name) =>
            (ROOT_FILES.includes(name) || /^tsconfig.*\.json$/.test(name)) &&
            statSync(join(root, name)).isFile(),
        );
  return { root, app, packages, rootFiles: rootFiles.sort() };
}

/** Lists the files to import, relative to `scope.root`, honouring .gitignore inside git repos. */
export function listFiles(scope: Scope): string[] {
  const files = new Set(scope.rootFiles);
  const git = gitRoot(scope.root);
  for (const dir of scope.packages) {
    const abs = join(scope.root, dir);
    if (git) {
      const out = execFileSync(
        "git",
        ["ls-files", "-z", "--cached", "--others", "--exclude-standard", "--", abs],
        { cwd: git, maxBuffer: 1 << 30 },
      ).toString();
      for (const path of out.split("\0")) {
        if (!path) continue;
        const rel = toPosix(relative(scope.root, join(git, path)));
        if (existsSync(join(scope.root, rel))) files.add(rel);
      }
    } else {
      walkFiles(scope.root, dir === "." ? "" : dir, files);
    }
  }
  return [...files].sort();
}

function walkFiles(root: string, base: string, into: Set<string>): void {
  for (const entry of readdirSync(join(root, base), { withFileTypes: true })) {
    const rel = base ? `${base}/${entry.name}` : entry.name;
    if (entry.isDirectory()) {
      if (!SKIP_DIRS.has(entry.name)) walkFiles(root, rel, into);
    } else if (entry.isFile()) {
      into.add(rel);
    }
  }
}

/** The git top-level directory containing `dir`, or undefined. */
export function gitRoot(dir: string): string | undefined {
  try {
    return execFileSync("git", ["rev-parse", "--show-toplevel"], {
      cwd: dir,
      stdio: ["ignore", "pipe", "ignore"],
    })
      .toString()
      .trim();
  } catch {
    return undefined;
  }
}

/** Describes where an import came from: remote (or path) and commit, marked when dirty. */
export function describeSource(root: string): string {
  const git = gitRoot(root);
  if (!git) return root;
  const run = (args: string[]) => {
    try {
      return execFileSync("git", args, { cwd: git, stdio: ["ignore", "pipe", "ignore"] })
        .toString()
        .trim();
    } catch {
      return "";
    }
  };
  const remote = run(["config", "--get", "remote.origin.url"]) || git;
  const commit = run(["rev-parse", "--short", "HEAD"]);
  const dirty = run(["status", "--porcelain"]) ? " (uncommitted changes)" : "";
  return `${remote}${commit ? ` @ ${commit}` : ""}${dirty}`;
}

function toPosix(path: string): string {
  return path.split(sep).join("/");
}
