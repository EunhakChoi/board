import assert from "node:assert/strict";
import { test } from "node:test";
import { spacingOf, withSpacing } from "./spacing.ts";

test("reads plain spacing classes, longest family first", () => {
  const found = spacingOf("mx-auto max-w-xl p-8 gap-x-2 md:p-4 -mt-1 space-y-1.5 px-[10px]");
  assert.deepEqual(
    [...found],
    [
      ["mx", "auto"],
      ["p", "8"],
      ["gap-x", "2"],
      ["space-y", "1.5"],
      ["px", "[10px]"],
    ],
  );
});

test("changes one family in place, adds it, or removes it", () => {
  assert.equal(withSpacing("mx-auto p-8 text-sm", "p", "10"), "mx-auto p-10 text-sm");
  assert.equal(withSpacing("mx-auto", "gap", "4"), "mx-auto gap-4");
  assert.equal(withSpacing("p-2 mb-6 p-4", "p", undefined), "mb-6");
  assert.equal(withSpacing("md:p-4 p-2", "p", "3"), "md:p-4 p-3", "variants stay");
});
