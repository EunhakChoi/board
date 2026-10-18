#!/usr/bin/env node
import { detectLocale, type Locale, t } from "@board/i18n";
import pkg from "../package.json" with { type: "json" };
import { findCheckout } from "./checkout.ts";
import * as commands from "./commands.ts";
import * as notes from "./comments.ts";
import { type Theme, themesOf } from "./data.ts";
import type { RenderOptions } from "./render.ts";
import * as renders from "./render.ts";
import * as structure from "./structure.ts";
import * as system from "./system.ts";

export interface Args {
  positional: string[];
  flags: Map<string, string | true>;
  /** Repeatable flags, e.g. `--include a --include b`. */
  lists: Map<string, string[]>;
}

const VALUE_FLAGS = new Set([
  "-p",
  "--project",
  "--key",
  "--title",
  "--include",
  "-m",
  "--message",
  "--rev",
  "--server",
  "--at",
  "--theme",
  "--out",
  "--from",
  "--to",
  "--page",
  "--format",
  "--template",
  "--context",
  "--sheet",
  "--on",
]);
const LIST_FLAGS = new Set(["--include"]);

export function parse(argv: string[]): Args {
  const args: Args = { positional: [], flags: new Map(), lists: new Map() };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i] as string;
    if (!arg.startsWith("-") || arg === "-") {
      args.positional.push(arg);
      continue;
    }
    const eq = arg.indexOf("=");
    const name = eq > 0 ? arg.slice(0, eq) : arg;
    const inline = eq > 0 ? arg.slice(eq + 1) : undefined;
    if (VALUE_FLAGS.has(name)) {
      const value = inline ?? argv[++i];
      if (value === undefined) throw new commands.CliError(`${name} needs a value`);
      if (LIST_FLAGS.has(name)) args.lists.set(name, [...(args.lists.get(name) ?? []), value]);
      else args.flags.set(name, value);
    } else {
      args.flags.set(name, true);
    }
  }
  return args;
}

function str(args: Args, ...names: string[]): string | undefined {
  for (const name of names) {
    const value = args.flags.get(name);
    if (typeof value === "string") return value;
  }
  return undefined;
}

export interface RunOptions {
  locale?: Locale;
  cwd?: string;
  env?: NodeJS.ProcessEnv;
  out?: (line: string) => void;
  err?: (line: string) => void;
  fetch?: typeof globalThis.fetch;
  ask?: (question: string) => Promise<string>;
  sh?: (command: string, args: string[]) => string | undefined;
  pollMs?: number;
}

/** Runs one CLI invocation. Returns the exit code. */
export async function run(argv: string[], options: RunOptions = {}): Promise<number> {
  const env = options.env ?? process.env;
  const locale = options.locale ?? detectLocale(env);
  const out = options.out ?? ((line: string) => console.log(line));
  const err = options.err ?? ((line: string) => console.error(line));
  try {
    const args = parse(argv);
    if (args.flags.has("--version") || args.flags.has("-v")) {
      out(t("cli.version", { version: pkg.version }, locale));
      return 0;
    }
    const token = env.BOARD_TOKEN;
    const cwd = options.cwd ?? process.cwd();
    // A checkout belongs to one server, which wins over the general BOARD_URL.
    const server =
      str(args, "--server") ??
      findCheckout(cwd)?.state.server ??
      env.BOARD_URL ??
      "http://127.0.0.1:5280";
    const ctx: commands.Context = {
      server: server.replace(/\/$/, ""),
      ...(token ? { token } : {}),
      cwd,
      locale,
      json: args.flags.has("--json"),
      out,
      err,
      env,
      ...(options.fetch ? { fetch: options.fetch } : {}),
      ...(options.ask && !args.flags.has("--yes") ? { ask: options.ask } : {}),
      ...(options.sh ? { sh: options.sh } : {}),
      ...(options.pollMs !== undefined ? { pollMs: options.pollMs } : {}),
    };
    const [command, ...rest] = args.positional;
    const project = () => commands.projectOf(ctx, str(args, "-p", "--project"));
    const key = str(args, "--key");
    const title = str(args, "--title");
    const message = str(args, "-m", "--message");
    const theme = str(args, "--theme");
    const render = (): RenderOptions => {
      const rev = str(args, "--rev");
      const out = str(args, "--out");
      return {
        targets: rest,
        themes: themesOf(ctx, theme),
        ...(rev ? { rev } : {}),
        ...(out ? { out } : {}),
        force: args.flags.has("--force"),
      };
    };
    const single = (): Theme => {
      const themes = themesOf(ctx, theme);
      if (themes.length !== 1) throw new commands.CliError(t("cli.error.oneTheme", {}, locale));
      return themes[0] as Theme;
    };
    switch (command) {
      case "setup":
        return await system.setup(ctx, args.flags.has("--yes"));
      case "doctor":
        return await system.doctor(ctx);
      case "serve":
        return await system.serve(ctx, {
          public: args.flags.has("--public"),
          serverOnly: args.flags.has("--server-only"),
        });
      case "dev":
        return await system.dev(ctx);
      case "status":
        return await system.status(ctx);
      case "init":
        return await system.init(
          ctx,
          rest[0],
          rest[1],
          str(args, "--template") ?? "recommended",
          title,
        );
      case "skill":
        return system.skill(ctx, rest[0], args.flags.has("--claude"), args.flags.has("--codex"));
      case "ls":
        await structure.ls(ctx, project(), args.flags.has("--stale"));
        break;
      case "show":
        if (!rest[0]) throw new commands.CliError(t("cli.usage.show", {}, locale));
        await structure.show(ctx, project(), rest[0], str(args, "--out"));
        break;
      case "url":
        await structure.url(ctx, project(), rest[0]);
        break;
      case "connections": {
        const filter: structure.ConnectionFilter = {};
        for (const name of ["from", "to", "page"] as const) {
          const value = str(args, `--${name}`);
          if (value) filter[name] = value;
        }
        await structure.connections(ctx, project(), filter);
        break;
      }
      case "path":
        if (!rest[0] || !rest[1]) throw new commands.CliError(t("cli.usage.path", {}, locale));
        return await structure.path(ctx, project(), rest[0], rest[1], str(args, "--sheet"));
      case "map":
        await structure.map(ctx, project(), str(args, "--format") ?? "text");
        break;
      case "flows":
        await structure.flows(ctx, project());
        break;
      case "flow":
        if (!rest[0]) throw new commands.CliError(t("cli.usage.flow", {}, locale));
        await structure.flow(ctx, project(), rest[0], str(args, "--sheet"), single());
        break;
      case "sheet":
        await structure.sheet(ctx, project(), rest, single(), str(args, "--out"));
        break;
      case "source":
        if (!rest[0]) throw new commands.CliError(t("cli.usage.source", {}, locale));
        await structure.source(ctx, project(), rest[0], Number(str(args, "--context") ?? "20"));
        break;
      case "check":
        return await renders.check(ctx, project(), render());
      case "capture":
        return await renders.capture(ctx, project(), render());
      case "comments":
        return await commentsCommand(ctx, rest, args, project, message, single);
      case "projects":
        await commands.projects(ctx);
        break;
      case "import":
        await commands.importProject(ctx, {
          path: rest[0] ?? ".",
          ...(key ? { key } : {}),
          ...(title ? { title } : {}),
          include: args.lists.get("--include") ?? [],
          dryRun: args.flags.has("--dry-run"),
        });
        break;
      case "checkout":
        if (!rest[0]) throw new commands.CliError(t("cli.usage.checkout", {}, locale));
        await commands.checkout(ctx, rest[0], rest[1], str(args, "--rev"));
        break;
      case "push":
        await commands.push(ctx, str(args, "-m", "--message"));
        break;
      case "pull":
        await commands.pull(ctx);
        break;
      case "versions":
        await commands.versions(ctx, project());
        break;
      case "diff":
        if (!rest[0]) throw new commands.CliError(t("cli.usage.diff", {}, locale));
        await commands.diff(ctx, project(), rest[0], rest[1]);
        break;
      default:
        out(t("cli.usage", {}, locale));
        return command === undefined || args.flags.has("--help") ? 0 : 1;
    }
    return 0;
  } catch (error) {
    if (error instanceof commands.CliError) {
      err(`board: ${error.message}`);
      return 1;
    }
    if (error instanceof TypeError && /fetch failed/.test(error.message)) {
      err(`board: ${t("cli.error.unreachable", {}, locale)}`);
      return 1;
    }
    throw error;
  }
}

async function commentsCommand(
  ctx: commands.Context,
  rest: string[],
  args: Args,
  project: () => string,
  message: string | undefined,
  theme: () => Theme,
): Promise<number> {
  const [sub, ...more] = rest;
  switch (sub) {
    case undefined: {
      const status = args.flags.has("--all")
        ? "all"
        : args.flags.has("--resolved")
          ? "resolved"
          : "open";
      const on = str(args, "--on");
      return notes.list(ctx, project(), { status, ...(on ? { on } : {}) });
    }
    case "show":
      await notes.showComment(
        ctx,
        project(),
        notes.parseCommentId(ctx, more[0]),
        str(args, "--out"),
      );
      return 0;
    case "add":
      await notes.add(ctx, project(), more[0], str(args, "--at"), message, theme());
      return 0;
    case "reply":
    case "resolve":
    case "reopen":
      await notes.answer(ctx, project(), sub, notes.parseCommentId(ctx, more[0]), message);
      return 0;
    default:
      throw new commands.CliError(t("cli.usage.comments", {}, ctx.locale));
  }
}

/** Reads one line from the terminal. */
function askTerminal(question: string): Promise<string> {
  return new Promise((done) => {
    process.stdout.write(question);
    process.stdin.setEncoding("utf8");
    process.stdin.once("data", (chunk) => {
      process.stdin.pause();
      done(String(chunk).split("\n")[0] ?? "");
    });
  });
}

if (import.meta.main) {
  const interactive = process.stdin.isTTY && process.stdout.isTTY && !process.env.CI;
  process.exitCode = await run(process.argv.slice(2), interactive ? { ask: askTerminal } : {});
  if (process.stderr.isTTY && !process.argv.includes("--json")) {
    const { updateNotice } = await import("./system.ts");
    await updateNotice({
      server: (process.env.BOARD_URL ?? "http://127.0.0.1:5280").replace(/\/$/, ""),
      cwd: process.cwd(),
      locale: detectLocale(process.env),
      json: false,
      out: console.log,
      err: console.error,
      env: process.env,
    });
  }
}
