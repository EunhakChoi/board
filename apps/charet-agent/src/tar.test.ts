import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { tar } from "./tar.ts";

test("writes an archive tar reads back", () => {
  const dir = mkdtempSync(join(tmpdir(), "charet-tar-"));
  try {
    const archive = tar([
      { path: "input/spec.json", data: Buffer.from('{"a":1}') },
      { path: "input/image-0.png", data: Buffer.alloc(1300, 7) },
    ]);
    assert.equal(archive.length % 512, 0);
    writeFileSync(join(dir, "a.tar"), archive);
    execFileSync("tar", ["-xf", "a.tar"], { cwd: dir });
    assert.equal(readFileSync(join(dir, "input/spec.json"), "utf8"), '{"a":1}');
    assert.equal(readFileSync(join(dir, "input/image-0.png")).length, 1300);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
