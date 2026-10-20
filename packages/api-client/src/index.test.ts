import assert from "node:assert/strict";
import { test } from "node:test";
import { createClient } from "./index.ts";

test("calls the contract path with the bearer token", async () => {
  let seen: Request | undefined;
  const client = createClient({
    baseUrl: "http://charet.test",
    token: "t0k",
    fetch: async (input) => {
      seen = input as Request;
      return Response.json({ ok: true });
    },
  });
  const { data } = await client.GET("/api/health");
  assert.deepEqual(data, { ok: true });
  assert.equal(seen?.url, "http://charet.test/api/health");
  assert.equal(seen?.headers.get("authorization"), "Bearer t0k");
  assert.equal(seen?.headers.get("x-charet-client"), "charet");
});

test("returns the shared error shape", async () => {
  const client = createClient({
    fetch: async () =>
      Response.json({ code: "unauthorized", message: "Sign in first" }, { status: 401 }),
    baseUrl: "http://charet.test",
  });
  const { error } = await client.GET("/api/auth/me");
  assert.equal(error?.code, "unauthorized");
});
