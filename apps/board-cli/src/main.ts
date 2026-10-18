#!/usr/bin/env node
import { detectLocale, t } from "@board/i18n";
import pkg from "../package.json" with { type: "json" };

export function run(argv: string[], locale = detectLocale()): string {
  if (argv[0] === "--version" || argv[0] === "-v")
    return t("cli.version", { version: pkg.version }, locale);
  return t("cli.usage", {}, locale);
}

if (import.meta.main) console.log(run(process.argv.slice(2)));
