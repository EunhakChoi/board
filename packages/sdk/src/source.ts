// Marks every host JSX element (`<div>`, `<button>`, …) with `data-src="path:line:col"` so the board
// can map a clicked element back to the exact place in the source. Only for preview builds; handoff
// exports come from the project store, never from this output.

import { parse } from "@babel/parser";
import MagicString from "magic-string";

export const SOURCE_ATTRIBUTE = "data-src";

interface Node {
  type: string;
  start?: number | null;
  end?: number | null;
  loc?: { start: { line: number; column: number } } | null;
  [key: string]: unknown;
}

/**
 * Adds `data-src` to host elements in `code` (a .jsx/.tsx module at `path`, relative to the project
 * root). Returns undefined when nothing changed. Components (`<Button>`, `<ui.Card>`) are left
 * alone: they may not forward unknown props, and the host elements they render get marked instead.
 */
export function addSourceLocations(
  code: string,
  path: string,
): { code: string; map: ReturnType<MagicString["generateMap"]> } | undefined {
  if (!code.includes("<")) return undefined;
  let ast: Node;
  try {
    ast = parse(code, {
      sourceType: "module",
      plugins: ["jsx", "typescript"],
      errorRecovery: true,
    }) as unknown as Node;
  } catch {
    return undefined; // Leave unparsable code to the real compiler, which reports it properly.
  }
  const out = new MagicString(code);
  let changed = false;
  walk(ast, (node) => {
    if (node.type !== "JSXOpeningElement") return;
    const name = node.name as Node;
    if (name.type !== "JSXIdentifier" || !/^[a-z]/.test(name.name as string)) return;
    const attributes = node.attributes as Node[];
    const already = attributes.some(
      (a) => a.type === "JSXAttribute" && (a.name as Node).name === SOURCE_ATTRIBUTE,
    );
    if (already || name.end == null || !node.loc) return;
    const { line, column } = node.loc.start;
    out.appendLeft(
      name.end,
      ` ${SOURCE_ATTRIBUTE}="${escapeAttribute(path)}:${line}:${column + 1}"`,
    );
    changed = true;
  });
  if (!changed) return undefined;
  return { code: out.toString(), map: out.generateMap({ hires: true, source: path }) };
}

function walk(node: Node, visit: (node: Node) => void): void {
  visit(node);
  for (const key of Object.keys(node)) {
    if (key === "loc" || key === "leadingComments" || key === "trailingComments") continue;
    const value = node[key];
    if (Array.isArray(value)) {
      for (const child of value) if (isNode(child)) walk(child, visit);
    } else if (isNode(value)) {
      walk(value, visit);
    }
  }
}

function isNode(value: unknown): value is Node {
  return typeof value === "object" && value !== null && typeof (value as Node).type === "string";
}

function escapeAttribute(text: string): string {
  return text.replace(/&/g, "&amp;").replace(/"/g, "&quot;");
}

/** Parses a `data-src` value back into its parts. */
export function parseSourceLocation(
  value: string,
): { path: string; line: number; column: number } | undefined {
  const match = /^(.*):(\d+):(\d+)$/.exec(value);
  if (!match) return undefined;
  return { path: match[1] as string, line: Number(match[2]), column: Number(match[3]) };
}
