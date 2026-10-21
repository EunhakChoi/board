// Tailwind spacing in a class list, for the Design panel: which spacing classes it has and the
// class list with one of them changed. Classes with a variant (`md:p-4`, `hover:…`) stay as they
// are; only the plain ones are edited.

export const FAMILIES = [
  "p",
  "px",
  "py",
  "pt",
  "pr",
  "pb",
  "pl",
  "m",
  "mx",
  "my",
  "mt",
  "mr",
  "mb",
  "ml",
  "gap",
  "gap-x",
  "gap-y",
  "space-x",
  "space-y",
] as const;
export type Family = (typeof FAMILIES)[number];

/** Padding families: a component with a `size` variant sets these through it instead. */
export const PADDING: readonly Family[] = ["p", "px", "py", "pt", "pr", "pb", "pl"];

export const SCALE = [
  "0",
  "0.5",
  "1",
  "1.5",
  "2",
  "2.5",
  "3",
  "4",
  "5",
  "6",
  "8",
  "10",
  "12",
  "16",
  "20",
  "24",
] as const;

// Longest names first, so `gap-x-2` is not read as `gap` with `x-2`.
const BY_LENGTH = [...FAMILIES].sort((a, b) => b.length - a.length);

function parse(token: string): { family: Family; value: string } | undefined {
  if (token.includes(":") || token.startsWith("-")) return undefined;
  for (const family of BY_LENGTH) {
    if (token.startsWith(`${family}-`)) {
      const value = token.slice(family.length + 1);
      if (/^(\d+(\.\d+)?|px|auto|\[[^\]]+\])$/.test(value)) return { family, value };
    }
  }
  return undefined;
}

/** The spacing a class list sets, family by family (the last one wins, as in CSS order). */
export function spacingOf(classes: string): Map<Family, string> {
  const out = new Map<Family, string>();
  for (const token of classes.split(/\s+/)) {
    const found = parse(token);
    if (found) out.set(found.family, found.value);
  }
  return out;
}

/** `classes` with `family` set to `value` (in place of the old one), or removed when undefined. */
export function withSpacing(classes: string, family: Family, value: string | undefined): string {
  const tokens = classes.split(/\s+/).filter(Boolean);
  const next = `${family}-${value}`;
  let placed = false;
  const out: string[] = [];
  for (const token of tokens) {
    if (parse(token)?.family !== family) {
      out.push(token);
    } else if (value !== undefined && !placed) {
      out.push(next);
      placed = true;
    }
  }
  if (value !== undefined && !placed) out.push(next);
  return out.join(" ");
}
