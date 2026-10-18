#!/usr/bin/env node
import { detectLocale, type Locale, t } from "@board/i18n";
import pkg from "../package.json" with { type: "json" };
import * as commands from "./commands.ts";

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
    const ctx: commands.Context = {
      server: (str(args, "--server") ?? env.BOARD_URL ?? "http://127.0.0.1:5280").replace(
        /\/$/,
        "",
      ),
      ...(token ? { token } : {}),
      cwd: options.cwd ?? process.cwd(),
      locale,
      json: args.flags.has("--json"),
      out,
      ...(options.fetch ? { fetch: options.fetch } : {}),
    };
    const [command, ...rest] = args.positional;
    const project = () => commands.projectOf(ctx, str(args, "-p", "--project"));
    const key = str(args, "--key");
    const title = str(args, "--title");
    switch (command) {
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

if (import.meta.main) process.exitCode = await run(process.argv.slice(2));
