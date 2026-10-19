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

test("points absolute public paths at the build's base", () => {
  const code = [
    'import logo from "/logo.png";',
    'type Path = "/logo.png";',
    'export const A = () => <img src="/logo.png" alt="" />;',
    'export const B = { icon: "/logo.png", "/logo.png": 1, other: "/api/x" };',
  ].join("\n");
  const result = addSourceLocations(code, "src/a.tsx", {
    locations: false,
    isPublic: (url) => url === "/logo.png",
  });
  assert.ok(result);
  assert.match(result.code, /^import logo from "\/logo.png";/);
  assert.match(result.code, /type Path = "\/logo.png";/);
  assert.match(
    result.code,
    /<img src=\{\(globalThis\.__BOARD_ROOT__ \?\? import\.meta\.env\.BASE_URL\) \+ "logo\.png"\} alt="" \/>/,
  );
  assert.match(
    result.code,
    /icon: \(\(globalThis\.__BOARD_ROOT__ \?\? import\.meta\.env\.BASE_URL\) \+ "logo\.png"\), "\/logo\.png": 1/,
  );
  assert.match(result.code, /other: "\/api\/x"/);
  assert.doesNotMatch(result.code, /data-src/);
  assert.equal(
    addSourceLocations('const a = "/x.png";', "a.ts", { isPublic: () => false }),
    undefined,
  );
});
