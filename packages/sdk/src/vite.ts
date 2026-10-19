// Vite plugin for board projects:
// - serves `.board/manifest.json` in dev and writes it into the build output
// - adds `.board/frame.html` (the screen and story renderer) as a build entry
// - marks host JSX elements with `data-src` (see source.ts)
//
// It works in dev and in preview builds (`BOARD_PREVIEW=1`, set by board-workspace). A plain
// `vite build` of the app is left untouched, so the product's own build never carries board parts.

import { existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, relative, resolve, sep } from "node:path";
import { type Plugin, type ResolvedConfig, runnerImport } from "vite";
import { type ProjectDef, toManifest } from "./project.ts";
import { addSourceLocations } from "./source.ts";

export interface BoardPluginOptions {
  /** Project definition module, relative to the vite root (default `.board/project.tsx` or `.ts`). */
  project?: string;
  /** Frame page, relative to the vite root. */
  frame?: string;
  /** Add `data-src` to host elements (default true). */
  sourceLocations?: boolean;
}

export const MANIFEST_PATH = ".board/manifest.json";

/** The MSW service worker the frame registers, at the root of the build. */
export const WORKER_FILE = "mockServiceWorker.js";

/** Board-only static files (mock images, fixtures), served at the root next to the app's own. */
export const BOARD_PUBLIC = ".board/public";

/** Files under `dir`, as paths relative to it with `/` separators. */
function filesUnder(dir: string, prefix = ""): string[] {
  if (!existsSync(dir)) return [];
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    const rel = prefix ? `${prefix}/${name}` : name;
    return statSync(path).isDirectory() ? filesUnder(path, rel) : [rel];
  });
}

const CONTENT_TYPES: Record<string, string> = {
  ".js": "text/javascript",
  ".json": "application/json",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".webp": "image/webp",
  ".gif": "image/gif",
  ".pdf": "application/pdf",
  ".txt": "text/plain; charset=utf-8",
};

/** Packages the app and board's frame must share (a second React breaks hooks). */
const SHARED = ["react", "react-dom"] as const;
const BOARD_DIR = ".board";

/**
 * In a sandboxed preview (opaque origin) `localStorage`, `sessionStorage` and `document.cookie`
 * throw. Apps touch them at import time, so the frame swaps in memory-only stand-ins first.
 */
export const OPAQUE_SHIM = `(() => {
  if (self.origin !== "null") return;
  const memory = () => {
    const data = new Map();
    return {
      get length() { return data.size; },
      key: (i) => [...data.keys()][i] ?? null,
      getItem: (k) => (data.has(String(k)) ? data.get(String(k)) : null),
      setItem: (k, v) => { data.set(String(k), String(v)); },
      removeItem: (k) => { data.delete(String(k)); },
      clear: () => { data.clear(); },
    };
  };
  for (const name of ["localStorage", "sessionStorage"]) {
    try { window[name]; } catch { Object.defineProperty(window, name, { value: memory(), configurable: true }); }
  }
  try { document.cookie; } catch {
    const jar = new Map();
    Object.defineProperty(document, "cookie", {
      configurable: true,
      get: () => [...jar].map(([k, v]) => k + "=" + v).join("; "),
      set: (text) => {
        const [pair = ""] = String(text).split(";");
        const at = pair.indexOf("=");
        if (at > 0) jar.set(pair.slice(0, at).trim(), pair.slice(at + 1));
      },
    });
  }
})();`;

/** Set by board-workspace for preview builds. */
export const PREVIEW_ENV = "BOARD_PREVIEW";

export function board(options: BoardPluginOptions = {}): Plugin[] {
  const frameFile = options.frame ?? ".board/frame.html";
  let config: ResolvedConfig;
  const apply: Plugin["apply"] = (_config, env) =>
    env.command === "serve" || process.env[PREVIEW_ENV] === "1";
  const projectPath = () => {
    if (options.project) return join(config.root, options.project);
    const tsx = join(config.root, ".board/project.tsx");
    return existsSync(tsx) ? tsx : join(config.root, ".board/project.ts");
  };

  // The project's own copy in `public/` wins; otherwise the one from its msw package is served.
  const worker = (): string | undefined => {
    if (config.publicDir && existsSync(join(config.publicDir, WORKER_FILE))) return undefined;
    // msw sits in the app's dependencies or in `.board/package.json`.
    for (const from of [
      join(config.root, BOARD_DIR, "package.json"),
      join(config.root, "package.json"),
    ]) {
      try {
        return readFileSync(createRequire(from).resolve(`msw/${WORKER_FILE}`), "utf8");
      } catch {}
    }
    return undefined;
  };

  // Absolute paths to public files, which need the base prefix under a preview path.
  const publicFiles = new Map<string, boolean>();
  const isPublic = (url: string): boolean => {
    if (!url.startsWith("/") || url.startsWith("//") || url.length > 300) return false;
    const path = (url.split(/[?#]/)[0] as string).slice(1);
    if (!path || path.includes("..")) return false;
    let known = publicFiles.get(path);
    if (known === undefined) {
      const dirs = [config.publicDir, join(config.root, BOARD_PUBLIC)].filter(Boolean) as string[];
      known = dirs.some((dir) => existsSync(join(dir, path)) && statSync(join(dir, path)).isFile());
      publicFiles.set(path, known);
    }
    return known;
  };

  const manifest: Plugin = {
    name: "board:manifest",
    apply,
    config(user, env) {
      // One React for the app and board's frame, also when `.board/package.json` installs the SDK
      // apart from the app (its peers then resolve from the app root).
      const shared = { resolve: { dedupe: [...SHARED] } };
      if (env.command !== "build") return shared;
      const root = resolve(user.root ?? process.cwd());
      const frame = join(root, frameFile);
      const index = join(root, "index.html");
      return {
        ...shared,
        build: {
          // Component names stay readable in the element map (React fibers carry function names).
          minify: false,
          // Build the app (when it has an index.html) and the frame side by side.
          ...(existsSync(frame)
            ? { rollupOptions: { input: { ...(existsSync(index) ? { index } : {}), frame } } }
            : {}),
        },
      };
    },
    configResolved(resolved) {
      config = resolved;
    },
    transformIndexHtml: {
      order: "pre",
      handler(_html, ctx) {
        if (resolve(ctx.filename) !== join(config.root, frameFile)) return undefined;
        // Runs before any app module: where the build's root is (relative builds), and stand-ins
        // for storage and cookies, which a sandboxed preview does not have.
        const up = "../".repeat(frameFile.split("/").length - 1);
        const root = `globalThis.__BOARD_ROOT__ = new URL(${JSON.stringify(up || "./")}, location.href).href;`;
        return [{ tag: "script", children: root + OPAQUE_SHIM, injectTo: "head-prepend" }];
      },
    },
    generateBundle() {
      const source = worker();
      if (source) this.emitFile({ type: "asset", fileName: WORKER_FILE, source });
      const extra = join(config.root, BOARD_PUBLIC);
      for (const file of filesUnder(extra)) {
        this.emitFile({ type: "asset", fileName: file, source: readFileSync(join(extra, file)) });
      }
    },
    configureServer(server) {
      server.middlewares.use(async (req, res, next) => {
        const path = (req.url ?? "").split("?")[0];
        if (path === `${config.base}${WORKER_FILE}`) {
          const source = worker();
          if (!source) return next();
          res.setHeader("content-type", "text/javascript");
          res.end(source);
          return;
        }
        if (path?.startsWith(config.base)) {
          const rel = decodeURIComponent(path.slice(config.base.length));
          const file = resolve(config.root, BOARD_PUBLIC, rel);
          const extra = resolve(config.root, BOARD_PUBLIC);
          if (rel && file.startsWith(extra + sep) && existsSync(file) && statSync(file).isFile()) {
            const ext = file.slice(file.lastIndexOf(".")).toLowerCase();
            res.setHeader("content-type", CONTENT_TYPES[ext] ?? "application/octet-stream");
            res.end(readFileSync(file));
            return;
          }
        }
        if (path !== `${config.base}${MANIFEST_PATH}`) return next();
        try {
          const module = await server.ssrLoadModule(projectPath());
          res.setHeader("content-type", "application/json");
          res.end(JSON.stringify(toManifest(module.default as ProjectDef)));
        } catch (error) {
          res.statusCode = 500;
          res.setHeader("content-type", "application/json");
          res.end(JSON.stringify({ code: "invalid_project", message: String(error) }));
        }
      });
    },
    async closeBundle() {
      if (config.command !== "build" || config.build.ssr) return;
      const { module } = await runnerImport<{ default: ProjectDef }>(projectPath(), {
        root: config.root,
        configFile: false,
        logLevel: "error",
        // Same import aliases as the app (e.g. `@/components`), without loading its plugins again.
        resolve: { alias: config.resolve.alias, dedupe: [...SHARED] },
      });
      const file = join(resolve(config.root, config.build.outDir), MANIFEST_PATH);
      mkdirSync(dirname(file), { recursive: true });
      writeFileSync(file, `${JSON.stringify(toManifest(module.default), null, 2)}\n`);
    },
  };

  const source: Plugin = {
    name: "board:source",
    apply,
    enforce: "pre",
    transform(code, id) {
      const file = id.split("?")[0] as string;
      if (!/\.[mc]?[jt]sx?$/.test(file) || file.includes(`${sep}node_modules${sep}`)) return null;
      const rel = relative(config.root, file).split(sep).join("/");
      // Only the app's own code: not dependencies outside the root, not board's frame files.
      if (rel.startsWith("..") || rel.startsWith(".board/")) return null;
      const locations = options.sourceLocations !== false && /x$/.test(file);
      const rebase = config.base !== "/";
      if (!locations && !rebase) return null;
      return addSourceLocations(code, rel, { locations, ...(rebase ? { isPublic } : {}) }) ?? null;
    },
  };

  return [manifest, source];
}
