// Signing the CLI in to a team server: `board login` (approve in the browser, or a personal
// token), `board logout`, `board whoami`. Tokens are kept per server in credentials.json (0600).

import { chmodSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { hostname } from "node:os";
import { dirname, join } from "node:path";
import { createClient } from "@board/api-client";
import { t } from "@board/i18n";
import { CliError, type Context, client } from "./commands.ts";
import { configDir } from "./system.ts";

type Credentials = Record<string, { token: string; email?: string }>;

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

/** The saved token for `server`, if `board login` stored one. */
export function savedToken(env: NodeJS.ProcessEnv, server: string): string | undefined {
  return readCredentials(env)[server]?.token;
}

/** A client that sends `token` (instead of the context's). */
function withToken(ctx: Context, token: string) {
  return createClient({
    baseUrl: ctx.server,
    token,
    ...(ctx.fetch ? { fetch: ctx.fetch } : {}),
  });
}

const sleep = (ms: number) => new Promise((done) => setTimeout(done, ms));

/** Starts a device login and waits until someone approves it in the browser. */
async function deviceLogin(ctx: Context, name: string | undefined): Promise<string> {
  const api = client(ctx);
  const started = await api.POST("/api/auth/device", { body: { name: name ?? hostname() } });
  if (!started.data) throw new CliError(`login failed (${started.response.status})`);
  const { deviceCode, userCode, verificationUrl, verificationUrlComplete, interval } = started.data;
  ctx.err(t("cli.login.open", { url: verificationUrl, code: userCode }, ctx.locale));
  // A person at the terminal gets the page opened for them.
  if (ctx.ask)
    ctx.sh?.(process.platform === "darwin" ? "open" : "xdg-open", [verificationUrlComplete]);
  const wait = ctx.pollMs ?? interval * 1000;
  for (;;) {
    await sleep(wait);
    const polled = await api.POST("/api/auth/device/token", { body: { deviceCode } });
    if (polled.data) return polled.data.token;
    const code = polled.error?.code;
    if (code === "authorization_pending") continue;
    throw new CliError(
      t(code === "access_denied" ? "cli.login.denied" : "cli.login.expired", {}, ctx.locale),
    );
  }
}

export async function login(ctx: Context, token: string | undefined, name: string | undefined) {
  const system = await client(ctx).GET("/api/system");
  if (!system.data) {
    throw new CliError(t("cli.error.unreachable", { server: ctx.server }, ctx.locale));
  }
  if (system.data.localMode) {
    ctx.out(t("cli.login.local", { server: ctx.server }, ctx.locale));
    return;
  }
  const issued = token ?? (await deviceLogin(ctx, name));
  const me = await withToken(ctx, issued).GET("/api/auth/me");
  if (!me.data) throw new CliError(t("cli.login.badToken", {}, ctx.locale));
  const credentials = readCredentials(ctx.env);
  credentials[ctx.server] = {
    token: issued,
    ...(me.data.email ? { email: me.data.email } : {}),
  };
  writeCredentials(ctx.env, credentials);
  if (ctx.json) {
    ctx.out(JSON.stringify({ server: ctx.server, ...me.data }, null, 2));
    return;
  }
  const params = { name: me.data.name, email: me.data.email ?? "", server: ctx.server };
  ctx.out(t("cli.login.done", params, ctx.locale));
}

export async function logout(ctx: Context) {
  const credentials = readCredentials(ctx.env);
  const saved = credentials[ctx.server];
  if (saved) {
    // Revoke it on the server too; a server that is gone just loses the local copy.
    await withToken(ctx, saved.token)
      .POST("/api/auth/logout")
      .catch(() => undefined);
    delete credentials[ctx.server];
    writeCredentials(ctx.env, credentials);
  }
  ctx.out(t(saved ? "cli.logout.done" : "cli.logout.none", { server: ctx.server }, ctx.locale));
}

export async function whoami(ctx: Context) {
  const { data, error } = await client(ctx).GET("/api/auth/me");
  if (!data) {
    if (error?.code === "unauthorized" || error?.code === "invalid_token")
      throw new CliError(t("cli.whoami.out", { server: ctx.server }, ctx.locale));
    throw new CliError(error ? `${error.message} (${error.code})` : "request failed");
  }
  if (ctx.json) {
    ctx.out(JSON.stringify({ server: ctx.server, ...data }, null, 2));
    return;
  }
  if (data.kind === "local") {
    ctx.out(t("cli.whoami.local", { server: ctx.server }, ctx.locale));
    return;
  }
  const params = { name: data.name, email: data.email ?? "", role: data.role, server: ctx.server };
  ctx.out(t("cli.whoami.user", params, ctx.locale));
}
