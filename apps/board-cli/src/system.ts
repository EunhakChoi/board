// Getting started and staying healthy: `setup`, `doctor`, `serve`, `dev`, `status`, `init`,
// `skill install`, and the new-version notice.

import { type ChildProcess, execFileSync, spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { cpSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createConnection } from "node:net";
import { homedir } from "node:os";
import { delimiter, dirname, join, resolve } from "node:path";
import { t } from "@board/i18n";
import pkg from "../package.json" with { type: "json" };
import { findCheckout, localChanges } from "./checkout.ts";
import { CliError, type Context, checkout, client, importProject } from "./commands.ts";
import { fail, system } from "./data.ts";

/** The board repository (releases, stars). */
export const REPOSITORY = "EunhakChoi/board";
const REPO_ROOT = resolve(import.meta.dirname, "../../..");
const SKILL_DIR = resolve(import.meta.dirname, "../skill");

// ---- user settings ----------------------------------------------------------------------------

interface Settings {
  /** The answer to the star question; asked once. */
  star?: "yes" | "no";
  /** When the new-version notice last ran (ISO time). */
  updateNoticeAt?: string;
}

function settingsFile(ctx: Context): string {
  const env = ctx.env;
  const dir =
    env.BOARD_CONFIG_DIR ??
    join(env.XDG_CONFIG_HOME ?? join(env.HOME ?? homedir(), ".config"), "board");
  return join(dir, "settings.json");
}

export function readSettings(ctx: Context): Settings {
  try {
    return JSON.parse(readFileSync(settingsFile(ctx), "utf8")) as Settings;
  } catch {
    return {};
  }
}

function writeSettings(ctx: Context, settings: Settings): void {
  const file = settingsFile(ctx);
  mkdirSync(dirname(file), { recursive: true });
  writeFileSync(file, `${JSON.stringify(settings, null, 2)}\n`);
}

// ---- helpers ----------------------------------------------------------------------------------

/** Runs a program and returns its trimmed output, or `undefined` when it is missing or fails. */
function output(ctx: Context, command: string, args: string[]): string | undefined {
  if (ctx.sh) return ctx.sh(command, args);
  try {
    return execFileSync(command, args, { stdio: ["ignore", "pipe", "ignore"], timeout: 15_000 })
      .toString()
      .trim();
  } catch {
    return undefined;
  }
}

function which(ctx: Context, name: string): string | undefined {
  for (const dir of (ctx.env.PATH ?? "").split(delimiter)) {
    if (dir && existsSync(join(dir, name))) return join(dir, name);
  }
  return undefined;
}

async function yes(ctx: Context, question: string): Promise<boolean> {
  if (!ctx.ask) return false;
  const answer = (await ctx.ask(`${question} [Y/n] `)).trim().toLowerCase();
  return answer === "" || answer === "y" || answer === "yes";
}

/** Compares dotted versions numerically (`0.10.0` > `0.9.1`); pre-release tags sort first. */
export function newer(a: string, b: string): boolean {
  const parts = (v: string) => v.split(/[.-]/).map((x) => (/^\d+$/.test(x) ? Number(x) : -1));
  const [x, y] = [parts(a), parts(b)];
  for (let i = 0; i < Math.max(x.length, y.length); i++) {
    const d = (x[i] ?? 0) - (y[i] ?? 0);
    if (d !== 0) return d > 0;
  }
  return false;
}

// ---- setup ------------------------------------------------------------------------------------

export async function setup(ctx: Context, assumeYes: boolean): Promise<number> {
  ctx.out(t("cli.setup.welcome", { version: pkg.version }, ctx.locale));
  const reachable = await system(ctx).then(
    (info) => info,
    () => undefined,
  );
  if (reachable) {
    ctx.out(t("cli.setup.server", { url: reachable.toolUrl ?? ctx.server }, ctx.locale));
  } else {
    ctx.out(t("cli.setup.noServer", { server: ctx.server }, ctx.locale));
  }

  // Star question: a person at a terminal only, asked once; never in CI, docker or with --yes.
  const settings = readSettings(ctx);
  if (!assumeYes && ctx.ask && !settings.star && !ctx.env.CI) {
    const star = await yes(ctx, t("cli.setup.star", {}, ctx.locale));
    writeSettings(ctx, { ...settings, star: star ? "yes" : "no" });
    if (star) starRepository(ctx);
  }

  const agents = detectAgents(ctx);
  if (agents.length === 0) {
    ctx.out(t("cli.setup.noAgents", {}, ctx.locale));
  } else if (
    assumeYes ||
    (await yes(ctx, t("cli.setup.skill", { agents: agents.join(", ") }, ctx.locale)))
  ) {
    installSkill(ctx, agents);
  }
  ctx.out(t("cli.setup.next", {}, ctx.locale));
  return 0;
}

function starRepository(ctx: Context): void {
  const viaGh =
    output(ctx, "gh", ["auth", "status"]) !== undefined &&
    output(ctx, "gh", ["api", "-X", "PUT", `/user/starred/${REPOSITORY}`, "--silent"]) !==
      undefined;
  if (viaGh) {
    ctx.out(t("cli.setup.starred", {}, ctx.locale));
    return;
  }
  const url = `https://github.com/${REPOSITORY}`;
  const opener =
    process.platform === "darwin" ? "open" : process.platform === "win32" ? "explorer" : "xdg-open";
  if (output(ctx, opener, [url]) === undefined)
    ctx.out(t("cli.setup.starManually", { url }, ctx.locale));
  else ctx.out(t("cli.setup.starOpened", { url }, ctx.locale));
}

// ---- skill ------------------------------------------------------------------------------------

type Agent = "claude" | "codex";

function home(ctx: Context): string {
  return ctx.env.HOME ?? homedir();
}

function skillTarget(ctx: Context, agent: Agent): string {
  return agent === "claude"
    ? join(ctx.env.CLAUDE_CONFIG_DIR ?? join(home(ctx), ".claude"), "skills", "board")
    : join(ctx.env.CODEX_HOME ?? join(home(ctx), ".codex"), "skills", "board");
}

function detectAgents(ctx: Context): Agent[] {
  const found: Agent[] = [];
  if (existsSync(dirname(dirname(skillTarget(ctx, "claude"))))) found.push("claude");
  if (existsSync(dirname(dirname(skillTarget(ctx, "codex"))))) found.push("codex");
  return found;
}

function installSkill(ctx: Context, agents: Agent[]): void {
  for (const agent of agents) {
    const target = skillTarget(ctx, agent);
    mkdirSync(dirname(target), { recursive: true });
    cpSync(SKILL_DIR, target, { recursive: true, force: true });
    ctx.out(t("cli.skill.installed", { agent, dir: target }, ctx.locale));
  }
}

export function skill(
  ctx: Context,
  sub: string | undefined,
  claude: boolean,
  codex: boolean,
): number {
  if (sub !== "install") throw new CliError(t("cli.usage.skill", {}, ctx.locale));
  const chosen: Agent[] = [
    ...(claude ? ["claude" as const] : []),
    ...(codex ? ["codex" as const] : []),
  ];
  const agents = chosen.length ? chosen : detectAgents(ctx);
  installSkill(ctx, agents.length ? agents : ["claude"]);
  return 0;
}

// ---- doctor -----------------------------------------------------------------------------------

type Check = { state: "ok" | "fail" | "warn" | "info"; text: string; hint?: string };

function portOpen(port: number): Promise<boolean> {
  return new Promise((done) => {
    const socket = createConnection({ host: "127.0.0.1", port, timeout: 500 });
    socket.once("connect", () => {
      socket.destroy();
      done(true);
    });
    socket.once("error", () => done(false));
    socket.once("timeout", () => {
      socket.destroy();
      done(false);
    });
  });
}

export async function doctor(ctx: Context): Promise<number> {
  const checks: Check[] = [];
  const node = process.versions.node;
  const [major = 0] = node.split(".").map(Number);
  checks.push(
    major >= 24
      ? { state: "ok", text: `node ${node}` }
      : { state: "fail", text: `node ${node}`, hint: t("cli.doctor.node", {}, ctx.locale) },
  );
  const git = output(ctx, "git", ["--version"]);
  checks.push(
    git
      ? { state: "ok", text: git }
      : { state: "fail", text: "git: not found", hint: t("cli.doctor.git", {}, ctx.locale) },
  );

  const info = await system(ctx).catch(() => undefined);
  if (!info) {
    checks.push({
      state: "fail",
      text: `server ${ctx.server}: unreachable`,
      hint: t("cli.doctor.serve", {}, ctx.locale),
    });
    for (const [port, name] of [
      [5280, "board-server"],
      [5282, "board-workspace"],
      [5283, "board-render"],
    ] as const) {
      if (await portOpen(port))
        checks.push({ state: "info", text: `port ${port} (${name}) is in use` });
    }
  } else {
    checks.push({
      state: "ok",
      text: `server ${ctx.server} (board ${info.version}${info.localMode ? ", local mode" : ""})`,
    });
    for (const [name, service] of [
      ["board-workspace", info.services.workspace],
      ["board-render", info.services.render],
    ] as const) {
      checks.push(
        service.ok
          ? { state: "ok", text: name }
          : {
              state: "fail",
              text: `${name}${service.error ? `: ${service.error}` : ""}`,
              hint: t("cli.doctor.serve", {}, ctx.locale),
            },
      );
    }
    const host = (url: string | null) => (url ? new URL(url).hostname : "");
    if (info.toolUrl && info.previewUrl) {
      checks.push(
        host(info.toolUrl) !== host(info.previewUrl)
          ? { state: "ok", text: `web tool ${info.toolUrl}, previews ${info.previewUrl}` }
          : {
              state: "fail",
              text: `web tool ${info.toolUrl} and previews ${info.previewUrl} share a host`,
              hint: t("cli.doctor.sameHost", {}, ctx.locale),
            },
      );
    }
    if (info.latest && newer(info.latest, info.version)) {
      checks.push({
        state: "warn",
        text: t("cli.update.available", { latest: info.latest, version: info.version }, ctx.locale),
      });
    }
  }
  const docker = output(ctx, "docker", ["version", "--format", "{{.Server.Version}}"]);
  checks.push(
    docker
      ? { state: "info", text: `docker ${docker}` }
      : { state: "info", text: t("cli.doctor.noDocker", {}, ctx.locale) },
  );

  const failed = checks.some((c) => c.state === "fail");
  if (ctx.json) {
    ctx.out(JSON.stringify({ ok: !failed, checks }, null, 2));
  } else {
    const mark = { ok: "✓", fail: "✗", warn: "!", info: "-" } as const;
    for (const c of checks) {
      ctx.out(`${mark[c.state]} ${c.text}`);
      if (c.hint) ctx.out(`    ${c.hint}`);
    }
  }
  return failed ? 1 : 0;
}

// ---- serve ------------------------------------------------------------------------------------

function findProgram(
  ctx: Context,
  env: string,
  name: string,
  repoPaths: string[],
): string | undefined {
  const fromEnv = ctx.env[env];
  if (fromEnv) return fromEnv;
  return which(ctx, name) ?? repoPaths.map((p) => join(REPO_ROOT, p)).find((p) => existsSync(p));
}

/** Runs board-server, and unless `serverOnly`, board-workspace and board-render next to it. */
export async function serve(
  ctx: Context,
  options: { public: boolean; serverOnly: boolean },
): Promise<number> {
  const server = findProgram(ctx, "BOARD_SERVER_BIN", "board-server", [
    "target/release/board-server",
    "target/debug/board-server",
  ]);
  if (!server) throw new CliError(t("cli.serve.missing", { name: "board-server" }, ctx.locale));
  const children: ChildProcess[] = [];
  const env: NodeJS.ProcessEnv = { ...ctx.env };
  if (!options.serverOnly) {
    const render = findProgram(ctx, "BOARD_RENDER_BIN", "board-render", [
      "target/release/board-render",
      "target/debug/board-render",
    ]);
    const workspace = findProgram(ctx, "BOARD_WORKSPACE_MAIN", "board-workspace", [
      "apps/board-workspace/src/main.ts",
    ]);
    if (!render) throw new CliError(t("cli.serve.missing", { name: "board-render" }, ctx.locale));
    if (!workspace)
      throw new CliError(t("cli.serve.missing", { name: "board-workspace" }, ctx.locale));
    env.BOARD_WORKSPACE_TOKEN ??= randomBytes(24).toString("hex");
    env.BOARD_RENDER_TOKEN ??= randomBytes(24).toString("hex");
    children.push(spawn(render, [], { env, stdio: "inherit" }));
    children.push(
      workspace.endsWith(".ts")
        ? spawn(process.execPath, [workspace], { env, stdio: "inherit" })
        : spawn(workspace, [], { env, stdio: "inherit" }),
    );
  }
  children.push(spawn(server, options.public ? ["--public"] : [], { env, stdio: "inherit" }));
  return new Promise((done) => {
    let finished = false;
    const stop = (code: number) => {
      if (finished) return;
      finished = true;
      for (const child of children) child.kill("SIGTERM");
      done(code);
    };
    for (const child of children) child.once("exit", (code) => stop(code ?? 0));
    process.once("SIGINT", () => stop(0));
    process.once("SIGTERM", () => stop(0));
  });
}

// ---- checkout helpers: dev, status ------------------------------------------------------------

function appDir(dir: string): string {
  try {
    const app = (JSON.parse(readFileSync(join(dir, "board.json"), "utf8")) as { app?: string }).app;
    return app && app !== "." ? join(dir, app) : dir;
  } catch {
    return dir;
  }
}

function packageManager(dir: string): "pnpm" | "npm" | "yarn" {
  if (existsSync(join(dir, "pnpm-lock.yaml"))) return "pnpm";
  if (existsSync(join(dir, "yarn.lock"))) return "yarn";
  return "npm";
}

/** Runs the app's vite dev server in this checkout (installing first when needed). */
export async function dev(ctx: Context): Promise<number> {
  const found = findCheckout(ctx.cwd);
  if (!found) throw new CliError(t("cli.error.noCheckout", {}, ctx.locale));
  const pm = packageManager(found.dir);
  const run = (args: string[], cwd: string) =>
    new Promise<number>((done) => {
      const child = spawn(pm, args, { cwd, env: ctx.env, stdio: "inherit" });
      child.once("exit", (code) => done(code ?? 1));
      child.once("error", () => done(127));
    });
  if (!existsSync(join(found.dir, "node_modules"))) {
    const installed = await run(["install"], found.dir);
    if (installed !== 0) return installed;
  }
  return run(["run", "dev"], appDir(found.dir));
}

export async function status(ctx: Context): Promise<number> {
  const found = findCheckout(ctx.cwd);
  if (!found) throw new CliError(t("cli.error.noCheckout", {}, ctx.locale));
  const { dir, state } = found;
  const changes = localChanges(dir);
  const { data, error } = await client(ctx, state.server).GET("/api/projects/{key}/versions", {
    params: { path: { key: state.project } },
  });
  if (!data) fail(error, "request failed");
  const latest = data.versions[0];
  const base = data.versions.find((v) => v.commit === state.base);
  const behind = latest !== undefined && latest.commit !== state.base;
  if (ctx.json) {
    ctx.out(
      JSON.stringify(
        {
          dir,
          ...state,
          baseVersion: base?.number ?? null,
          latestVersion: latest?.number ?? null,
          behind,
          changes: changes.map((c) => ({ path: c.path, kind: c.kind })),
        },
        null,
        2,
      ),
    );
    return 0;
  }
  ctx.out(`project  ${state.project} (${state.server})`);
  ctx.out(`base     ${base ? `v${base.number} ` : ""}${state.base.slice(0, 10)}`);
  ctx.out(
    `main     ${latest ? `v${latest.number} ${latest.commit.slice(0, 10)}` : "?"}${behind ? `  ${t("cli.status.behind", {}, ctx.locale)}` : ""}`,
  );
  ctx.out(`changes  ${changes.length}`);
  for (const c of changes) ctx.out(`  ${c.kind} ${c.path}`);
  return 0;
}

// ---- init -------------------------------------------------------------------------------------

/** Creates a project from a template and checks it out into `dir`. */
export async function init(
  ctx: Context,
  key: string | undefined,
  dir: string | undefined,
  template: string,
  title: string | undefined,
): Promise<number> {
  if (!key) throw new CliError(t("cli.usage.init", {}, ctx.locale));
  const source = join(REPO_ROOT, "templates", template);
  if (!existsSync(join(source, "package.json"))) {
    throw new CliError(t("cli.init.noTemplate", { template }, ctx.locale));
  }
  const quiet = { ...ctx, out: () => {} };
  await importProject(quiet, {
    path: source,
    key,
    ...(title ? { title } : {}),
    include: [],
    dryRun: false,
  });
  await checkout(ctx, key, dir, undefined);
  return 0;
}

// ---- new-version notice -----------------------------------------------------------------------

/**
 * Once a day, tells a person at a terminal when the server knows of a newer release. The server
 * does the checking (`BOARD_UPDATE_CHECK`); the CLI only asks it, with a short timeout.
 */
export async function updateNotice(ctx: Context): Promise<void> {
  if (ctx.env.BOARD_UPDATE_CHECK === "false" || ctx.env.CI) return;
  const settings = readSettings(ctx);
  const last = settings.updateNoticeAt ? Date.parse(settings.updateNoticeAt) : 0;
  if (Date.now() - last < 24 * 60 * 60 * 1000) return;
  try {
    const response = await (ctx.fetch ?? fetch)(`${ctx.server}/api/system`, {
      signal: AbortSignal.timeout(800),
    });
    if (!response.ok) return;
    const info = (await response.json()) as { latest?: string | null };
    writeSettings(ctx, { ...settings, updateNoticeAt: new Date().toISOString() });
    if (info.latest && newer(info.latest, pkg.version)) {
      ctx.err(t("cli.update.available", { latest: info.latest, version: pkg.version }, ctx.locale));
    }
  } catch {
    // Offline or no server: say nothing.
  }
}
