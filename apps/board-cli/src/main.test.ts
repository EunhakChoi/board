import assert from "node:assert/strict";
import { test } from "node:test";
import { run } from "./main.ts";

test("prints the version and usage", () => {
  assert.equal(run(["--version"], "en"), "board 0.0.0");
  assert.match(run([], "en"), /^Usage: board/);
});
