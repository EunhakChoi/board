// Signing the CLI in to a shared charet: `charet login` with the token an admin handed over,
// `charet logout`, `charet whoami`. Tokens are kept per server in credentials.json (0600).

import { chmodSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { createClient } from "@charet/api-client";
import { t } from "@charet/i18n";
import { CliError, type Context, client } from "./commands.ts";
import { configDir } from "./system.ts";

type Credentials = Record<string, { token: string; name?: string }>;

function credentialsFile(env: NodeJS.ProcessEnv): string {
  return join(configDir(env), "credentials.json");
}

function readCredentials(env: NodeJS.ProcessEnv): Credentials {
  try {
    return JSON.parse(readFileSync(credentialsFile(env), "utf8")) as Credentials;
  } catch {
    return {};
  }
}

function writeCredentials(env: NodeJS.ProcessEnv, credentials: Credentials): void {
  const file = credentialsFile(env);
  mkdirSync(dirname(file), { recursive: true });
  writeFileSync(file, `${JSON.stringify(credentials, null, 2)}\n`, { mode: 0o600 });
  chmodSync(file, 0o600);
}

/** The saved token for `server`, if `charet login` stored one. */
export function savedToken(env: NodeJS.ProcessEnv, server: string): string | undefined {
  return readCredentials(env)[server]?.token;
}

/** Keeps `token` as the CLI's way into `server`. */
export function rememberToken(
  env: NodeJS.ProcessEnv,
  server: string,
  token: string,
  name?: string,
): void {
  const credentials = readCredentials(env);
  credentials[server] = { token, ...(name ? { name } : {}) };
  writeCredentials(env, credentials);
}

/** A client that sends `token` (instead of the context's). */
export function withToken(ctx: Context, token: string) {
  return createClient({
    baseUrl: ctx.server,
    token,
    ...(ctx.fetch ? { fetch: ctx.fetch } : {}),
  });
}

export async function login(ctx: Context, token: string | undefined) {
  const system = await client(ctx).GET("/api/system");
  if (!system.data) {
    throw new CliError(t("cli.error.unreachable", { server: ctx.server }, ctx.locale));
  }
  const given =
    token?.trim() || (ctx.ask ? (await ctx.ask(t("cli.login.prompt", {}, ctx.locale))).trim() : "");
  if (!given) throw new CliError(t("cli.login.needToken", {}, ctx.locale));
  const me = await withToken(ctx, given).GET("/api/auth/me");
  if (me.data?.kind !== "user") throw new CliError(t("cli.login.badToken", {}, ctx.locale));
  rememberToken(ctx.env, ctx.server, given, me.data.name);
  if (ctx.json) {
    ctx.out(JSON.stringify({ server: ctx.server, ...me.data }, null, 2));
    return;
  }
  const params = { name: me.data.name, role: me.data.role, server: ctx.server };
  ctx.out(t("cli.login.done", params, ctx.locale));
}

export async function logout(ctx: Context) {
  const credentials = readCredentials(ctx.env);
  const saved = credentials[ctx.server];
  if (saved) {
    // The token itself stays valid (it is the member's); an admin reissues it to stop it.
    delete credentials[ctx.server];
    writeCredentials(ctx.env, credentials);
  }
  ctx.out(t(saved ? "cli.logout.done" : "cli.logout.none", { server: ctx.server }, ctx.locale));
}

export async function whoami(ctx: Context) {
  const { data, error } = await client(ctx).GET("/api/auth/me");
  if (data?.kind === "visitor") {
    throw new CliError(t("cli.whoami.out", { server: ctx.server }, ctx.locale));
  }
  if (!data) {
    if (error?.code === "unauthorized" || error?.code === "invalid_token")
      throw new CliError(t("cli.whoami.out", { server: ctx.server }, ctx.locale));
    throw new CliError(error ? `${error.message} (${error.code})` : "request failed");
  }
  if (ctx.json) {
    ctx.out(JSON.stringify({ server: ctx.server, ...data }, null, 2));
    return;
  }
  const params = { name: data.name, role: data.role, server: ctx.server };
  ctx.out(t("cli.whoami.user", params, ctx.locale));
}
