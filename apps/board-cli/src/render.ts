// `check` and `capture`: open screens and stories in a browser and report problems; capture also
// stores the pictures the web tool shows (and moves comment pins with them).

import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import type { ApiError, Schemas } from "@board/api-client";
import { t } from "@board/i18n";
import { type Context, client } from "./commands.ts";
import { fail, manifest, plural, type Theme, targetOf, whenBuilt } from "./data.ts";

type Result = Schemas["RenderResult"];

export interface RenderOptions {
  targets: string[];
  themes: Theme[];
  rev?: string;
  force?: boolean;
  /** Capture only: also save the pictures into this folder. */
  out?: string;
}

function problems(r: Result): string[] {
  if (r.ready === "true" || r.issues.length > 0) return r.issues;
  return [r.ready ? `ready: ${r.ready}` : "never got ready"];
}

/** Targets per request: long runs go in batches, so results show as they come. */
const BATCH = 12;

function line(ctx: Context, r: Result, verb: string): boolean {
  const found = problems(r);
  ctx.out(`${found.length ? "FAIL" : "ok  "}  ${r.kind} ${r.target} (${r.theme})${verb}`);
  for (const issue of found) ctx.out(`        ${issue}`);
  return found.length > 0;
}

function summary(ctx: Context, failed: number, total: number): number {
  ctx.out(
    failed
      ? t("cli.render.failed", { count: String(failed), total: String(total) }, ctx.locale)
      : t("cli.render.ok", { total: plural(total, "page") }, ctx.locale),
  );
  return failed ? 1 : 0;
}

/** The named targets (checked against the manifest), or every screen and story. */
async function targets(ctx: Context, project: string, options: RenderOptions): Promise<string[]> {
  const m = await manifest(ctx, project);
  if (options.targets.length === 0) return [...m.screens, ...m.stories].map((x) => x.id);
  for (const id of options.targets) targetOf(ctx, m, id);
  return options.targets;
}

type Results = { commit: string; results: Result[] };
type Call = (input: {
  rev?: string;
  targets: string[];
  themes: Theme[];
}) => Promise<{ data?: Results; error?: ApiError }>;

/** Runs `call` over the targets in batches, all at the commit the first batch used. */
async function batches(
  ctx: Context,
  project: string,
  options: RenderOptions,
  call: Call,
  each: (r: Result) => void,
): Promise<Results> {
  const ids = await targets(ctx, project, options);
  let rev = options.rev;
  let commit = "";
  const results: Result[] = [];
  for (let i = 0; i < ids.length; i += BATCH) {
    const data = await whenBuilt(ctx, project, () =>
      call({ ...(rev ? { rev } : {}), targets: ids.slice(i, i + BATCH), themes: options.themes }),
    );
    commit = data.commit;
    rev ??= data.commit;
    for (const r of data.results) each(r);
    results.push(...data.results);
  }
  return { commit, results };
}

export async function check(
  ctx: Context,
  project: string,
  options: RenderOptions,
): Promise<number> {
  let failed = 0;
  const data = await batches(
    ctx,
    project,
    options,
    (body) =>
      client(ctx).POST("/api/projects/{key}/check", { params: { path: { key: project } }, body }),
    (r) => {
      if (!ctx.json && line(ctx, r, "")) failed++;
      else if (problems(r).length) failed++;
    },
  );
  if (ctx.json) ctx.out(JSON.stringify(data, null, 2));
  else summary(ctx, failed, data.results.length);
  return failed ? 1 : 0;
}

export async function capture(
  ctx: Context,
  project: string,
  options: RenderOptions,
): Promise<number> {
  let failed = 0;
  const dir = options.out ? resolve(ctx.cwd, options.out) : undefined;
  const saved: string[] = [];
  const data = await batches(
    ctx,
    project,
    options,
    (body) =>
      client(ctx).POST("/api/projects/{key}/captures", {
        params: { path: { key: project } },
        body: { ...body, ...(options.force ? { force: true } : {}) },
      }),
    (r) => {
      if (!ctx.json && line(ctx, r, r.captured ? "" : "  unchanged")) failed++;
      else if (problems(r).length) failed++;
    },
  );
  if (dir) {
    mkdirSync(dir, { recursive: true });
    for (const r of data.results) {
      const { data: image, error } = await client(ctx).GET(
        "/api/projects/{key}/captures/{kind}/{target}/{theme}/image",
        {
          params: { path: { key: project, kind: r.kind, target: r.target, theme: r.theme } },
          parseAs: "arrayBuffer",
        },
      );
      if (!image) fail(error as never, "image download failed");
      const file = join(dir, `${r.target}${r.theme === "dark" ? ".dark" : ""}.jpg`);
      writeFileSync(file, Buffer.from(image as ArrayBuffer));
      saved.push(file);
    }
  }
  if (ctx.json) {
    ctx.out(JSON.stringify({ ...data, saved }, null, 2));
    return failed ? 1 : 0;
  }
  summary(ctx, failed, data.results.length);
  for (const file of saved) ctx.out(file);
  return failed ? 1 : 0;
}
