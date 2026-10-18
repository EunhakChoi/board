import { existsSync, readdirSync, readFileSync, rmSync, statSync } from "node:fs";
import { basename, join, resolve } from "node:path";
import { type ApiError, type Client, createClient, type Schemas } from "@board/api-client";
import { type Key, type Locale, t } from "@board/i18n";
import {
  type CheckoutState,
  commitBase,
  findCheckout,
  git,
  initBase,
  localChanges,
  unpack,
  writeState,
} from "./checkout.ts";
import { describeSource, listFiles, resolveScope } from "./scope.ts";

export interface Context {
  server: string;
  token?: string;
  cwd: string;
  locale: Locale;
  json: boolean;
  out: (line: string) => void;
  /** Progress and notes (stderr), kept apart from output scripts parse. */
  err: (line: string) => void;
  env: NodeJS.ProcessEnv;
  fetch?: typeof globalThis.fetch;
  /** Asks a person at the terminal; absent when nobody is there (CI, pipes, `--yes`). */
  ask?: (question: string) => Promise<string>;
  /** Runs a program for its output (tests replace it); `undefined` = missing or failed. */
  sh?: (command: string, args: string[]) => string | undefined;
  /** How often to poll a building preview. */
  pollMs?: number;
}

/** A failure to report to the user, already translated. */
export class CliError extends Error {}

export function client(ctx: Context, server = ctx.server): Client {
  return createClient({
    baseUrl: server,
    ...(ctx.token ? { token: ctx.token } : {}),
    ...(ctx.fetch ? { fetch: ctx.fetch } : {}),
  });
}

function say(ctx: Context, key: Key, params: Record<string, string> = {}): void {
  ctx.out(t(key, params, ctx.locale));
}

function fail(error: ApiError | undefined, fallback: string): never {
  throw new CliError(error ? `${error.message} (${error.code})` : fallback);
}

function emit(ctx: Context, value: unknown, human: () => void): void {
  if (ctx.json) ctx.out(JSON.stringify(value, null, 2));
  else human();
}

/** The project named by `-p`, else the checkout the command runs in. */
export function projectOf(ctx: Context, flag: string | undefined): string {
  const project = flag ?? findCheckout(ctx.cwd)?.state.project;
  if (!project) throw new CliError(t("cli.error.noProject", {}, ctx.locale));
  return project;
}

export async function projects(ctx: Context): Promise<void> {
  const { data, error } = await client(ctx).GET("/api/projects");
  if (!data) fail(error, "request failed");
  emit(ctx, data, () => {
    if (data.projects.length === 0) say(ctx, "cli.projects.none");
    for (const p of data.projects) ctx.out(`${p.key}\t${p.title}\t${p.origin}`);
  });
}

/** Deletes a project for good, after the user types its key (or with `--yes`). */
export async function deleteProject(ctx: Context, key: string, yes: boolean): Promise<void> {
  if (!yes) {
    if (!ctx.ask) throw new CliError(t("cli.delete.needsYes", { key }, ctx.locale));
    const answer = await ctx.ask(t("cli.delete.confirm", { key }, ctx.locale));
    if (answer.trim() !== key) throw new CliError(t("cli.delete.cancelled", {}, ctx.locale));
  }
  const { error, response } = await client(ctx).DELETE("/api/projects/{key}", {
    params: { path: { key } },
  });
  if (!response.ok) fail(error, "request failed");
  emit(ctx, { deleted: key }, () => say(ctx, "cli.delete.done", { key }));
}

export interface ImportOptions {
  path: string;
  key?: string;
  title?: string;
  include: string[];
  dryRun: boolean;
}

export async function importProject(ctx: Context, options: ImportOptions): Promise<void> {
  const scope = resolveScope(resolve(ctx.cwd, options.path), options.include);
  const files = listFiles(scope);
  const key = options.key ?? defaultKey(scope.app === "." ? scope.root : scope.app);
  const source = describeSource(scope.root);
  const boardJson = {
    source,
    app: scope.app,
    packages: scope.packages,
    rootFiles: scope.rootFiles,
  };
  if (options.dryRun) {
    emit(ctx, { key, ...boardJson, files }, () => {
      say(ctx, "cli.import.plan", { key, count: String(files.length), source });
      for (const dir of scope.packages) ctx.out(`  ${dir}/`);
      for (const file of scope.rootFiles) ctx.out(`  ${file}`);
    });
    return;
  }
  const payload = files.map((path) => {
    const abs = join(scope.root, path);
    return {
      path,
      content: readFileSync(abs).toString("base64"),
      executable: (statSync(abs).mode & 0o111) !== 0,
    };
  });
  payload.push({
    path: "board.json",
    content: Buffer.from(`${JSON.stringify(boardJson, null, 2)}\n`).toString("base64"),
    executable: false,
  });
  const { data, error } = await client(ctx).POST("/api/projects", {
    body: {
      key,
      ...(options.title ? { title: options.title } : {}),
      import: { source, files: payload },
    },
  });
  if (!data) fail(error, "import failed");
  emit(ctx, data, () =>
    say(ctx, "cli.import.done", { key: data.project.key, count: String(files.length) }),
  );
}

/** A project key from a folder name: lowercase letters, digits and dashes. */
export function defaultKey(path: string): string {
  const key = basename(path)
    .toLowerCase()
    .replace(/[^a-z0-9-]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 63);
  return key || "project";
}

async function fetchArchive(
  ctx: Context,
  server: string,
  project: string,
  rev?: string,
): Promise<{ commit: string; archive: Buffer }> {
  const { data, error, response } = await client(ctx, server).GET("/api/projects/{key}/checkout", {
    params: { path: { key: project }, query: rev ? { rev } : {} },
    parseAs: "arrayBuffer",
  });
  if (!data) fail(error as ApiError | undefined, "checkout failed");
  const commit = response.headers.get("x-board-commit");
  if (!commit) throw new CliError("server sent no commit");
  return { commit, archive: Buffer.from(data as ArrayBuffer) };
}

export async function checkout(
  ctx: Context,
  project: string,
  dir: string | undefined,
  rev: string | undefined,
): Promise<void> {
  const target = resolve(ctx.cwd, dir ?? project);
  if (existsSync(target) && readdirSync(target).length > 0) {
    throw new CliError(t("cli.error.notEmpty", { dir: target }, ctx.locale));
  }
  const { commit, archive } = await fetchArchive(ctx, ctx.server, project, rev);
  unpack(target, archive);
  writeState(target, { server: ctx.server, project, base: commit });
  initBase(target, commit);
  emit(ctx, { dir: target, project, base: commit }, () =>
    say(ctx, "cli.checkout.done", { project, dir: target, commit: commit.slice(0, 10) }),
  );
}

function requireCheckout(ctx: Context): { dir: string; state: CheckoutState } {
  const found = findCheckout(ctx.cwd);
  if (!found) throw new CliError(t("cli.error.noCheckout", {}, ctx.locale));
  return found;
}

export async function push(ctx: Context, message: string | undefined): Promise<void> {
  const { dir, state } = requireCheckout(ctx);
  const changes = localChanges(dir);
  if (changes.length === 0) {
    say(ctx, "cli.push.nothing");
    return;
  }
  const files = changes.map((c) =>
    c.content
      ? { path: c.path, content: c.content.toString("base64"), executable: c.executable }
      : { path: c.path },
  );
  const { data, error } = await client(ctx, state.server).POST("/api/projects/{key}/push", {
    params: { path: { key: state.project } },
    body: { base: state.base, files, ...(message ? { message } : {}) },
  });
  if (!data) {
    if (error?.code === "conflict") throw new CliError(t("cli.push.conflict", {}, ctx.locale));
    fail(error, "push failed");
  }
  writeState(dir, { ...state, base: data.commit });
  commitBase(dir, data.commit);
  emit(ctx, data, () =>
    say(ctx, "cli.push.done", { count: String(changes.length), version: String(data.number) }),
  );
}

export async function pull(ctx: Context): Promise<void> {
  const { dir, state } = requireCheckout(ctx);
  if (localChanges(dir).length > 0) throw new CliError(t("cli.pull.dirty", {}, ctx.locale));
  const { commit, archive } = await fetchArchive(ctx, state.server, state.project);
  if (commit === state.base) {
    say(ctx, "cli.pull.upToDate");
    return;
  }
  // Replace the base's files with the new main. Ignored files (node_modules, build output) stay.
  for (const path of git(dir, ["ls-files", "-z"]).toString().split("\0").filter(Boolean)) {
    rmSync(join(dir, path), { force: true });
  }
  unpack(dir, archive);
  writeState(dir, { ...state, base: commit });
  commitBase(dir, commit);
  emit(ctx, { base: commit }, () => say(ctx, "cli.pull.done", { commit: commit.slice(0, 10) }));
}

export async function versions(ctx: Context, project: string): Promise<void> {
  const { data, error } = await client(ctx).GET("/api/projects/{key}/versions", {
    params: { path: { key: project } },
  });
  if (!data) fail(error, "request failed");
  emit(ctx, data, () => {
    for (const v of data.versions as Schemas["Version"][]) {
      ctx.out(`v${v.number}\t${v.kind}\t${v.commit.slice(0, 10)}\t${v.actor}\t${v.note}`);
    }
  });
}

export async function diff(
  ctx: Context,
  project: string,
  from: string,
  to?: string,
): Promise<void> {
  const { data, error } = await client(ctx).GET("/api/projects/{key}/diff", {
    params: { path: { key: project }, query: { from, ...(to ? { to } : {}) } },
  });
  if (!data) fail(error, "request failed");
  const mark = { added: "A", modified: "M", deleted: "D" } as const;
  emit(ctx, data, () => {
    for (const c of data.changes) ctx.out(`${mark[c.kind]}\t${c.path}`);
  });
}
