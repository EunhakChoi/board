// Marks every host JSX element (`<div>`, `<button>`, …) with `data-src="path:line:col"` so the charet
// can map a clicked element back to the exact place in the source, and points absolute paths to
// public files (`"/logo.png"`) at the build's base. Only for preview builds; handoff exports come
// from the project store, never from this output.

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

export interface SourceOptions {
  /** Add `data-src` to host elements (JSX files only; default true). */
  locations?: boolean;
  /**
   * True for an absolute URL that names a public file (`/logo.png`). Such string literals become
   * `import.meta.env.BASE_URL + "logo.png"`, because previews are served under a path prefix and
   * vite only rewrites these in HTML and CSS.
   */
  isPublic?: (url: string) => boolean;
}

/**
 * Adds `data-src` to host elements in `code` (a module at `path`, relative to the project root) and
 * rebases public file paths. Returns undefined when nothing changed. Components (`<Button>`,
 * `<ui.Card>`) are left alone: they may not forward unknown props, and the host elements they
 * render get marked instead.
 */
export function addSourceLocations(
  code: string,
  path: string,
  options: SourceOptions = {},
): { code: string; map: ReturnType<MagicString["generateMap"]> } | undefined {
  const locations = (options.locations ?? true) && code.includes("<");
  const { isPublic } = options;
  if (!locations && !(isPublic && code.includes('"/'))) return undefined;
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
  walk(ast, undefined, (node, parent) => {
    if (isPublic && node.type === "StringLiteral" && node.start != null && node.end != null) {
      const value = node.value as string;
      const skip =
        parent?.type === "ImportDeclaration" ||
        parent?.type === "ExportNamedDeclaration" ||
        parent?.type === "ExportAllDeclaration" ||
        parent?.type === "TSLiteralType" ||
        parent?.type === "TSExternalModuleReference" ||
        (parent?.type === "ObjectProperty" && parent.key === node);
      if (!skip && isPublic(value)) {
        // Preview builds are relative (served under a per-viewer path); the frame records where
        // the build's root is. Elsewhere the configured base applies.
        const rebased = `(globalThis.__CHARET_ROOT__ ?? import.meta.env.BASE_URL) + ${JSON.stringify(value.slice(1))}`;
        out.overwrite(
          node.start,
          node.end,
          parent?.type === "JSXAttribute" ? `{${rebased}}` : `(${rebased})`,
        );
        changed = true;
      }
      return;
    }
    if (!locations || node.type !== "JSXOpeningElement") return;
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

function walk(
  node: Node,
  parent: Node | undefined,
  visit: (node: Node, parent: Node | undefined) => void,
): void {
  visit(node, parent);
  for (const key of Object.keys(node)) {
    if (key === "loc" || key === "leadingComments" || key === "trailingComments") continue;
    const value = node[key];
    if (Array.isArray(value)) {
      for (const child of value) if (isNode(child)) walk(child, node, visit);
    } else if (isNode(value)) {
      walk(value, node, visit);
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
