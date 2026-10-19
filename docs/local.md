# Running board locally

**English** | [한국어](local.ko.md)

board runs as three services: **board-server** (API, web tool, previews), **board-workspace**
(installs and builds project code) and **board-render** (Chromium: captures and checks). In local
mode there is no sign-in: everyone who can reach the ports acts as the built-in admin, so the ports
stay on this machine's loopback.

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
