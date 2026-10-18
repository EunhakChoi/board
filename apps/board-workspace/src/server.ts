// Internal HTTP API. Only board-server calls it (bearer token); the preview origin reaches built
// files through board-server, never directly.
//
//   GET    /health
//   PUT    /v1/builds/{project}/{commit}?base=&verify=1&retry=1   body: gzip tar of the tree
//   GET    /v1/builds/{project}/{commit}                          build status
//   GET    /v1/builds/{project}/{commit}/log                      full log (text)
//   DELETE /v1/builds/{project}/{commit}
//   GET    /v1/files/{project}/{commit}/{path}                    a built file

import { timingSafeEqual } from "node:crypto";
import { createReadStream, existsSync, statSync } from "node:fs";
import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";
import { extname, join, normalize, sep } from "node:path";
import { Transform } from "node:stream";
import { type Builds, validCommit, validProject } from "./builds.ts";

export interface ServerOptions {
  builds: Builds;
  /** Required bearer token; undefined only for loopback-only local use. */
  token: string | undefined;
  maxUploadBytes: number;
  version: string;
}

const TYPES: Record<string, string> = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json",
  ".map": "application/json",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".gif": "image/gif",
  ".webp": "image/webp",
  ".avif": "image/avif",
  ".ico": "image/x-icon",
  ".woff": "font/woff",
  ".woff2": "font/woff2",
  ".ttf": "font/ttf",
  ".otf": "font/otf",
  ".txt": "text/plain; charset=utf-8",
  ".wasm": "application/wasm",
};

export function createWorkspaceServer(options: ServerOptions): Server {
  return createServer((req, res) => {
    handle(options, req, res).catch((error: unknown) => {
      if (!res.headersSent) send(res, 500, { code: "internal", message: String(error) });
      else res.destroy();
    });
  });
}

async function handle(
  options: ServerOptions,
  req: IncomingMessage,
  res: ServerResponse,
): Promise<void> {
  const url = new URL(req.url ?? "/", "http://workspace");
  const parts = url.pathname.split("/").filter(Boolean).map(decodeURIComponent);
  if (url.pathname === "/health" && req.method === "GET") {
    return send(res, 200, { status: "ok", version: options.version });
  }
  if (!authorized(options.token, req.headers.authorization)) {
    return send(res, 401, { code: "unauthorized", message: "missing or wrong token" });
  }
  const [v1, kind, project, commit, ...rest] = parts;
  if (v1 !== "v1" || (kind !== "builds" && kind !== "files") || !project || !commit) {
    return send(res, 404, { code: "not_found", message: "no such route" });
  }
  if (!validProject(project) || !validCommit(commit)) {
    return send(res, 400, { code: "invalid", message: "bad project key or commit" });
  }
  const { builds } = options;

  if (kind === "files") {
    if (req.method !== "GET" && req.method !== "HEAD") return send(res, 405, methodNotAllowed());
    return serveFile(builds, project, commit, rest.join("/"), req, res);
  }

  if (rest.length === 1 && rest[0] === "log" && req.method === "GET") {
    const log = builds.logPath(project, commit);
    if (!builds.get(project, commit) || !existsSync(log)) return send(res, 404, notFound());
    res.writeHead(200, { "content-type": "text/plain; charset=utf-8" });
    createReadStream(log).pipe(res);
    return;
  }
  if (rest.length > 0) return send(res, 404, { code: "not_found", message: "no such route" });

  switch (req.method) {
    case "GET": {
      const build = builds.get(project, commit);
      return build ? send(res, 200, build) : send(res, 404, notFound());
    }
    case "DELETE": {
      if (!builds.get(project, commit)) return send(res, 404, notFound());
      return builds.remove(project, commit)
        ? void res.writeHead(204).end()
        : send(res, 409, { code: "conflict", message: "the build is still running" });
    }
    case "PUT": {
      const base = url.searchParams.get("base") ?? `/${project}/${commit}/`;
      if (!/^\/([A-Za-z0-9._~-]+\/)*$/.test(base)) {
        return send(res, 400, {
          code: "invalid",
          message: "base must be an absolute path ending in /",
        });
      }
      const declared = Number(req.headers["content-length"] ?? "0");
      if (declared > options.maxUploadBytes) {
        return send(res, 413, {
          code: "too_large",
          message: `source is over ${options.maxUploadBytes} bytes`,
        });
      }
      const { build, started } = await builds.start(
        {
          project,
          commit,
          base,
          verify: url.searchParams.get("verify") === "1",
          retry: url.searchParams.get("retry") === "1",
        },
        req.pipe(limit(options.maxUploadBytes)),
      );
      return send(res, started ? 202 : 200, build);
    }
    default:
      return send(res, 405, methodNotAllowed());
  }
}

function serveFile(
  builds: Builds,
  project: string,
  commit: string,
  path: string,
  req: IncomingMessage,
  res: ServerResponse,
): void {
  if (builds.get(project, commit)?.state !== "ready") {
    send(res, 404, notFound());
    return;
  }
  const root = builds.outDir(project, commit);
  const file = join(root, normalize(path === "" ? "index.html" : path));
  if (file !== root && !file.startsWith(root + sep)) {
    send(res, 400, { code: "invalid", message: "bad path" });
    return;
  }
  if (!existsSync(file) || !statSync(file).isFile()) {
    send(res, 404, notFound());
    return;
  }
  const immutable = file.startsWith(join(root, "assets") + sep);
  res.writeHead(200, {
    "content-type": TYPES[extname(file).toLowerCase()] ?? "application/octet-stream",
    "content-length": statSync(file).size,
    // Hashed assets never change; pages are revalidated.
    "cache-control": immutable ? "public, max-age=31536000, immutable" : "no-cache",
    "x-content-type-options": "nosniff",
  });
  if (req.method === "HEAD") {
    res.end();
    return;
  }
  createReadStream(file).pipe(res);
}

function authorized(token: string | undefined, header: string | undefined): boolean {
  if (token === undefined) return true;
  const given = Buffer.from(header?.startsWith("Bearer ") ? header.slice(7) : "");
  const wanted = Buffer.from(token);
  return given.length === wanted.length && timingSafeEqual(given, wanted);
}

/** Fails the stream once more than `max` bytes went through (chunked uploads have no length). */
function limit(max: number): Transform {
  let seen = 0;
  return new Transform({
    transform(chunk: Buffer, _encoding, callback) {
      seen += chunk.length;
      callback(seen > max ? new Error(`source is over ${max} bytes`) : null, chunk);
    },
  });
}

function send(res: ServerResponse, status: number, body: unknown): void {
  const text = JSON.stringify(body);
  res.writeHead(status, {
    "content-type": "application/json",
    "content-length": Buffer.byteLength(text),
  });
  res.end(text);
}

const notFound = () => ({ code: "not_found", message: "no such build" });
const methodNotAllowed = () => ({ code: "method_not_allowed", message: "method not allowed" });
