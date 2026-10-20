import assert from "node:assert/strict";
import { createServer, request, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { after, test } from "node:test";
import { createDoorProxy, createModelProxy } from "./proxy.ts";

const servers: Server[] = [];
after(() => {
  for (const s of servers) s.close();
});

function listen(server: Server): Promise<number> {
  servers.push(server);
  return new Promise((done) =>
    server.listen(0, "127.0.0.1", () => done((server.address() as AddressInfo).port)),
  );
}

test("the door relays /api/ to the agent door with the run's token", async () => {
  const upstream = await listen(
    createServer((req, res) => {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ path: req.url, auth: req.headers.authorization ?? null }));
    }),
  );
  const door = await listen(createDoorProxy(`http://127.0.0.1:${upstream}`));
  const res = await fetch(`http://127.0.0.1:${door}/api/projects/web`, {
    headers: { authorization: "Bearer abc" },
  });
  assert.deepEqual(await res.json(), { path: "/api/projects/web", auth: "Bearer abc" });
  assert.equal((await fetch(`http://127.0.0.1:${door}/other`)).status, 404);
});

test("the model proxy tunnels only to allowed hosts on 443", async () => {
  const proxy = await listen(createModelProxy(["api.anthropic.com"]));
  const status = (target: string) =>
    new Promise<number>((done, fail) => {
      const req = request({ host: "127.0.0.1", port: proxy, method: "CONNECT", path: target });
      req.on("connect", (res, socket) => {
        socket.destroy();
        done(res.statusCode ?? 0);
      });
      req.on("error", fail);
      req.end();
    });
  assert.equal(await status("example.com:443"), 403);
  assert.equal(await status("api.anthropic.com:80"), 403);
  assert.equal((await fetch(`http://127.0.0.1:${proxy}/`)).status, 403);
});
