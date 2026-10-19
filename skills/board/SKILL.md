---
name: board
description: Work with board, a code-first design board where an app's real React UI is rendered with mock APIs as screens, flows and component stories that a team reviews and comments on. Use this skill whenever the user mentions board or a board project, design or UI review comments to handle, screens, flows or stories, a `.board/` folder or a board checkout, putting an app on board (import), or wants a UI change checked, captured or shown to the team — even if they only say "check the screen", "the designer left comments" or "make this reviewable".
---

# board

A board project is a real app (React + its own components) plus a `.board/` folder that declares
**screens** (a page in one state, rendered with mock API data), **connections** between screens,
**flows** (the order a user goes through a feature) and **stories** (component states). The team
looks at them in the web tool and pins **comments** to elements. You change code in a checkout;
the `board` CLI is how you see what the team sees.

| Who | Does |
|---|---|
| Team (web tool) | browse screens, flows and stories; click an element to comment |
| You (code) | edit the app and `.board/` in a checkout, `board push` |
| You (CLI) | read structure and comments, check and capture, reply and resolve |

The server keeps its own copy of the project with a version per push. The product repository is
not touched by board; a checkout is a working copy of board's copy, not of the product repo.

## Start here

```bash
board status            # inside a checkout: base version, newer versions, local changes
board comments          # open comments (exit 1 while some are open)
board ls                # screens, stories, flows and whether their captures are current
```

- Commands run inside a checkout use its project and server. Elsewhere add `-p <project>`
  (and `--server <url>` when the server is not the default `http://127.0.0.1:5280`).
- No checkout yet: `board projects`, then `board checkout <project> <dir>`.
- If `board status` says main moved, `board pull` before editing — it saves a refused push later.
- "Cannot reach the board server" → `board doctor` says which part is down.
- "Sign in first" / "token was revoked" → a team server: ask the person to run `board login`
  (they approve it in the browser); CI uses `BOARD_TOKEN`.
- On a team server, `board inbox` lists what concerns the person (new comments, answers in their
  threads) with links — a quick way to find what to handle first.
- Every command takes `--json` when you need to parse the output.

## Workflow 1 · Handle comments

1. `board comments`, then `board comments show c12`. It saves the snapshot the comment was made
   on and prints its path, the pinned element, its component chain (outermost › innermost) and
   the source location.
2. Read the snapshot image. A comment is about *that element in that state*; the text alone is
   often ambiguous ("make this clearer").
3. The innermost component is often a shared primitive (Button, Card). Fix it there only when the
   comment is really about the primitive — a shared change alters every screen using it.
4. If you cannot tell what is wanted, ask on the comment instead of guessing:
   `board comments reply c12 -m "…"`. The team reads replies where they left the comment.
5. Fix the code, push, verify (Workflow 3).
6. `board comments resolve c12 -m "what changed (vN)"` — after you have looked at the new capture.
   The note is what the reviewer reads to decide whether to reopen.

If a reply or resolve fails with "changed meanwhile", someone touched the comment. Re-read it and
act on the latest content.

## Workflow 2 · Build a UI feature (flow first)

A flow written before the code is the plan the team can review; planned steps are your to-do list.

1. In `.board/project.tsx`, add or extend a flow. Steps that do not exist yet are planned:
   `{ id: "export-dialog", title: "Export dialog", note: "CSV, date range" }`.
2. Build the UI in the app.
3. Add a screen with the **same id** as each planned step: its page, mocks for the state it shows
   and, if it needs interaction to reach that state, a `setup`. Add connections from the screens
   that lead to it. Empty, loading and error states are screens of their own.
   (`references/project.md`, `references/mocks.md`)
4. Push and verify (Workflow 3); `board flow <id> --sheet /tmp/flow.jpg` shows the whole flow.
5. When the feature changes shape (a step added, split or dropped), change the flow too, so it
   keeps describing what was built.

## Workflow 3 · Verify a UI change

```bash
board push -m "…"                  # check and capture run on the pushed version
board check <id…>                  # page errors, console errors, missing mocks (exit 1 on problems)
board capture <id…> --out /tmp/cv  # update what the team sees, and save the pictures for you
board ls --stale                   # screens whose capture is older than the code
```

- Push first: `check` and `capture` build and render the latest pushed version, so an unpushed
  fix is invisible to them.
- `[board] missing mock GET /api/…` means the screen calls an API no handler answers. Add the
  handler; a silent failure there would show the team a broken screen.
- Open the saved pictures and look at them before saying it works. A page can pass `check` and
  still be wrong (empty table, clipped dialog, wrong state).
- A shared component change makes many screens stale: `board capture $(board ls --stale)`.
- Add `--theme both` when the change can look different in dark mode.
- Push refused because someone pushed first: a checkout is a git work tree, so
  `git stash && board pull && git stash pop`, then push again. Their version stays; yours lands on
  top.

## Workflow 4 · Put an existing app on board

`board import <app>` copies the app into board; then you add `.board/` (board-only dependencies,
vite config, frame, project, mocks) in the checkout, push and check. The app's own `package.json`
and lockfile stay exactly as in its repository. Follow `references/import.md` step by step — it
covers getting the SDK, the files to write and the usual failures.

## Understanding structure

```bash
board show <id|c12>              # details, connections in/out, flows, sources, open comments
board connections --from <id>    # transitions (click · auto · error)
board path <from> <to>           # how a user gets from one screen to another
board map --format mermaid       # every page and screen with connections
board source <screen>            # the files a screen renders from; `board source <path[:line]>` reads one
board url <id>                   # link to give the team
```

Connections and flows are written by hand and can lag behind the UI. When one looks wrong, check
the app code (router, buttons, links) and answer from the code. If you are already changing the
project, fix `.board/project.tsx` too. If you were only asked a question, say what is wrong and
offer the fix (or leave a board comment) instead of pushing a change nobody asked for — a push
creates a version the whole team sees.

## Ground rules

- Change comments and captures only through the CLI or the web tool; the server tracks revisions
  and pin positions.
- Leave your own questions or notes on the screen (`board comments add <id> --at x,y|selector -m
  "…"`), not only in chat, so the team finds them where they look.
- Deleting a project (`board delete`) cannot be undone; do it only when the user asks.
- Vague requests ("clean this up", "make it nicer"): tidying board itself is fine — missing or
  stale captures, connections that disagree with the code. Changes to how the app looks or behaves
  are design decisions: list them, or leave them as comments on the screens, and let the user
  choose before you change the app.
- Report back with what changed, the version, and `board url <id>` links for the screens.

## References

- `references/cli.md` — every command and its output
- `references/project.md` — the `.board/` contract: project.tsx rules, setup kit, frame, static files
- `references/mocks.md` — writing MSW handlers
- `references/import.md` — bringing an existing app onto board
