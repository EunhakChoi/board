# Sharing board with a team (public mode)

**English** | [한국어](public.ko.md)

In [local mode](local.md) board trusts everyone who reaches its ports. Public mode is for a team:
everyone signs in, and an admin decides who gets in.

## What you need

- **A fixed address** for the tool and one for previews, both over HTTPS, e.g.
  `https://board.example.com` and `https://preview.board.example.com`. A reverse proxy (Caddy,
  nginx) or a tunnel with fixed domains (Cloudflare Tunnel, ngrok reserved domains) in front of
  ports 5280 and 5281. Previews must be a different host from the tool: they run project code and
  must never see the tool's cookies.
- **An OpenID Connect client.** Google is the default:
  1. Google Cloud console → *APIs & Services* → *Credentials* → *Create credentials* →
     *OAuth client ID* → *Web application*.
  2. Authorized redirect URI: `https://board.example.com/api/auth/oidc/callback`.
  3. Keep the client ID and secret.

  Any OIDC provider works: set `BOARD_OIDC_ISSUER` to its issuer URL.

## Start

Add to `.env` next to `compose.yaml`:

```bash
BOARD_PUBLIC_URL=https://board.example.com
BOARD_PUBLIC_PREVIEW_URL=https://preview.board.example.com
BOARD_OIDC_CLIENT_ID=…
BOARD_OIDC_CLIENT_SECRET=…
# optional: addresses that become admins on their first sign-in
BOARD_ADMIN_EMAILS=you@example.com
```

```bash
docker compose -f compose.yaml -f compose.public.yaml up -d
docker compose logs server | grep -A1 "setup link"
```

Without Docker: `board-server --public` with the same variables (`BOARD_PREVIEW_URL` instead of
`BOARD_PUBLIC_PREVIEW_URL`). The server refuses to start in public mode without sign-in or a
fixed address.

## The first admin

Until an admin exists, nobody gets in. On start the server prints a **one-time setup link** to
its log; the first account that signs in through it becomes the admin. An address in
`BOARD_ADMIN_EMAILS` becomes an admin on its own. The link stops working once used, and a new one
is printed on each start while there is still no admin.

## Who gets in

- Addresses or domains on the **allowlist** get in on their first sign-in, as editors.
- Anyone else sees "Waiting for approval" while an admin approves or declines the **access
  request**.

## Security notes

- Sessions are `HttpOnly`, `SameSite=Lax` cookies, `Secure` over HTTPS, lasting 30 days. Changing
  requests also need the `x-board-client` header, which other sites cannot send.
- Only token hashes are stored. Removing or declining someone ends their sessions at once.
- The client secret can come from a file: `BOARD_OIDC_CLIENT_SECRET_FILE`.
