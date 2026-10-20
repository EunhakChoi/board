# Sharing charet with a team (public mode)

**English** | [한국어](public.ko.md)

[Local mode](local.md) is a charet on one machine, shared when needed. Public mode is for a server
only a team uses, at a fixed HTTPS address. In both, nobody is trusted by address. Members come in with the **token** an
admin hands over; everyone else is a visitor, who may look, also comment, or not come in at all, as
the admin sets it.

## What you need

- **A fixed address** people open, e.g. `https://charet.example.com`. One is enough: previews
  (which run project code) are then served from the same address under `/_preview/`, in a
  sandboxed frame with an opaque origin, so project code cannot reach the tool or its session.
  Optionally give previews their own origin with `CHARET_PUBLIC_PREVIEW_URL` (another host, or the
  same host on another port). Ways to get the address:

  | How | Address |
  |---|---|
  | Your domain behind a proxy or Cloudflare Tunnel | `https://charet.example.com` |
  | Tailscale Funnel (free, fixed name) | `https://pc.tailnet.ts.net` (`tailscale funnel --bg 5280`) |
  | ngrok (one free static domain) | `https://name.ngrok-free.app` (`ngrok http --url=… 5280`) |
  | Office LAN (testing; not encrypted) | `http://192.168.0.10:5280` |

  Prefer an address with a name: links people get then show no IP address.
## Start

Add to `.env` next to `compose.yaml`:

```bash
CHARET_PUBLIC_URL=https://charet.example.com
# optional: previews on their own origin instead of the tool's address
# CHARET_PUBLIC_PREVIEW_URL=https://preview.charet.example.com
```

```bash
docker compose -f compose.yaml -f compose.public.yaml up -d
docker compose logs server | grep -A1 "Admin token"
```

Without Docker: `charet-server --public` with the same variables (`CHARET_PREVIEW_URL` instead of
`CHARET_PUBLIC_PREVIEW_URL`). The server refuses to start in public mode without a fixed address.

Then check it from any machine: `charet doctor --server https://charet.example.com`. Besides the
services it checks that the address is HTTPS (tokens travel with every request).

## The first admin

On its first start the server makes the admin (named *Admin*; rename it on the Team page) and
prints their **token** to its log, once. Open `https://charet.example.com/?t=<token>`, or enter the
token on the page. Lost it? `charet-server admin-token` (in Docker:
`docker compose exec server charet-server admin-token --public`) prints it again.

## Who gets in

- **Members** (Team → Members): add a name and a role, then hand over the token or the link that
  carries it. The browser stays signed in; a second browser with the same token signs the first
  one out (the CLI is not affected: `charet login --token …`). **New token** stops the old one.
- **Visitors**: without a token, people come in under a made-up name. *Visitors* on the Team page
  sets what they may do on the team's projects: *Token required* (not at all), *Can look* (screens,
  flows, code and comments) or *Can comment*. A project can set its own level under **Access**
  (*Server setting* follows the Team page), so one project can take outside comments while the rest
  needs a token. Private projects never show to visitors, and downloading a whole project
  (`charet checkout`) always needs a token.

## Notifications and webhooks

- **Inbox**: a new comment reaches everyone who can see the project; a reply, resolve or reopen
  reaches the people in that thread. Nobody is told about their own action. The bell in the header shows them; agents read them with `charet inbox`.
- **Webhooks** (Team → Webhooks, admins; works in local mode too): pick the events
  (`comment.added`, `comment.replied`, `comment.resolved`, `comment.reopened`,
  `version.created`) and optionally one project. A private project's events
  only go to webhooks set to that project. Each delivery is a JSON `POST`:

  ```json
  { "event": "comment.added", "project": "web", "actor": "Ann",
    "text": "Ann commented on screen home: …", "url": "https://charet.example.com/p/web/…",
    "data": { "comment": 12, "kind": "screen", "target": "home", "body": "…" } }
  ```

  Check `X-Charet-Signature: sha256=<hex>`: the HMAC-SHA256 of `<X-Charet-Timestamp>.<body>` with
  the secret shown when the webhook was made. Failed deliveries are retried twice (after 2 s and
  10 s); the Team page shows the last status. "Send test" posts a `ping`.

## Security notes

- Browser sessions are `HttpOnly`, `SameSite=Lax` cookies, `Secure` over HTTPS, lasting a year.
  Changing requests also need the `x-charet-client` header, which other sites cannot send.
- Member tokens are stored as they are, so admins can hand them over again; session cookies are
  stored as hashes. Removing a member or giving them a new token ends their sessions at once.
- Webhook secrets are derived from the server key (`CHARET_SECRET_KEY` or `data/secret.key`) and
  never stored; changing that key changes every webhook secret.
