# charet CLI reference

All commands accept `--json` (machine-readable output), `-p <project>` (default: the checkout you
are in) and `--server <url>` (default: the checkout's server, else `CHARET_URL`, else
`http://127.0.0.1:5280`).

## Get started

| Command | Does |
|---|---|
| `charet setup [--yes]` | First-run guide: checks the server, offers to install this skill |
| `charet login [--token t]` | Signs in to a team server: prints a code, the person approves it in the browser (`--token` uses a personal token instead). Not needed in local mode |
| `charet whoami` · `charet logout` | Who the CLI acts as · sign out and revoke the token |
| `charet doctor` | Checks node, git, the server, charet-workspace, charet-render, cookie separation (exit 1 on failure) |
| `charet serve [--server-only]` | Runs charet-server with charet-workspace and charet-render |
| `charet skill install [--claude] [--codex]` | Installs this skill for Claude Code (`~/.claude/skills/charet`) and / or Codex (`~/.codex/skills/charet`) |

## Projects and versions

| Command | Does |
|---|---|
| `charet projects` | Lists projects |
| `charet delete <project> [--yes]` | Deletes a project with its versions, captures and comments (no undo; asks for the key). Only when the user asks |
| `charet init <key> [dir] [--template recommended]` | Creates a project from a template and checks it out |
| `charet import [path] [--key k] [--include dir]… [--dry-run]` | Imports an existing app (and the workspace packages it uses) |
| `charet checkout <project> [dir] [--rev vN]` | Downloads a project into a folder |
| `charet status` | Base version, whether main moved, local changes |
| `charet push [-m msg]` | Sends local changes as a new version (fails if someone pushed first: `charet pull`, then push) |
| `charet pull` | Updates the checkout to the latest version (refuses with unpushed changes) |
| `charet dev` | Runs the app's vite dev server in the checkout |
| `charet versions` · `charet diff <from> [to]` | Version list · changed paths |

## Structure

| Command | Output |
|---|---|
| `charet ls` | Screens (id, title, page, capture state, open comments), stories, flows |
| `charet ls --stale` | Ids that need a capture (never captured or stale), one per line |
| `charet show <id>` | Screen: page, route, size, note, from/to connections, flows, sources, issues, open comments. Also pages, flows, components (`Button`), comments (`c12`) |
| `charet url [id]` | Web tool link |
| `charet connections [--from s] [--to s] [--page p]` | `from → to "label" (kind)` |
| `charet path <from> <to> [--sheet f.jpg]` | Shortest chain of connections; exit 1 when there is none |
| `charet map [--format text\|mermaid]` | Pages, screens and their outgoing connections |
| `charet flows` · `charet flow <id> [--sheet f.jpg]` | Flows · numbered steps with capture state, planned steps marked |
| `charet sheet <id>… --out f.jpg [--theme dark]` | Captures side by side (grey = planned or not captured) |
| `charet source <path[:line]> [--context n]` | A file of main with line numbers (`>` marks the line). Paths may be relative to the app |
| `charet source <screen\|story>` | The files that screen rendered from (needs a capture) |

## Check and capture

Both run on the pushed version (`--rev vN` for another) and wait for its preview build.

| Command | Does |
|---|---|
| `charet check [id…] [--theme light\|dark\|both]` | Opens pages, reports page errors, console errors, missing mocks. Nothing is stored. Exit 1 on problems |
| `charet capture [id…] [--theme …] [--force] [--out dir]` | Stores captures the web tool shows and re-anchors comment pins. Already captured at this commit = `unchanged` unless `--force`. `--out` saves `<id>.jpg` / `<id>.dark.jpg` |

No ids = every screen and story.

## Comments

| Command | Does |
|---|---|
| `charet comments [--all\|--resolved] [--on id]` | Lists open comments (exit 1 while any listed one is open) |
| `charet comments show c12 [--out dir]` | Body, box, element, components, source, replies; saves the snapshot image and prints its path |
| `charet comments add <id> --at x,y\|selector -m text [--theme dark]` | Pins a comment to the smallest element at the point (or the selector) |
| `charet comments reply c12 -m text` | Replies |
| `charet comments resolve c12 [-m note]` · `reopen c12 [-m note]` | Resolves · reopens |
| `charet inbox [--all]` | Your notifications on a team server (new comments, answers in your threads, access requests for admins), each with a link; unread only unless `--all` |
| `charet inbox read [id...]` | Marks those notifications (or all) read |
