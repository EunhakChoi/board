// Commands that explain a project: what screens, stories, pages and flows it has, how screens
// connect, where things are defined, and links into the web tool.

import { t } from "@board/i18n";
import { CliError, type Context, client } from "./commands.ts";
import {
  type Capture,
  type Comment,
  captures,
  comments,
  type Flow,
  type Found,
  fail,
  type Kind,
  type Manifest,
  manifest,
  mustFind,
  plural,
  type Screen,
  system,
  table,
} from "./data.ts";

function emit(ctx: Context, value: unknown, human: () => void): void {
  if (ctx.json) ctx.out(JSON.stringify(value, null, 2));
  else human();
}

/** `light`, `light (stale)`, `light + dark`, or `not captured`. */
function captureState(list: Capture[], kind: Kind, target: string): string {
  const mine = list.filter((c) => c.kind === kind && c.target === target);
  if (mine.length === 0) return "not captured";
  return mine
    .sort((a, b) => a.theme.localeCompare(b.theme) * -1)
    .map(
      (c) =>
        `${c.theme}${c.stale ? " (stale)" : ""}${c.issues.length ? ` (${plural(c.issues.length, "issue")})` : ""}`,
    )
    .join(" + ");
}

function openCount(list: Comment[], kind: Kind, target: string): string {
  const open = list.filter((c) => c.kind === kind && c.target === target && c.status === "open");
  return open.length ? plural(open.length, "open comment") : "";
}

/** Screens and stories that need a capture: never captured, or stale in some theme. */
function needsCapture(list: Capture[], kind: Kind, target: string): boolean {
  const mine = list.filter((c) => c.kind === kind && c.target === target);
  return mine.length === 0 || mine.some((c) => c.stale);
}

export async function ls(ctx: Context, project: string, stale: boolean): Promise<void> {
  const [m, caps, notes] = await Promise.all([
    manifest(ctx, project),
    captures(ctx, project),
    comments(ctx, project),
  ]);
  if (stale) {
    const targets = [
      ...m.screens.map((s) => ({ kind: "screen" as const, id: s.id })),
      ...m.stories.map((s) => ({ kind: "story" as const, id: s.id })),
    ].filter((x) => needsCapture(caps, x.kind, x.id));
    emit(ctx, { targets }, () => {
      for (const x of targets) ctx.out(x.id);
    });
    return;
  }
  const rows = {
    screens: m.screens.map((s) => ({
      id: s.id,
      title: s.title,
      page: s.page,
      capture: captureState(caps, "screen", s.id),
      comments: openCount(notes, "screen", s.id),
    })),
    stories: m.stories.map((s) => ({
      id: s.id,
      title: s.title,
      component: s.component,
      capture: captureState(caps, "story", s.id),
      comments: openCount(notes, "story", s.id),
    })),
    flows: m.flows.map((f) => ({ id: f.id, title: f.title, steps: stepSummary(f) })),
  };
  emit(ctx, rows, () => {
    ctx.out(`Screens (${rows.screens.length})`);
    for (const line of table(
      rows.screens.map((r) => [r.id, r.title, r.page, r.capture, r.comments]),
    ))
      ctx.out(`  ${line}`);
    ctx.out("");
    ctx.out(`Stories (${rows.stories.length})`);
    for (const line of table(
      rows.stories.map((r) => [r.id, r.title, `<${r.component}>`, r.capture, r.comments]),
    ))
      ctx.out(`  ${line}`);
    ctx.out("");
    ctx.out(`Flows (${rows.flows.length})`);
    for (const line of table(rows.flows.map((r) => [r.id, r.title, r.steps]))) ctx.out(`  ${line}`);
  });
}

function stepSummary(flow: Flow): string {
  const planned = flow.steps.filter((s) => s.planned).length;
  return `${plural(flow.steps.length, "step")}${planned ? `, ${planned} planned` : ""}`;
}

function connectionLine(c: Manifest["connections"][number]): string {
  return `${c.from} → ${c.to}  "${c.label}" (${c.kind})`;
}

export interface ConnectionFilter {
  from?: string;
  to?: string;
  page?: string;
}

export async function connections(
  ctx: Context,
  project: string,
  filter: ConnectionFilter,
): Promise<void> {
  const m = await manifest(ctx, project);
  const onPage = (id: string) => m.screens.find((s) => s.id === id)?.page === filter.page;
  const list = m.connections.filter(
    (c) =>
      (!filter.from || c.from === filter.from) &&
      (!filter.to || c.to === filter.to) &&
      (!filter.page || onPage(c.from) || onPage(c.to)),
  );
  emit(ctx, { connections: list }, () => {
    if (list.length === 0) ctx.out(t("cli.connections.none", {}, ctx.locale));
    for (const c of list) ctx.out(connectionLine(c));
  });
}

/** The shortest chain of connections from one screen to another (breadth first). */
export function shortestPath(
  m: Manifest,
  from: string,
  to: string,
): Manifest["connections"] | undefined {
  const previous = new Map<string, Manifest["connections"][number]>();
  const seen = new Set([from]);
  const queue = [from];
  while (queue.length > 0) {
    const here = queue.shift() as string;
    if (here === to) {
      const path: Manifest["connections"] = [];
      for (let at = to; at !== from; ) {
        const step = previous.get(at) as Manifest["connections"][number];
        path.unshift(step);
        at = step.from;
      }
      return path;
    }
    for (const c of m.connections) {
      if (c.from === here && !seen.has(c.to)) {
        seen.add(c.to);
        previous.set(c.to, c);
        queue.push(c.to);
      }
    }
  }
  return undefined;
}

export async function path(
  ctx: Context,
  project: string,
  from: string,
  to: string,
  sheet?: string,
): Promise<number> {
  const m = await manifest(ctx, project);
  for (const id of [from, to]) {
    if (!m.screens.some((s) => s.id === id))
      throw new CliError(t("cli.error.notScreen", { id }, ctx.locale));
  }
  const found = shortestPath(m, from, to);
  emit(ctx, { from, to, path: found ?? null }, () => {
    if (!found) {
      ctx.out(t("cli.path.none", { from, to }, ctx.locale));
      return;
    }
    if (found.length === 0) ctx.out(from);
    found.forEach((c, i) => {
      if (i === 0) ctx.out(c.from);
      ctx.out(`  ↓ "${c.label}" (${c.kind})`);
      ctx.out(c.to);
    });
  });
  if (!found) return 1;
  if (sheet) {
    const ids = found.length ? [from, ...found.map((c) => c.to)] : [from];
    await writeSheet(
      ctx,
      project,
      ids.map((id) => `screen:${id}`),
      "light",
      sheet,
    );
  }
  return 0;
}

export async function map(ctx: Context, project: string, format: string): Promise<void> {
  const m = await manifest(ctx, project);
  if (format === "mermaid") {
    const id = (s: string) => s.replace(/[^A-Za-z0-9_]/g, "_");
    ctx.out("flowchart LR");
    for (const page of m.pages) {
      const screens = m.screens.filter((s) => s.page === page.id);
      if (screens.length === 0) continue;
      ctx.out(`  subgraph ${id(`page_${page.id}`)}["${page.title} (${page.route})"]`);
      for (const s of screens) ctx.out(`    ${id(s.id)}["${s.title}"]`);
      ctx.out("  end");
    }
    for (const c of m.connections) {
      const arrow = c.kind === "auto" ? "-.->" : c.kind === "error" ? "==>" : "-->";
      ctx.out(`  ${id(c.from)} ${arrow}|${c.label}| ${id(c.to)}`);
    }
    return;
  }
  if (format !== "text") throw new CliError(t("cli.error.format", { format }, ctx.locale));
  emit(ctx, { pages: m.pages, screens: m.screens, connections: m.connections }, () => {
    for (const page of m.pages) {
      ctx.out(`${page.title} (${page.route})`);
      for (const s of m.screens.filter((x) => x.page === page.id)) {
        ctx.out(`  ${s.id}  ${s.title}`);
        for (const c of m.connections.filter((x) => x.from === s.id))
          ctx.out(`    → ${c.to}  "${c.label}" (${c.kind})`);
      }
    }
  });
}

export async function flows(ctx: Context, project: string): Promise<void> {
  const m = await manifest(ctx, project);
  emit(ctx, { flows: m.flows }, () => {
    if (m.flows.length === 0) ctx.out(t("cli.flows.none", {}, ctx.locale));
    for (const line of table(m.flows.map((f) => [f.id, f.title, stepSummary(f)]))) ctx.out(line);
  });
}

export async function flow(
  ctx: Context,
  project: string,
  id: string,
  sheet?: string,
  theme = "light",
): Promise<void> {
  const [m, caps] = await Promise.all([manifest(ctx, project), captures(ctx, project)]);
  const found = m.flows.find((f) => f.id === id);
  if (!found) throw new CliError(t("cli.error.unknownFlow", { id }, ctx.locale));
  emit(ctx, found, () => printFlow(ctx, found, caps));
  if (sheet) {
    const items = found.steps.map((s) => (s.planned ? "planned" : `screen:${s.id}`));
    await writeSheet(ctx, project, items, theme, sheet);
  }
}

function printFlow(ctx: Context, f: Flow, caps: Capture[]): void {
  ctx.out(`flow ${f.id} — ${f.title}`);
  for (const line of table(
    f.steps.map((s, i) => [
      `${i + 1}.`,
      s.id,
      s.title,
      s.planned ? `planned${s.note ? `: ${s.note}` : ""}` : captureState(caps, "screen", s.id),
    ]),
  ))
    ctx.out(`  ${line}`);
}

/** Writes a sheet (captures side by side) of `items` (`screen:<id>`, `story:<id>`, `planned`). */
export async function writeSheet(
  ctx: Context,
  project: string,
  items: string[],
  theme: string,
  out: string,
): Promise<void> {
  const { writeFileSync } = await import("node:fs");
  const { resolve } = await import("node:path");
  const { data, error } = await client(ctx).GET("/api/projects/{key}/sheet", {
    params: {
      path: { key: project },
      query: { items: items.join(","), theme: theme as "light" | "dark" },
    },
    parseAs: "arrayBuffer",
  });
  if (!data) fail(error as never, "sheet failed");
  const file = resolve(ctx.cwd, out);
  writeFileSync(file, Buffer.from(data as ArrayBuffer));
  ctx.err(t("cli.sheet.wrote", { file }, ctx.locale));
}

export async function sheet(
  ctx: Context,
  project: string,
  ids: string[],
  theme: string,
  out: string | undefined,
): Promise<void> {
  if (!out) throw new CliError(t("cli.usage.sheet", {}, ctx.locale));
  if (ids.length === 0) throw new CliError(t("cli.usage.sheet", {}, ctx.locale));
  const m = await manifest(ctx, project);
  const items = ids.map((id) => {
    if (m.screens.some((s) => s.id === id)) return `screen:${id}`;
    if (m.stories.some((s) => s.id === id)) return `story:${id}`;
    if (m.flows.some((f) => f.id === id))
      throw new CliError(t("cli.sheet.useFlow", { id }, ctx.locale));
    throw new CliError(t("cli.error.notTarget", { id }, ctx.locale));
  });
  await writeSheet(ctx, project, items, theme, out);
}

/** The web tool's address for an id (or the project itself). */
export async function url(ctx: Context, project: string, id: string | undefined): Promise<void> {
  const info = await system(ctx);
  const base = `${(info.toolUrl ?? ctx.server).replace(/\/$/, "")}/p/${encodeURIComponent(project)}`;
  let link = base;
  if (id) {
    const m = await manifest(ctx, project);
    const found = mustFind(ctx, m, id);
    link = await linkOf(ctx, project, base, found);
  }
  emit(ctx, { url: link }, () => ctx.out(link));
}

async function linkOf(ctx: Context, project: string, base: string, found: Found): Promise<string> {
  switch (found.type) {
    case "screen":
      return `${base}/t/screen/${found.screen.id}`;
    case "story":
      return `${base}/t/story/${found.story.id}`;
    case "page":
      return `${base}/screens`;
    case "flow":
      return `${base}/flows`;
    case "component":
      return `${base}/components`;
    case "comment": {
      const c = await commentById(ctx, project, found.id);
      return `${base}/t/${c.kind}/${c.target}${c.theme === "dark" ? "?theme=dark" : ""}`;
    }
  }
}

export async function commentById(ctx: Context, project: string, id: number): Promise<Comment> {
  const { data, error } = await client(ctx).GET("/api/projects/{key}/comments/{id}", {
    params: { path: { key: project, id } },
  });
  if (!data) fail(error, "request failed");
  return data;
}

export async function show(ctx: Context, project: string, id: string, out?: string): Promise<void> {
  const m = await manifest(ctx, project);
  const found = mustFind(ctx, m, id);
  if (found.type === "comment") {
    const { showComment } = await import("./comments.ts");
    await showComment(ctx, project, found.id, out);
    return;
  }
  const [caps, notes] = await Promise.all([captures(ctx, project), comments(ctx, project)]);
  switch (found.type) {
    case "screen":
      return emit(ctx, { type: "screen", ...found.screen }, () =>
        showScreen(ctx, m, found.screen, caps, notes),
      );
    case "story": {
      const s = found.story;
      return emit(ctx, { type: "story", ...s }, () => {
        ctx.out(`story ${s.id} — ${s.title}`);
        const rows = [
          ["component", `<${s.component}>`],
          ["size", `${s.size.w}×${s.size.h}`],
          ...targetRows(caps, notes, "story", s.id),
        ];
        for (const line of table(rows)) ctx.out(`  ${line}`);
      });
    }
    case "page": {
      const p = found.page;
      const screens = m.screens.filter((s) => s.page === p.id);
      return emit(ctx, { type: "page", ...p, screens: screens.map((s) => s.id) }, () => {
        ctx.out(`page ${p.id} — ${p.title} (${p.route})`);
        for (const line of table(
          screens.map((s) => [s.id, s.title, captureState(caps, "screen", s.id)]),
        ))
          ctx.out(`  ${line}`);
      });
    }
    case "flow":
      return emit(ctx, { type: "flow", ...found.flow }, () => printFlow(ctx, found.flow, caps));
    case "component":
      return emit(
        ctx,
        { type: "component", component: found.component, stories: found.stories },
        () => {
          ctx.out(`component <${found.component}>`);
          for (const line of table(
            found.stories.map((s) => [s.id, s.title, captureState(caps, "story", s.id)]),
          ))
            ctx.out(`  ${line}`);
        },
      );
  }
}

function targetRows(caps: Capture[], notes: Comment[], kind: Kind, id: string): string[][] {
  const mine = caps.filter((c) => c.kind === kind && c.target === id);
  const rows = [["capture", captureState(caps, kind, id)]];
  const sources = [...new Set(mine.flatMap((c) => c.sources))].sort();
  if (sources.length) rows.push(["sources", sources.join(", ")]);
  for (const c of mine) for (const issue of c.issues) rows.push([`issue (${c.theme})`, issue]);
  for (const c of notes.filter((x) => x.kind === kind && x.target === id && x.status === "open"))
    rows.push(["comment", `c${c.id} ${c.author.name}: ${firstLine(c.body)}`]);
  return rows;
}

function showScreen(ctx: Context, m: Manifest, s: Screen, caps: Capture[], notes: Comment[]): void {
  const page = m.pages.find((p) => p.id === s.page);
  ctx.out(`screen ${s.id} — ${s.title}`);
  const rows = [
    ["page", page ? `${page.id} (${page.route})` : s.page],
    ["route", s.route],
    ["size", `${s.size.w}×${s.size.h}`],
  ];
  if (s.note) rows.push(["note", s.note]);
  for (const c of m.connections.filter((x) => x.to === s.id))
    rows.push(["from", `${c.from}  "${c.label}" (${c.kind})`]);
  for (const c of m.connections.filter((x) => x.from === s.id))
    rows.push(["to", `${c.to}  "${c.label}" (${c.kind})`]);
  for (const f of m.flows) {
    const steps = f.steps.flatMap((step, i) => (step.id === s.id ? [i + 1] : []));
    if (steps.length) rows.push(["flow", `${f.id} (step ${steps.join(", ")})`]);
  }
  rows.push(...targetRows(caps, notes, "screen", s.id));
  for (const line of table(rows)) ctx.out(`  ${line}`);
}

export function firstLine(text: string): string {
  const line = text.split("\n")[0] ?? "";
  return line.length > 80 ? `${line.slice(0, 79)}…` : line;
}

/** Prints a file of `main` with line numbers; `line` centres a window of `context` lines on it. */
export async function source(
  ctx: Context,
  project: string,
  spec: string,
  context: number,
): Promise<void> {
  const m = await manifest(ctx, project);
  const found = /^c\d+$/.test(spec)
    ? undefined
    : (m.screens.find((s) => s.id === spec) ?? m.stories.find((s) => s.id === spec));
  if (found) {
    // A screen or story: the files its capture rendered from.
    const kind: Kind = m.screens.some((s) => s.id === spec) ? "screen" : "story";
    const caps = (await captures(ctx, project)).filter((c) => c.kind === kind && c.target === spec);
    const sources = [...new Set(caps.flatMap((c) => c.sources))].sort();
    emit(ctx, { sources }, () => {
      if (sources.length === 0) ctx.out(t("cli.source.notCaptured", { id: spec }, ctx.locale));
      for (const s of sources) ctx.out(s);
    });
    return;
  }
  const match = /^(.*?)(?::(\d+))?(?::\d+)?$/.exec(spec);
  const file = match?.[1] ?? spec;
  const line = match?.[2] ? Number(match[2]) : undefined;
  const { path: repoPath, text } = await readFile(ctx, project, file);
  const lines = text.split("\n");
  if (lines.at(-1) === "") lines.pop();
  const from = line ? Math.max(1, line - context) : 1;
  const to = line ? Math.min(lines.length, line + context) : lines.length;
  emit(ctx, { path: repoPath, from, to, lines: lines.slice(from - 1, to) }, () => {
    ctx.out(`${repoPath}${line ? `:${line}` : ""}`);
    const width = String(to).length;
    for (let n = from; n <= to; n++) {
      ctx.out(`${n === line ? ">" : " "}${String(n).padStart(width)}  ${lines[n - 1]}`.trimEnd());
    }
  });
}

/** A text file of `main`, by repository path or by a path relative to the app (`board.json` `app`). */
async function readFile(
  ctx: Context,
  project: string,
  file: string,
): Promise<{ path: string; text: string }> {
  const get = async (path: string) => {
    const response = await (ctx.fetch ?? fetch)(
      `${ctx.server}/api/projects/${encodeURIComponent(project)}/file?path=${encodeURIComponent(path)}`,
      { headers: ctx.token ? { authorization: `Bearer ${ctx.token}` } : {} },
    );
    return response.ok ? response.text() : undefined;
  };
  const direct = await get(file);
  if (direct !== undefined) return { path: file, text: direct };
  const board = await get("board.json");
  const app = board ? (JSON.parse(board) as { app?: string }).app : undefined;
  if (app && app !== ".") {
    const inApp = await get(`${app}/${file}`);
    if (inApp !== undefined) return { path: `${app}/${file}`, text: inApp };
  }
  throw new CliError(t("cli.error.noFile", { path: file }, ctx.locale));
}
