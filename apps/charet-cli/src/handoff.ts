// Between the project and the product repo (story 07), for admins:
// `charet handoff [status]`, `charet handoff export`, `charet handoff mark vN`,
// `charet import --update`, and the drift report `charet status -p <project>` gives in a product repo.

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import type { Schemas } from "@charet/api-client";
import { t } from "@charet/i18n";
import { CliError, type Context, client, emit, fail, say } from "./commands.ts";
import { describeSource, listFiles, resolveScope, type Scope } from "./scope.ts";

/** What `charet.json` (written by `charet import`) says about the product repo. */
interface Settings {
  source?: string;
  app?: string;
  packages?: string[];
  rootFiles?: string[];
}

/** The full HEAD commit of the git repository holding `dir`, if any. */
export function repoCommit(dir: string): string | undefined {
  return git(dir, ["rev-parse", "HEAD"]);
}

function git(cwd: string, args: string[]): string | undefined {
  try {
    return execFileSync("git", args, { cwd, stdio: ["ignore", "pipe", "ignore"] })
      .toString()
      .trim();
  } catch {
    return undefined;
  }
}

/** Paths charet keeps to itself: never part of a handoff or of the drift report. */
function charetOnly(path: string): boolean {
  return path === "charet.json" || path.split("/").includes(".charet");
}

async function settings(ctx: Context, project: string): Promise<Settings> {
  const { data, error, response } = await client(ctx).GET("/api/projects/{key}/file", {
    params: { path: { key: project }, query: { path: "charet.json" } },
    parseAs: "text",
  });
  if (response.status === 404) return {};
  if (data === undefined) fail(error, "request failed");
  try {
    return JSON.parse(data as string) as Settings;
  } catch {
    return {};
  }
}

async function handoffState(ctx: Context, project: string): Promise<Schemas["HandoffStatus"]> {
  const { data, error } = await client(ctx).GET("/api/projects/{key}/handoff", {
    params: { path: { key: project } },
  });
  if (!data) fail(error, "request failed");
  return data;
}

/**
 * The imported scope in the product repo around `from`: the workspace root whose `app` (from
 * `charet.json`) has a package.json. Walks up from `from`, so any folder inside the repo works.
 */
function locate(from: string, saved: Settings): Scope | undefined {
  const app = saved.app ?? ".";
  let dir = resolve(from);
  for (;;) {
    const appDir = join(dir, app);
    if (existsSync(join(appDir, "package.json"))) {
      try {
        const extra = (saved.packages ?? [])
          .map((p) => join(dir, p))
          .filter((p) => existsSync(join(p, "package.json")));
        const scope = resolveScope(appDir, extra);
        if (scope.root === dir) return scope;
      } catch {}
    }
    const parent = dirname(dir);
    if (parent === dir) return undefined;
    dir = parent;
  }
}

/** `charet handoff [status]`, and `charet status -p <project>` outside a checkout. */
export async function status(ctx: Context, project: string): Promise<number> {
  const state = await handoffState(ctx, project);
  const saved = await settings(ctx, project);
  const scope = locate(ctx.cwd, saved);
  const base = state.baseline;
  const drift = scope && base?.repoCommit ? repoDrift(scope, base.repoCommit) : undefined;
  emit(
    ctx,
    {
      project,
      ...state,
      repo: scope ? { root: scope.root, head: repoCommit(scope.root) } : null,
      drift,
    },
    () => {
      ctx.out(`project   ${project}`);
      ctx.out(
        `baseline  ${
          base
            ? `v${base.version} ${base.kind}${base.repoCommit ? ` · repo ${base.repoCommit.slice(0, 10)}` : ""}`
            : t("cli.handoff.noBaseline", {}, ctx.locale)
        }`,
      );
      const pending = state.pending.map((n) => `v${n}`).join(" ");
      ctx.out(
        `handoff   ${
          state.changed.length === 0
            ? t("cli.handoff.nothing", {}, ctx.locale)
            : t(
                "cli.handoff.pending",
                { files: String(state.changed.length), versions: pending || "-" },
                ctx.locale,
              )
        }`,
      );
      if (!scope) {
        ctx.out(`repo      ${t("cli.handoff.noRepo", {}, ctx.locale)}`);
        return;
      }
      ctx.out(`repo      ${scope.root} (HEAD ${(repoCommit(scope.root) ?? "?").slice(0, 10)})`);
      if (!drift) {
        ctx.out(`drift     ${t("cli.handoff.noRepoCommit", {}, ctx.locale)}`);
        return;
      }
      if (drift.unknown) {
        ctx.out(
          `drift     ${t("cli.handoff.unknownCommit", { commit: base?.repoCommit?.slice(0, 10) ?? "" }, ctx.locale)}`,
        );
        return;
      }
      if (drift.commits.length === 0 && drift.uncommitted.length === 0) {
        ctx.out(`drift     ${t("cli.handoff.noDrift", {}, ctx.locale)}`);
        return;
      }
      ctx.out(
        `drift     ${t(
          "cli.handoff.drift",
          {
            commits: String(drift.commits.length),
            files: String(drift.uncommitted.length),
          },
          ctx.locale,
        )}`,
      );
      for (const line of drift.commits.slice(0, 20)) ctx.out(`  ${line}`);
      for (const line of drift.uncommitted.slice(0, 20)) ctx.out(`  ${line}`);
    },
  );
  return 0;
}

interface Drift {
  /** The baseline commit is not in this repository (another clone, or not fetched yet). */
  unknown: boolean;
  /** `<sha> <subject>` of commits since the baseline that touch the imported files. */
  commits: string[];
  /** `git status --short` lines for imported files changed but not committed. */
  uncommitted: string[];
}

function repoDrift(scope: Scope, since: string): Drift {
  if (git(scope.root, ["cat-file", "-e", `${since}^{commit}`]) === undefined) {
    return { unknown: true, commits: [], uncommitted: [] };
  }
  const paths = [
    ...scope.packages,
    ...scope.rootFiles,
    ":(exclude,glob)**/.charet/**",
    ":(exclude)charet.json",
  ];
  const lines = (text: string | undefined) => (text ?? "").split("\n").filter(Boolean);
  return {
    unknown: false,
    commits: lines(git(scope.root, ["log", "--format=%h %s", `${since}..HEAD`, "--", ...paths])),
    uncommitted: lines(git(scope.root, ["status", "--short", "--", ...paths])),
  };
}

/** `charet handoff export`: the patch since the baseline (or `--since vN`). */
export async function exportPatch(
  ctx: Context,
  project: string,
  options: { since?: string; withMocks: boolean; out?: string },
): Promise<void> {
  const since = options.since === undefined ? undefined : versionNumber(ctx, options.since);
  const { data, error } = await client(ctx).GET("/api/projects/{key}/handoff/export", {
    params: {
      path: { key: project },
      query: { ...(since ? { since } : {}), ...(options.withMocks ? { withMocks: true } : {}) },
    },
  });
  if (!data) fail(error, "export failed");
  if (ctx.json) {
    if (options.out) writeFileSync(resolve(ctx.cwd, options.out), data.patch);
    ctx.out(JSON.stringify(data, null, 2));
    return;
  }
  const vars = {
    from: String(data.fromVersion),
    to: String(data.toVersion),
    files: String(data.changes.length),
    repo: data.repoCommit?.slice(0, 10) ?? "?",
  };
  if (data.changes.length === 0) {
    ctx.err(t("cli.handoff.exportNothing", vars, ctx.locale));
    return;
  }
  if (options.out) {
    const file = resolve(ctx.cwd, options.out);
    writeFileSync(file, data.patch);
    say(ctx, "cli.handoff.exported", { ...vars, file });
    const letter = { added: "A", modified: "M", deleted: "D" } as const;
    for (const c of data.changes) ctx.out(`  ${letter[c.kind]} ${c.path}`);
    say(ctx, "cli.handoff.next", { file, to: vars.to });
    return;
  }
  // The patch alone on stdout, so `charet handoff export | git apply` works.
  ctx.err(t("cli.handoff.exportSummary", vars, ctx.locale));
  ctx.out(data.patch.replace(/\n$/, ""));
}

/** `charet handoff mark vN [--commit sha]`: the commit defaults to HEAD of the repo here. */
export async function mark(
  ctx: Context,
  project: string,
  version: string | undefined,
  commit: string | undefined,
): Promise<void> {
  const number = versionNumber(ctx, version);
  const sha = commit ?? repoCommit(ctx.cwd);
  if (!sha) throw new CliError(t("cli.handoff.needCommit", {}, ctx.locale));
  const { data, error } = await client(ctx).POST("/api/projects/{key}/versions/{number}/handoff", {
    params: { path: { key: project, number } },
    body: { commit: sha },
  });
  if (!data) fail(error, "mark failed");
  emit(ctx, data, () =>
    say(ctx, "cli.handoff.marked", { version: String(number), commit: sha.slice(0, 10) }),
  );
}

/** `charet import --update [path]`: the product repo's files again, as an import version. */
export async function updateImport(
  ctx: Context,
  project: string,
  path: string,
  dryRun: boolean,
): Promise<void> {
  const saved = await settings(ctx, project);
  const scope = locate(resolve(ctx.cwd, path), saved);
  if (!scope) {
    throw new CliError(t("cli.handoff.noRepoFor", { app: saved.app ?? "." }, ctx.locale));
  }
  const files = listFiles(scope).filter((p) => !charetOnly(p));
  const source = describeSource(scope.root);
  const commit = repoCommit(scope.root);
  if (dryRun) {
    emit(ctx, { project, root: scope.root, source, commit, files }, () =>
      say(ctx, "cli.import.updatePlan", { key: project, count: String(files.length), source }),
    );
    return;
  }
  const charetJson = {
    ...saved,
    source,
    app: scope.app,
    packages: scope.packages,
    rootFiles: scope.rootFiles,
  };
  const payload = files.map((p) => {
    const abs = join(scope.root, p);
    return {
      path: p,
      content: readFileSync(abs).toString("base64"),
      executable: (statSync(abs).mode & 0o111) !== 0,
    };
  });
  payload.push({
    path: "charet.json",
    content: Buffer.from(`${JSON.stringify(charetJson, null, 2)}\n`).toString("base64"),
    executable: false,
  });
  const { data, error } = await client(ctx).POST("/api/projects/{key}/import", {
    params: { path: { key: project } },
    body: { source, ...(commit ? { commit } : {}), files: payload },
  });
  if (!data) {
    if (error?.code === "clash") {
      const clashed = ((error.detail as { files?: string[] } | undefined)?.files ?? []).join(", ");
      throw new CliError(t("cli.import.updateClash", { files: clashed }, ctx.locale));
    }
    fail(error, "import failed");
  }
  emit(ctx, data, () =>
    say(ctx, "cli.import.updated", { version: String(data.number), count: String(files.length) }),
  );
}

function versionNumber(ctx: Context, text: string | undefined): number {
  const number = Number(text?.replace(/^v/, ""));
  if (!Number.isInteger(number) || number < 1) {
    throw new CliError(t("cli.usage.handoff", {}, ctx.locale));
  }
  return number;
}
