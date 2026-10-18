// Static builds of one commit: extract the source, install dependencies, optionally verify
// (typecheck · lint), and `vite build` under the preview base path. A ready build is kept by
// (project, commit) and never rebuilt; the source tree is removed once the job ends.

import {
  createWriteStream,
  existsSync,
  mkdirSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";
import type { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";
import { findBin, type Layout, LayoutError, readLayout } from "./project.ts";
import { childEnv, type RunResult, run } from "./run.ts";

export type StepName = "extract" | "install" | "typecheck" | "lint" | "build";
export type State = "queued" | "running" | "ready" | "failed";

export interface Step {
  name: StepName;
  state: "ok" | "failed" | "skipped";
  durationMs: number;
  /** Why it was skipped, or the end of its output when it failed. */
  detail: string;
}

export interface Build {
  project: string;
  commit: string;
  base: string;
  verify: boolean;
  state: State;
  steps: Step[];
  error: string;
  createdAt: string;
  finishedAt: string | null;
}

export interface BuildsOptions {
  home: string;
  /** Builds that may run at the same time. */
  concurrency: number;
  timeouts?: Partial<Record<StepName, number>>;
}

const DEFAULT_TIMEOUTS: Record<StepName, number> = {
  extract: 120_000,
  install: 600_000,
  typecheck: 300_000,
  lint: 300_000,
  build: 300_000,
};

export const validProject = (key: string) => /^[a-z0-9][a-z0-9-]{0,62}$/.test(key);
export const validCommit = (commit: string) => /^[0-9a-f]{40}([0-9a-f]{24})?$/.test(commit);

export class Builds {
  readonly #home: string;
  readonly #concurrency: number;
  readonly #timeouts: Record<StepName, number>;
  readonly #live = new Map<string, Build>();
  readonly #queue: { build: Build; source: string }[] = [];
  #running = 0;
  #idle: (() => void)[] = [];

  constructor(options: BuildsOptions) {
    this.#home = options.home;
    this.#concurrency = Math.max(1, options.concurrency);
    this.#timeouts = { ...DEFAULT_TIMEOUTS, ...options.timeouts };
    for (const dir of ["builds", "src", "uploads", "cache"])
      mkdirSync(join(this.#home, dir), { recursive: true });
    // Trees and uploads of jobs a previous process did not finish are useless now.
    rmSync(join(this.#home, "src"), { recursive: true, force: true });
    rmSync(join(this.#home, "uploads"), { recursive: true, force: true });
    mkdirSync(join(this.#home, "src"));
    mkdirSync(join(this.#home, "uploads"));
  }

  /** The build of `commit`, if one was ever started. */
  get(project: string, commit: string): Build | undefined {
    const live = this.#live.get(key(project, commit));
    if (live) return live;
    const file = join(this.#dir(project, commit), "build.json");
    if (!existsSync(file)) return undefined;
    const saved = JSON.parse(readFileSync(file, "utf8")) as Build;
    if (saved.state === "queued" || saved.state === "running") {
      // The process that ran it is gone.
      return { ...saved, state: "failed", error: "interrupted by a restart" };
    }
    return saved;
  }

  /**
   * Starts a build from a gzip tar of the commit's tree. A ready, queued or running build is
   * returned as is (the source is not read); a failed one runs again only with `retry`.
   */
  async start(
    request: { project: string; commit: string; base: string; verify: boolean; retry: boolean },
    source: Readable,
  ): Promise<{ build: Build; started: boolean }> {
    const existing = this.get(request.project, request.commit);
    if (existing && (existing.state !== "failed" || !request.retry)) {
      source.resume();
      return { build: existing, started: false };
    }
    const build: Build = {
      project: request.project,
      commit: request.commit,
      base: request.base,
      verify: request.verify,
      state: "queued",
      steps: [],
      error: "",
      createdAt: new Date().toISOString(),
      finishedAt: null,
    };
    this.#live.set(key(build.project, build.commit), build);
    const dir = this.#dir(build.project, build.commit);
    rmSync(dir, { recursive: true, force: true });
    mkdirSync(dir, { recursive: true });
    writeFileSync(this.logPath(build.project, build.commit), "");
    this.#save(build);
    // Keep the upload as a file so the job can start later without holding the request open.
    const upload = join(this.#home, "uploads", `${build.project}-${build.commit}.tar.gz`);
    try {
      await save(source, upload);
    } catch (error) {
      this.#finish(build, `upload failed: ${String(error)}`);
      return { build, started: true };
    }
    this.#queue.push({ build, source: upload });
    this.#pump();
    return { build, started: true };
  }

  /** Resolves when no build is queued or running (tests and shutdown). */
  idle(): Promise<void> {
    if (this.#running === 0 && this.#queue.length === 0) return Promise.resolve();
    return new Promise((resolve) => this.#idle.push(resolve));
  }

  remove(project: string, commit: string): boolean {
    const build = this.get(project, commit);
    if (!build || build.state === "queued" || build.state === "running") return false;
    rmSync(this.#dir(project, commit), { recursive: true, force: true });
    this.#live.delete(key(project, commit));
    return true;
  }

  logPath(project: string, commit: string): string {
    return join(this.#dir(project, commit), "log.txt");
  }

  /** Directory with the built files, once the build is ready. */
  outDir(project: string, commit: string): string {
    return join(this.#dir(project, commit), "out");
  }

  #dir(project: string, commit: string): string {
    return join(this.#home, "builds", project, commit);
  }

  #pump(): void {
    while (this.#running < this.#concurrency) {
      const next = this.#queue.shift();
      if (!next) break;
      this.#running++;
      void this.#run(next.build, next.source).finally(() => {
        this.#running--;
        this.#pump();
        if (this.#running === 0 && this.#queue.length === 0) {
          for (const resolve of this.#idle.splice(0)) resolve();
        }
      });
    }
  }

  async #run(build: Build, upload: string): Promise<void> {
    build.state = "running";
    this.#save(build);
    const tree = join(this.#home, "src", `${build.project}-${build.commit}`);
    const log = this.logPath(build.project, build.commit);
    const home = join(tree, ".home");
    try {
      mkdirSync(home, { recursive: true });
      const extracted = await this.#step(build, "extract", () =>
        run("tar", ["-xzf", upload, "-C", tree, "--no-same-owner", "--no-same-permissions"], {
          cwd: tree,
          env: childEnv(home),
          log,
          timeoutMs: this.#timeouts.extract,
        }),
      );
      if (!extracted) return this.#finish(build, "could not extract the source");

      let layout: Layout;
      try {
        layout = readLayout(tree);
      } catch (error) {
        if (error instanceof LayoutError) return this.#finish(build, error.message);
        throw error;
      }
      const env = childEnv(home, {
        // pnpm 11+ reads only pnpm_config_*; npm_config_* covers older pnpm.
        pnpm_config_store_dir: join(this.#home, "cache", "pnpm-store"),
        npm_config_store_dir: join(this.#home, "cache", "pnpm-store"),
        npm_config_cache: join(this.#home, "cache", "npm"),
      });
      const go = (cwd: string, timeoutMs: number) => ({ cwd, env, log, timeoutMs });

      // Dependency install scripts stay off (security.md): bundlers ship prebuilt binaries.
      const install =
        layout.manager === "pnpm"
          ? ["install", "--frozen-lockfile", "--prefer-offline", "--ignore-scripts"]
          : ["ci", "--prefer-offline", "--no-audit", "--no-fund", "--ignore-scripts"];
      if (
        !(await this.#step(build, "install", () =>
          run(layout.manager, install, go(tree, this.#timeouts.install)),
        ))
      ) {
        return this.#finish(build, "dependency install failed");
      }

      if (build.verify) {
        const typecheck = this.#check(layout, "typecheck", "tsc", ["--noEmit", "-p", "."]);
        const lint = this.#check(layout, "lint", "eslint", ["."]);
        for (const [name, command] of [
          ["typecheck", typecheck],
          ["lint", lint],
        ] as const) {
          if (!command) {
            this.#skip(build, name, `no ${name} script or config`);
            continue;
          }
          const ok = await this.#step(build, name, () =>
            run(command[0], command[1], go(layout.app, this.#timeouts[name])),
          );
          if (!ok) return this.#finish(build, `${name} failed`);
        }
      }

      const vite = findBin(layout, "vite");
      if (!vite) return this.#finish(build, "vite is not installed in the project");
      const out = this.outDir(build.project, build.commit);
      const args = ["build", "--outDir", out, "--emptyOutDir", "--base", build.base];
      if (layout.viteConfig) args.push("--config", layout.viteConfig);
      // BOARD_PREVIEW turns on @board/sdk/vite for this build (frame entry, manifest, data-src).
      const preview = {
        ...go(layout.app, this.#timeouts.build),
        env: { ...env, BOARD_PREVIEW: "1" },
      };
      if (!(await this.#step(build, "build", () => run(vite, args, preview)))) {
        return this.#finish(build, "vite build failed");
      }
      if (!existsSync(join(out, ".board", "manifest.json"))) {
        return this.#finish(
          build,
          "the build has no .board/manifest.json; add board() from @board/sdk/vite",
        );
      }
      build.state = "ready";
      this.#finish(build, "");
    } catch (error) {
      this.#finish(build, `internal error: ${String(error)}`);
    } finally {
      rmSync(tree, { recursive: true, force: true });
      rmSync(upload, { force: true });
    }
  }

  /** The project's own script when it has one, else the tool's default when its config exists. */
  #check(
    layout: Layout,
    script: "typecheck" | "lint",
    tool: string,
    args: string[],
  ): [string, string[]] | undefined {
    if (layout.scripts[script]) return [layout.manager, ["run", script]];
    const configured =
      script === "typecheck"
        ? existsSync(join(layout.app, "tsconfig.json"))
        : ["js", "mjs", "cjs", "ts", "mts"].some((ext) =>
            existsSync(join(layout.app, `eslint.config.${ext}`)),
          );
    const bin = configured ? findBin(layout, tool) : undefined;
    return bin ? [bin, args] : undefined;
  }

  async #step(build: Build, name: StepName, action: () => Promise<RunResult>): Promise<boolean> {
    const started = Date.now();
    const result = await action();
    const ok = result.code === 0;
    build.steps.push({
      name,
      state: ok ? "ok" : "failed",
      durationMs: Date.now() - started,
      detail: ok ? "" : result.timedOut ? `timed out\n${result.tail}` : result.tail,
    });
    this.#save(build);
    return ok;
  }

  #skip(build: Build, name: StepName, detail: string): void {
    build.steps.push({ name, state: "skipped", durationMs: 0, detail });
    this.#save(build);
  }

  #finish(build: Build, error: string): void {
    if (error) build.state = "failed";
    build.error = error;
    build.finishedAt = new Date().toISOString();
    this.#save(build);
    this.#live.delete(key(build.project, build.commit));
  }

  #save(build: Build): void {
    const dir = this.#dir(build.project, build.commit);
    const tmp = join(dir, "build.json.tmp");
    writeFileSync(tmp, `${JSON.stringify(build, null, 2)}\n`);
    renameSync(tmp, join(dir, "build.json"));
  }
}

function key(project: string, commit: string): string {
  return `${project}/${commit}`;
}

function save(source: Readable, file: string): Promise<void> {
  return pipeline(source, createWriteStream(file));
}
