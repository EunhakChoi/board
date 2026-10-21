import assert from "node:assert/strict";
import { test } from "node:test";
import { blocks, inline } from "./markdown.ts";

test("an agent's answer becomes headings, lists and paragraphs", () => {
  const parsed = blocks(
    "Saved on #4.\n\n**What changed**\n- the `Open` button\n- its **flow** name\n\n1. one\n2. two\nend",
  );
  assert.deepEqual(
    parsed.map((b) => b.kind),
    ["p", "h", "ul", "ol", "p"],
  );
  assert.deepEqual(inline("the `Open` button"), [
    { kind: "text", text: "the " },
    { kind: "code", text: "Open" },
    { kind: "text", text: " button" },
  ]);
});

test("markup in a reply stays text", () => {
  assert.deepEqual(blocks("<img src=x onerror=alert(1)>"), [
    { kind: "p", lines: [[{ kind: "text", text: "<img src=x onerror=alert(1)>" }]] },
  ]);
});

test("a fenced block keeps its lines", () => {
  assert.deepEqual(blocks("```\na\n  b\n```"), [{ kind: "pre", text: "a\n  b" }]);
});
