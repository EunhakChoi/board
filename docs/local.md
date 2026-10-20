# Running board locally

**English** | [한국어](local.ko.md)

board runs as three services: **board-server** (API, web tool, previews), **board-workspace**
(installs and builds project code) and **board-render** (Chromium: captures and checks). In local
mode the person at this machine needs no token: everyone who reaches port 5280 acts as the board's
admin, so the ports stay on this machine's loopback.

## With Docker (recommended)

Needs Docker with Compose v2.

```bash
git clone https://github.com/EunhakChoi/board.git
cd board
docker compose up -d
```

Open **http://localhost:5280** — `localhost`, not `127.0.0.1`. Previews are served from
`127.0.0.1:5281`, a different site, so a project's code can never read the tool's cookies.

The first build of the images takes a while (Rust and the web tool are compiled).

| Setting (environment or `.env`) | Default | |
|---|---|---|
| `BOARD_PORT` | `5280` | web tool and API |
| `BOARD_PREVIEW_PORT` | `5281` | previews |
| `BOARD_UPDATE_CHECK` | `true` | daily check for a new release (the only outside call) |
| `BOARD_TZ` | `UTC` | time zone of rendered screens (e.g. `Asia/Seoul`) |
| `BOARD_DATA_DIR` | (volume `board_data`) | a folder on this machine for projects, comments and captures |
| `BOARD_USER` | `board` | user the server runs as; with `BOARD_DATA_DIR`, the folder's owner (`uid:gid`) |

Data lives in Docker volumes:

| Volume | Holds |
|---|---|
| `board_data` | projects (git), versions, comments, captures, the preview signing key |
| `board_workspace` | dependency caches and builds (can be dropped; rebuilt on demand) |
| `board_secrets` | the token the services use with each other |

```bash
docker compose logs -f server        # logs
docker compose up -d --build         # update after `git pull`
docker compose down                  # stop (data is kept)
docker compose down -v               # stop and delete all data (not a BOARD_DATA_DIR folder)

# back up projects, comments and captures
docker run --rm -v board_data:/data -v "$PWD":/backup debian:trixie-slim \
  tar -czf /backup/board-data.tgz -C /data .
```

To keep the data in a folder of your choice instead of a volume, create the folder and point
`BOARD_DATA_DIR` at it, with `BOARD_USER` set to its owner (git refuses a repository owned by
someone else):

```bash
mkdir -p ~/board-data
printf 'BOARD_DATA_DIR=%s\nBOARD_USER=%s\n' ~/board-data "$(id -u):$(id -g)" >> .env
docker compose up -d
```

## Team and sharing

**Team** (in the header) lists the members. Add someone with a name and a role (admin, editor,
commenter, viewer) and hand over their **token**, or the link that carries it
(`…/?t=<token>`): it signs their browser in once and stays signed in. Tokens are shown and copied
from the Team page at any time; **New token** stops the old one. When the same token signs in on a
second browser, the first one is signed out (the CLI is not). People without a token are
**visitors**: they look at the team's projects under a made-up name, until you set *Visitors* to
*Token required*.

The same rules hold on every address except this machine's `localhost:5280`, which is the admin
(the member marked *This machine's admin*; their token works from other machines too).

To let other machines in, board has a second port, the **remote door** (`127.0.0.1:5284`,
`BOARD_REMOTE_BIND`). In **Team › Sharing**, pick how others reach it:

- **Tailscale network** — people on your tailnet (Tailscale, or Headscale over plain HTTP inside
  the tailnet). board runs `tailscale serve` and shows the machine's name.
- **Tailscale Funnel** — anyone with the address, at the machine's `ts.net` name. If Funnel is not
  enabled for the machine, the page shows the admin-console link.
- **ngrok** — the server runs an ngrok tunnel itself (no ngrok program needed). Paste the authtoken
  from your ngrok dashboard (kept on the server, never shown again) and, if you have one, your
  ngrok domain so the address stays the same. `BOARD_NGROK_AUTHTOKEN` also works.
- **My address** — a tunnel or reverse proxy you set up (ngrok, cloudflared, nginx…) pointing at
  `127.0.0.1:5284`. Never point one at 5280, which trusts everyone.
- **Local network** — the remote door also listens on this machine's LAN address, over plain HTTP.
  For trying things out: links show the IP, and tokens travel unencrypted. (In Docker, publish
  port 5284 instead.)

Links (member links, notifications) use the shared address while sharing is on. `board doctor`
says whether it answers. The Tailscale choices need the `tailscale` CLI where board-server runs
(`BOARD_TAILSCALE`, `BOARD_TAILSCALE_SOCKET`). In Docker use **My address** and run
`tailscale serve --bg http://127.0.0.1:5284` (or `tailscale funnel …`) on the host. Previews come
through the same address, sandboxed. For a server that is only ever used by a team, see
[public mode](public.md).

## The CLI

The CLI is not published yet; run it from the clone (Node.js 24+, pnpm 12):

```bash
pnpm install
alias board="node $PWD/apps/board-cli/src/main.ts"
board doctor                          # checks node, git, the server and its services
board setup                           # first-run guide; offers the skill for Claude Code / Codex
board init demo                       # a new project from the recommended template
board import ../my-app                # or an existing app (with the workspace packages it uses)
```

`BOARD_URL` points the CLI at another server (default `http://127.0.0.1:5280`).

## Without Docker

Needs Rust (stable), Node.js 24, pnpm 12, git and Chromium (or
`pnpm --filter board-web exec playwright install chromium-headless-shell`).

```bash
make setup
cargo build --release -p board-server -p board-render
pnpm --filter board-web run build
node apps/board-cli/src/main.ts serve    # server + workspace + render, Ctrl-C stops all
```

Data goes to `~/.board` (`BOARD_HOME`).

## Security notes

- Local mode trusts everyone who reaches the ports. Do not publish them beyond loopback; to share
  board with a team use [public mode](public.md).
- Project code (dependency installs, builds) runs only in board-workspace, with install scripts
  turned off and a minimal environment. Previews run in the browser on their own origin with a
  signed, short-lived token.
- The only outside call is the daily release check (`BOARD_UPDATE_CHECK=false` turns it off). No
  code, screens or usage data are sent.

## Troubleshooting

- `board doctor` names what is wrong and how to start it.
- Previews stay blank in the tool: open the tool as `http://localhost:<port>`, not `127.0.0.1`.
- Port in use: set `BOARD_PORT` / `BOARD_PREVIEW_PORT` and run `docker compose up -d` again.
