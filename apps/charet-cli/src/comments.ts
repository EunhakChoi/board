// Comments: list the open ones, read one with what it points at, add, reply, resolve and reopen.

import { mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { t } from "@charet/i18n";
import { CliError, type Context, client } from "./commands.ts";
import { type Comment, fail, manifest, plural, type Theme, table, targetOf } from "./data.ts";
import { commentById, firstLine } from "./structure.ts";

export function parseCommentId(ctx: Context, id: string | undefined): number {
  const match = /^c?(\d+)$/.exec(id ?? "");
  if (!match) throw new CliError(t("cli.usage.comments", {}, ctx.locale));
  return Number(match[1]);
}

export interface ListOptions {
  status: "open" | "resolved" | "all";
  on?: string;
  /** Comments on this branch's screens instead of main's. */
  branch?: number;
}

/** Lists comments. Exit code 1 while some listed comment is open, so scripts can wait on it. */
export async function list(ctx: Context, project: string, options: ListOptions): Promise<number> {
  const query: { status: ListOptions["status"]; target?: string; branch?: number } = {
    status: options.status,
  };
  if (options.on) query.target = options.on;
  if (options.branch) query.branch = options.branch;
  const { data, error } = await client(ctx).GET("/api/projects/{key}/comments", {
    params: { path: { key: project }, query },
  });
  if (!data) fail(error, "request failed");
  const open = data.comments.some((c) => c.status === "open");
  if (ctx.json) {
    ctx.out(JSON.stringify(data, null, 2));
    return open ? 1 : 0;
  }
  if (data.comments.length === 0) ctx.out(t("cli.comments.none", {}, ctx.locale));
  for (const line of table(
    data.comments.map((c) => [
      `c${c.id}`,
      c.status,
      `${c.kind} ${c.target}${c.theme === "dark" ? " (dark)" : ""}`,
      `${c.author.name}: ${firstLine(c.body)}${c.replies.length ? ` [${plural(c.replies.length, "reply", "replies")}]` : ""}`,
    ]),
  ))
    ctx.out(line);
  return open ? 1 : 0;
}

/** One comment in full; its snapshot (the screen when it was written) is saved for a look. */
export async function showComment(
  ctx: Context,
  project: string,
  id: number,
  out: string | undefined,
): Promise<void> {
  const c = await commentById(ctx, project, id);
  const dir = out ? resolve(ctx.cwd, out) : join(tmpdir(), "charet", project);
  const file = join(dir, `c${c.id}.jpg`);
  const { data } = await client(ctx).GET("/api/projects/{key}/comments/{id}/snapshot", {
    params: { path: { key: project, id } },
    parseAs: "arrayBuffer",
  });
  let snapshot: string | undefined;
  if (data) {
    mkdirSync(dir, { recursive: true });
    writeFileSync(file, Buffer.from(data as ArrayBuffer));
    snapshot = file;
  }
  if (ctx.json) {
    ctx.out(JSON.stringify({ ...c, snapshot: snapshot ?? null }, null, 2));
    return;
  }
  const a = c.anchor;
  ctx.out(`c${c.id}  ${c.status}  ${c.kind} ${c.target} (${c.theme})`);
  ctx.out(`${c.author.name}, ${c.createdAt}`);
  for (const line of c.body.split("\n")) ctx.out(`  ${line}`);
  ctx.out("");
  const rows = [
    [
      "box",
      `${a.x},${a.y} ${a.w}×${a.h}${a.detached ? "  (detached: the element is gone; this is where it was)" : ""}`,
    ],
  ];
  if (a.tag) rows.push(["element", `<${a.tag}>${a.text ? ` "${firstLine(a.text)}"` : ""}`]);
  if (a.sel) rows.push(["selector", a.sel]);
  if (a.chain.length) rows.push(["components", a.chain.join(" › ")]);
  if (a.src) rows.push(["source", a.src]);
  if (snapshot) rows.push(["snapshot", snapshot]);
  for (const line of table(rows)) ctx.out(line);
  if (c.replies.length) ctx.out("");
  for (const r of c.replies) {
    const what = r.kind === "reply" ? "" : ` (${r.kind === "resolve" ? "resolved" : "reopened"})`;
    ctx.out(`  ↳ ${r.author.name}${what}, ${r.createdAt}${r.body ? `: ${r.body}` : ""}`);
  }
}

export async function add(
  ctx: Context,
  project: string,
  target: string | undefined,
  at: string | undefined,
  body: string | undefined,
  theme: Theme,
): Promise<void> {
  if (!target || !at || !body) throw new CliError(t("cli.usage.commentsAdd", {}, ctx.locale));
  const m = await manifest(ctx, project);
  const { kind } = targetOf(ctx, m, target);
  const point = /^(\d+),(\d+)$/.exec(at);
  const where = point ? { x: Number(point[1]), y: Number(point[2]) } : { selector: at };
  const { data, error } = await client(ctx).POST("/api/projects/{key}/comments", {
    params: { path: { key: project } },
    body: { kind, target, theme, at: where, body },
  });
  if (!data) fail(error, "request failed");
  if (ctx.json) ctx.out(JSON.stringify(data, null, 2));
  else ctx.out(t("cli.comments.added", { id: `c${data.id}`, target }, ctx.locale));
}

export async function answer(
  ctx: Context,
  project: string,
  action: "reply" | "resolve" | "reopen",
  id: number,
  body: string | undefined,
): Promise<void> {
  if (action === "reply" && !body) throw new CliError(t("cli.usage.commentsReply", {}, ctx.locale));
  const current: Comment = await commentById(ctx, project, id);
  const path = {
    params: { path: { key: project, id } },
    body: { rev: current.rev, ...(body ? { body } : {}) },
  };
  const api = client(ctx);
  const { data, error } =
    action === "reply"
      ? await api.POST("/api/projects/{key}/comments/{id}/replies", path)
      : action === "resolve"
        ? await api.POST("/api/projects/{key}/comments/{id}/resolve", path)
        : await api.POST("/api/projects/{key}/comments/{id}/reopen", path);
  if (!data) {
    if (error?.code === "changed")
      throw new CliError(t("cli.comments.changed", { id: `c${id}` }, ctx.locale));
    fail(error, "request failed");
  }
  if (ctx.json) ctx.out(JSON.stringify(data, null, 2));
  else {
    const done = {
      reply: "cli.comments.replied",
      resolve: "cli.comments.resolved",
      reopen: "cli.comments.reopened",
    } as const;
    ctx.out(t(done[action], { id: `c${id}` }, ctx.locale));
  }
}
