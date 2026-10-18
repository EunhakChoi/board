// `check` and `capture`: open screens and stories in a browser and report problems; capture also
// stores the pictures the web tool shows (and moves comment pins with them).

import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import type { Schemas } from "@board/api-client";
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

function print(ctx: Context, results: Result[], verb: (r: Result) => string): number {
  let failed = 0;
  for (const r of results) {
    const found = problems(r);
    if (found.length) failed++;
    ctx.out(`${found.length ? "FAIL" : "ok  "}  ${r.kind} ${r.target} (${r.theme})${verb(r)}`);
    for (const issue of found) ctx.out(`        ${issue}`);
  }
  ctx.out(
    failed
      ? t("cli.render.failed", { count: String(failed), total: String(results.length) }, ctx.locale)
      : t("cli.render.ok", { total: plural(results.length, "page") }, ctx.locale),
  );
  return failed ? 1 : 0;
}

async function body(ctx: Context, project: string, options: RenderOptions) {
  // Name targets by id; checking the id here gives a clearer error than the server's.
  if (options.targets.length) {
    const m = await manifest(ctx, project);
    for (const id of options.targets) targetOf(ctx, m, id);
  }
  return {
    ...(options.rev ? { rev: options.rev } : {}),
    targets: options.targets,
    themes: options.themes,
  };
}

export async function check(
  ctx: Context,
  project: string,
  options: RenderOptions,
): Promise<number> {
  const input = await body(ctx, project, options);
  const data = await whenBuilt(ctx, project, () =>
    client(ctx).POST("/api/projects/{key}/check", {
      params: { path: { key: project } },
      body: input,
    }),
  );
  if (ctx.json) {
    ctx.out(JSON.stringify(data, null, 2));
    return data.results.some((r) => problems(r).length) ? 1 : 0;
  }
  return print(ctx, data.results, () => "");
}

export async function capture(
  ctx: Context,
  project: string,
  options: RenderOptions,
): Promise<number> {
  const input = {
    ...(await body(ctx, project, options)),
    ...(options.force ? { force: true } : {}),
  };
  const data = await whenBuilt(ctx, project, () =>
    client(ctx).POST("/api/projects/{key}/captures", {
      params: { path: { key: project } },
      body: input,
    }),
  );
  const saved: string[] = [];
  if (options.out) {
    const dir = resolve(ctx.cwd, options.out);
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
    return data.results.some((r) => problems(r).length) ? 1 : 0;
  }
  const code = print(ctx, data.results, (r) => (r.captured ? "" : "  unchanged"));
  for (const file of saved) ctx.out(file);
  return code;
}
