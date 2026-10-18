import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { readConfig } from "./config.ts";

test("defaults to loopback under BOARD_HOME", () => {
  const config = readConfig({ HOME: "/home/a" });
  assert.equal(config.home, "/home/a/.board/workspace");
  assert.equal(config.host, "127.0.0.1");
  assert.equal(config.port, 5282);
  assert.equal(config.token, undefined);
  assert.equal(config.jobs, 2);
});

test("needs a token off loopback and checks values", () => {
  assert.throws(
    () => readConfig({ HOME: "/h", BOARD_WORKSPACE_BIND: "0.0.0.0:5282" }),
    /BOARD_WORKSPACE_TOKEN/,
  );
  const config = readConfig({
    HOME: "/h",
    BOARD_WORKSPACE_BIND: "[::]:7000",
    BOARD_WORKSPACE_TOKEN: "s",
  });
  assert.equal(config.host, "::");
  assert.equal(config.port, 7000);
  assert.throws(
    () => readConfig({ HOME: "/h", BOARD_WORKSPACE_BIND: "localhost" }),
    /not an address/,
  );
  assert.throws(() => readConfig({ HOME: "/h", BOARD_WORKSPACE_JOBS: "0" }), /positive/);
});

test("reads the token from a file", () => {
  const dir = mkdtempSync(join(tmpdir(), "board-config-"));
  writeFileSync(join(dir, "token"), "secret\n");
  const config = readConfig({
    HOME: "/h",
    BOARD_WORKSPACE_BIND: "0.0.0.0:5282",
    BOARD_WORKSPACE_TOKEN_FILE: join(dir, "token"),
  });
  assert.equal(config.token, "secret");
  rmSync(dir, { recursive: true });
});
