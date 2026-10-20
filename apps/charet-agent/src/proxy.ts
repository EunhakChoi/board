// The only ways out of a run container (its network is internal):
// - the door: plain HTTP relayed to the server's agent door (which checks the run's token);
// - the model proxy: HTTPS CONNECT tunnels to the allowed model API hosts on port 443.

import { createServer, request, type Server } from "node:http";
import { request as requestTls } from "node:https";
import { connect } from "node:net";

/** Relays `/api/*` requests to the agent door at `target`. */
export function createDoorProxy(target: string): Server {
  const base = new URL(target);
  const send = base.protocol === "https:" ? requestTls : request;
  return createServer((req, res) => {
    if (!req.url?.startsWith("/api/")) {
      res.writeHead(404, { "content-type": "application/json" });
      res.end('{"code":"not_found","message":"only /api/ goes through"}');
      return;
    }
    const headers = { ...req.headers, host: base.host };
    const upstream = send(
      {
        protocol: base.protocol,
        hostname: base.hostname,
        port: base.port,
        method: req.method,
        path: req.url,
        headers,
      },
      (answer) => {
        res.writeHead(answer.statusCode ?? 502, answer.headers);
        answer.pipe(res);
      },
    );
    upstream.on("error", () => {
      if (!res.headersSent) {
        res.writeHead(502, { "content-type": "application/json" });
        res.end('{"code":"unavailable","message":"the charet server does not answer"}');
      } else res.destroy();
    });
    req.pipe(upstream);
  });
}

/** An HTTPS proxy that opens tunnels only to `allowed` hosts on port 443. */
export function createModelProxy(allowed: string[]): Server {
  const hosts = new Set(allowed.map((h) => h.toLowerCase()));
  const server = createServer((_req, res) => {
    res.writeHead(403, { "content-type": "text/plain" });
    res.end("only HTTPS tunnels to the model APIs\n");
  });
  server.on("connect", (req, socket, head) => {
    const match = /^([a-z0-9.-]+):(\d+)$/i.exec(req.url ?? "");
    const host = match?.[1]?.toLowerCase();
    if (!host || match?.[2] !== "443" || !hosts.has(host)) {
      socket.end("HTTP/1.1 403 Forbidden\r\n\r\n");
      return;
    }
    const upstream = connect(443, host, () => {
      socket.write("HTTP/1.1 200 Connection Established\r\n\r\n");
      if (head.length) upstream.write(head);
      upstream.pipe(socket);
      socket.pipe(upstream);
    });
    const close = () => {
      upstream.destroy();
      socket.destroy();
    };
    upstream.on("error", () => {
      if (socket.writable) socket.end("HTTP/1.1 502 Bad Gateway\r\n\r\n");
      close();
    });
    socket.on("error", close);
  });
  return server;
}
