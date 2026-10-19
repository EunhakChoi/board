// A stand-in OpenID Connect provider for the public-mode tests. Its sign-in page lists accounts
// to pick (`<name>@example.com`); the token endpoint returns an unsigned ID token for the pick.

import { createServer } from "node:http";

export function startIdp(port: number): () => void {
  const issuer = `http://127.0.0.1:${port}`;
  const nonces = new Map<string, string>();
  const server = createServer((req, res) => {
    const url = new URL(req.url ?? "/", issuer);
    const json = (body: unknown) => {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(body));
    };
    if (url.pathname === "/.well-known/openid-configuration") {
      return json({
        issuer,
        authorization_endpoint: `${issuer}/auth`,
        token_endpoint: `${issuer}/token`,
      });
    }
    if (url.pathname === "/auth") {
      const back = url.searchParams.get("redirect_uri") ?? "";
      const state = url.searchParams.get("state") ?? "";
      const nonce = url.searchParams.get("nonce") ?? "";
      const links = ["boss", "eve", "ann"].map((who) => {
        const code = `${who}.${Math.random().toString(36).slice(2)}`;
        nonces.set(code, nonce);
        const target = `${back}?state=${encodeURIComponent(state)}&code=${encodeURIComponent(code)}`;
        return `<p><a href="${target}">${who}@example.com</a></p>`;
      });
      res.writeHead(200, { "content-type": "text/html" });
      return res.end(`<!doctype html><title>Test provider</title>${links.join("")}`);
    }
    if (url.pathname === "/token" && req.method === "POST") {
      let body = "";
      req.on("data", (chunk) => {
        body += chunk;
      });
      req.on("end", () => {
        const code = new URLSearchParams(body).get("code") ?? "";
        const who = code.split(".")[0] ?? "";
        const claims = {
          iss: issuer,
          sub: who,
          aud: "e2e-client",
          exp: Math.floor(Date.now() / 1000) + 300,
          nonce: nonces.get(code),
          email: `${who}@example.com`,
          email_verified: true,
          name: who[0]?.toUpperCase() + who.slice(1),
        };
        const payload = Buffer.from(JSON.stringify(claims)).toString("base64url");
        json({ id_token: `e30.${payload}.sig`, access_token: "a" });
      });
      return;
    }
    res.writeHead(404);
    res.end();
  });
  server.listen(port, "127.0.0.1");
  return () => server.close();
}
