import assert from "node:assert/strict";
import { test } from "node:test";
import { diffLines, fold } from "./diff.ts";

test("diffs lines with numbers on both sides", () => {
  const lines = diffLines("a\nb\nc\nd", "a\nB\nc\nd\ne");
  assert.deepEqual(
    lines?.map((l) => `${l.kind[0]}${l.before ?? "-"}:${l.after ?? "-"} ${l.text}`),
    ["s1:1 a", "d2:- b", "a-:2 B", "s3:3 c", "s4:4 d", "a-:5 e"],
  );
  assert.deepEqual(diffLines("", "x"), [{ kind: "add", text: "x", after: 1 }]);
  assert.deepEqual(diffLines("x", ""), [{ kind: "del", text: "x", before: 1 }]);
});

test("folds unchanged runs away from changes", () => {
  const before = Array.from({ length: 20 }, (_, i) => `l${i}`).join("\n");
  const after = before.replace("l10", "L10");
  const rows = fold(diffLines(before, after) ?? [], 2);
  assert.deepEqual(
    rows.map((r) => (r.kind === "fold" ? `…${r.count}` : r.kind)),
    ["…8", "same", "same", "del", "add", "same", "same", "…7"],
  );
});
