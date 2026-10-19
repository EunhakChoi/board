// `board inbox`: what happened that concerns you (new comments, answers in your threads, access
// requests for admins). `board inbox read [id...]` marks them read.

import { t } from "@board/i18n";
import { CliError, type Context, client } from "./commands.ts";

function failed(error: { code: string; message: string } | undefined): never {
  if (error?.code === "unauthorized" || error?.code === "invalid_token")
    throw new CliError(error.message);
  throw new CliError(error ? `${error.message} (${error.code})` : "request failed");
}

export async function list(ctx: Context, all: boolean): Promise<void> {
  const { data, error } = await client(ctx).GET("/api/me/notifications", {
    params: { query: { unread: !all, limit: 50 } },
  });
  if (!data) failed(error);
  if (ctx.json) {
    ctx.out(JSON.stringify(data, null, 2));
    return;
  }
  if (data.notifications.length === 0) {
    ctx.out(t(all ? "cli.inbox.none" : "cli.inbox.noneUnread", {}, ctx.locale));
    return;
  }
  for (const n of data.notifications) {
    const mark = n.readAt ? " " : "•";
    const when = n.createdAt.slice(0, 16).replace("T", " ");
    ctx.out(`${mark} #${n.id}  ${when}  ${n.text}`);
    ctx.out(`    ${ctx.server.replace(/\/$/, "")}${n.link}`);
  }
  if (data.unread > 0) ctx.out(t("cli.inbox.unread", { count: String(data.unread) }, ctx.locale));
}

export async function read(ctx: Context, ids: string[]): Promise<void> {
  const numbers = ids.map((id) => Number(id.replace(/^#/, "")));
  if (numbers.some((n) => !Number.isInteger(n) || n < 1))
    throw new CliError(t("cli.usage.inbox", {}, ctx.locale));
  const { data, error } = await client(ctx).POST("/api/me/notifications/read", {
    body: numbers.length > 0 ? { ids: numbers } : {},
  });
  if (!data) failed(error);
  if (ctx.json) {
    ctx.out(JSON.stringify(data));
    return;
  }
  ctx.out(t("cli.inbox.marked", { count: String(data.unread) }, ctx.locale));
}
