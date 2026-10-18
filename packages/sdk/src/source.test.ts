import assert from "node:assert/strict";
import { test } from "node:test";
import { addSourceLocations, parseSourceLocation } from "./source.ts";

test("marks host elements with their line and column", () => {
  const code = [
    "export function Card({ title }: { title: string }) {",
    "  return (",
    '    <section className="card">',
    "      <h2>{title}</h2>",
    '      <Button variant="primary" />',
    "      <ui.Icon />",
    "    </section>",
    "  );",
    "}",
  ].join("\n");
  const result = addSourceLocations(code, "src/Card.tsx");
  assert.ok(result);
  assert.match(result.code, /<section data-src="src\/Card.tsx:3:5" className="card">/);
  assert.match(result.code, /<h2 data-src="src\/Card.tsx:4:7">/);
  // Components and member expressions stay untouched.
  assert.match(result.code, /<Button variant="primary" \/>/);
  assert.match(result.code, /<ui.Icon \/>/);
  assert.equal(result.map.sources[0], "src/Card.tsx");
});

test("keeps an existing data-src and skips files without JSX", () => {
  assert.equal(addSourceLocations('const a = <div data-src="x:1:1" />;', "a.tsx"), undefined);
  assert.equal(addSourceLocations("export const n = 1 < 2;", "a.ts"), undefined);
  assert.equal(addSourceLocations("const a = <Foo />;", "a.tsx"), undefined);
});

test("handles generics, fragments and quotes in paths", () => {
  const code = "const f = <T,>(x: T) => x;\nexport const A = () => <><span>hi</span></>;";
  const result = addSourceLocations(code, 'src/we"ird.tsx');
  assert.ok(result);
  assert.match(result.code, /<span data-src="src\/we&quot;ird.tsx:2:26">/);
});

test("parses a location back", () => {
  assert.deepEqual(parseSourceLocation("src/a:b.tsx:12:3"), {
    path: "src/a:b.tsx",
    line: 12,
    column: 3,
  });
  assert.equal(parseSourceLocation("nope"), undefined);
});
