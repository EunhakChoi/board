// Reading a project's structure (manifest), captures and comments, shared by the commands that
// explain or check it.

import type { ApiError, Schemas } from "@board/api-client";
import { t } from "@board/i18n";
import { CliError, type Context, client } from "./commands.ts";

export type Manifest = Schemas["Manifest"];
export type Screen = Manifest["screens"][number];
export type Story = Manifest["stories"][number];
export type Flow = Manifest["flows"][number];
export type Capture = Schemas["Capture"];
export type Comment = Schemas["Comment"];
export type Kind = "screen" | "story";
export type Theme = "light" | "dark";

export function fail(error: ApiError | undefined, fallback: string): never {
  throw new CliError(error ? `${error.message} (${error.code})` : fallback);
}

const sleep = (ms: number) => new Promise((done) => setTimeout(done, ms));

/** Polls the preview of `main` until it is built; throws when the build failed. */
export async function waitForBuild(ctx: Context, project: string): Promise<void> {
  let told = false;
  for (let i = 0; i < 600; i++) {
    const { data, error } = await client(ctx).GET("/api/projects/{key}/preview", {
      params: { path: { key: project } },
    });
    if (!data) fail(error, "request failed");
    if (data.state === "ready") return;
    if (data.state === "failed") {
      throw new CliError(t("cli.build.failed", { error: data.error ?? "" }, ctx.locale));
    }
    if (!told) ctx.err(t("cli.build.waiting", {}, ctx.locale));
    told = true;
    await sleep(ctx.pollMs ?? 1000);
  }
  throw new CliError(t("cli.build.timeout", {}, ctx.locale));
}

/** Retries `call` while the server answers `pending` (the preview is still building). */
export async function whenBuilt<T>(
  ctx: Context,
  project: string,
  call: () => Promise<{ data?: T; error?: ApiError }>,
): Promise<T> {
  let result = await call();
  if (!result.data && result.error?.code === "pending") {
    await waitForBuild(ctx, project);
    result = await call();
  }
  if (!result.data) fail(result.error, "request failed");
  return result.data;
}

export async function manifest(ctx: Context, project: string): Promise<Manifest> {
  const data = await whenBuilt(ctx, project, () =>
    client(ctx).GET("/api/projects/{key}/manifest", { params: { path: { key: project } } }),
  );
  return data.manifest;
}

export async function captures(ctx: Context, project: string): Promise<Capture[]> {
  const { data, error } = await client(ctx).GET("/api/projects/{key}/captures", {
    params: { path: { key: project } },
  });
  if (!data) fail(error, "request failed");
  return data.captures;
}

export async function comments(ctx: Context, project: string): Promise<Comment[]> {
  const { data, error } = await client(ctx).GET("/api/projects/{key}/comments", {
    params: { path: { key: project }, query: { status: "all" } },
  });
  if (!data) fail(error, "request failed");
  return data.comments;
}

export async function system(ctx: Context): Promise<Schemas["SystemInfo"]> {
  const { data, error } = await client(ctx).GET("/api/system");
  if (!data) fail(error, "request failed");
  return data;
}

/** What an id names: a screen, story, page, flow, component (story group) or comment (`c12`). */
export type Found =
  | { type: "screen"; screen: Screen }
  | { type: "story"; story: Story }
  | { type: "page"; page: Manifest["pages"][number] }
  | { type: "flow"; flow: Flow }
  | { type: "component"; component: string; stories: Story[] }
  | { type: "comment"; id: number };

export function find(m: Manifest, id: string): Found | undefined {
  const comment = /^c(\d+)$/.exec(id);
  if (comment) return { type: "comment", id: Number(comment[1]) };
  const screen = m.screens.find((s) => s.id === id);
  if (screen) return { type: "screen", screen };
  const story = m.stories.find((s) => s.id === id);
  if (story) return { type: "story", story };
  const page = m.pages.find((p) => p.id === id);
  if (page) return { type: "page", page };
  const flow = m.flows.find((f) => f.id === id);
  if (flow) return { type: "flow", flow };
  const stories = m.stories.filter((s) => s.component === id);
  if (stories.length > 0) return { type: "component", component: id, stories };
  return undefined;
}

export function mustFind(ctx: Context, m: Manifest, id: string): Found {
  const found = find(m, id);
  if (!found) throw new CliError(t("cli.error.unknownId", { id }, ctx.locale));
  return found;
}

/** A screen or story id as a render target. */
export function targetOf(ctx: Context, m: Manifest, id: string): { kind: Kind; target: string } {
  if (m.screens.some((s) => s.id === id)) return { kind: "screen", target: id };
  if (m.stories.some((s) => s.id === id)) return { kind: "story", target: id };
  throw new CliError(t("cli.error.notTarget", { id }, ctx.locale));
}

export function themesOf(ctx: Context, value: string | undefined): Theme[] {
  switch (value ?? "light") {
    case "light":
      return ["light"];
    case "dark":
      return ["dark"];
    case "both":
      return ["light", "dark"];
    default:
      throw new CliError(t("cli.error.theme", { theme: value ?? "" }, ctx.locale));
  }
}

/** Lines aligned in columns, two spaces apart; the last column is not padded. */
export function table(rows: string[][]): string[] {
  const widths: number[] = [];
  for (const row of rows) {
    row.forEach((cell, i) => {
      widths[i] = Math.max(widths[i] ?? 0, cell.length);
    });
  }
  return rows.map((row) =>
    row
      .map((cell, i) => (i === row.length - 1 ? cell : cell.padEnd(widths[i] ?? 0)))
      .join("  ")
      .trimEnd(),
  );
}

export function plural(count: number, one: string, many = `${one}s`): string {
  return `${count} ${count === 1 ? one : many}`;
}
