import en from "../locales/en.json" with { type: "json" };
import ko from "../locales/ko.json" with { type: "json" };

export type Key = keyof typeof en;
export type Locale = "en" | "ko";

const catalogs: Record<Locale, Partial<Record<Key, string>>> = { en, ko };

/** Picks `ko` for Korean system locales, otherwise English (the default). */
export function detectLocale(env: NodeJS.ProcessEnv = process.env): Locale {
  const tag = env.BOARD_LANG ?? env.LC_ALL ?? env.LC_MESSAGES ?? env.LANG ?? "";
  return tag.toLowerCase().startsWith("ko") ? "ko" : "en";
}

/** Looks up `key` in `locale`, falling back to English, and fills `{name}` placeholders. */
export function t(key: Key, params: Record<string, string> = {}, locale: Locale = "en"): string {
  const text = catalogs[locale][key] ?? en[key];
  return text.replace(/\{(\w+)\}/g, (match, name: string) => params[name] ?? match);
}
