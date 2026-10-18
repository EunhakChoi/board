// Vite plugin for board projects:
// - serves `.board/manifest.json` in dev and writes it into the build output
// - adds `.board/frame.html` (the screen and story renderer) as a build entry
// - marks host JSX elements with `data-src` (see source.ts)

import { existsSync, mkdirSync, writeFileSync } from "node:fs";
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

export function board(options: BoardPluginOptions = {}): Plugin[] {
  const frameFile = options.frame ?? ".board/frame.html";
  let config: ResolvedConfig;
  const projectPath = () => {
    if (options.project) return join(config.root, options.project);
    const tsx = join(config.root, ".board/project.tsx");
    return existsSync(tsx) ? tsx : join(config.root, ".board/project.ts");
  };

  const manifest: Plugin = {
    name: "board:manifest",
    config(user) {
      const root = resolve(user.root ?? process.cwd());
      const frame = join(root, frameFile);
      if (!existsSync(frame)) return;
      const index = join(root, "index.html");
      // Build the app (when it has an index.html) and the frame side by side.
      return {
        build: {
          rollupOptions: {
            input: { ...(existsSync(index) ? { index } : {}), frame },
          },
        },
      };
    },
    configResolved(resolved) {
      config = resolved;
    },
    configureServer(server) {
      server.middlewares.use(async (req, res, next) => {
        const path = (req.url ?? "").split("?")[0];
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
        resolve: { alias: config.resolve.alias },
      });
      const file = join(resolve(config.root, config.build.outDir), MANIFEST_PATH);
      mkdirSync(dirname(file), { recursive: true });
      writeFileSync(file, `${JSON.stringify(toManifest(module.default), null, 2)}\n`);
    },
  };

  const source: Plugin = {
    name: "board:source",
    enforce: "pre",
    transform(code, id) {
      if (options.sourceLocations === false) return null;
      const file = id.split("?")[0] as string;
      if (!/\.[jt]sx$/.test(file) || file.includes(`${sep}node_modules${sep}`)) return null;
      const rel = relative(config.root, file).split(sep).join("/");
      // Only the app's own code: not dependencies outside the root, not board's frame files.
      if (rel.startsWith("..") || rel.startsWith(".board/")) return null;
      return addSourceLocations(code, rel) ?? null;
    },
  };

  return [manifest, source];
}
