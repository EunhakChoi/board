---
name: board
description: Work with a board project — real UI code (React) rendered with mock APIs as screens, flows and stories that a team reviews and comments on. Use when the user mentions board, a board project or screen, design comments to handle, or wants a UI change checked and captured.
---

# board

A board project is a real app (React + its own components) plus a `.board/` folder that declares
**screens** (states of a page, rendered with mock API data), **connections** between screens,
**flows** (the order a user goes through a feature) and **stories** (component states). The team
looks at them in the web tool and leaves **comments** pinned to elements. You change code; the
`board` CLI is your eyes.

| Who | Does |
|---|---|
| Team (web tool) | browse screens, flows and stories; click an element to comment |
| You (code) | edit the app and `.board/` in a checkout, `board push` |
| You (CLI) | understand structure, read comments, check and capture, reply and resolve |

## Before anything

```bash
board status            # inside a checkout: base version, newer versions, local changes
board comments          # open comments (exit 1 = some are open)
board ls                # screens, stories, flows and their capture state
```

- The project comes from the checkout you are in; elsewhere add `-p <project>`.
- "Cannot reach the board server" → `board doctor`, then `board serve` (or `docker compose up`).
- Every command takes `--json`.

## Workflow 1 · Handle comments

1. `board comments` → pick one → `board comments show c12`.
2. Look at what was pointed at: open the saved `snapshot` image (Read it) and use the `box`,
   `element`, `components` (outermost › innermost) and `source` (`file:line:col`). The comment is
   about *that element in that state*.
3. The innermost component is often a shared primitive (Button, Card). Decide whether the fix
   belongs in the screen-level component instead: shared changes ripple into many screens.
4. Ambiguous? Ask instead of guessing: `board comments reply c12 -m "question"`.
5. Fix the code in your checkout, `board push -m "…"`, then verify (Workflow 3).
6. `board comments resolve c12 -m "what changed (version)"` — only after looking at the new capture.

If a write fails with "changed meanwhile", someone else touched the comment. Re-read it and act on
the latest content.

## Workflow 2 · Build a UI feature (flow first)

1. Write or update the flow in `.board/project.tsx`. Steps that do not exist yet are planned:
   `{ id: "export-dialog", title: "Export dialog", note: "CSV, date range" }`.
2. `board flow <id>` lists the steps; planned ones are your to-do list.
3. Build the screen in the app. Add a screen with the **same id** as the planned step, with mocks
   and a connection from the previous step (see `references/project.md`, `references/mocks.md`).
4. Push and verify (Workflow 3). `board flow <id> --sheet /tmp/flow.jpg` shows the whole flow in one
   picture.
5. When the feature changes (a step added, split or dropped), edit the flow too.

## Workflow 3 · Verify a UI change

```bash
board push -m "…"                  # checks run on the pushed version
board check <id…>                  # errors, console errors, missing mocks (exit 1 on problems)
board capture <id…> --out /tmp/cv  # update what the team sees, and save the pictures for you
board ls --stale                   # everything whose capture may now be out of date
```

- `[board] missing mock GET /api/...` means the screen calls an API with no handler. Add one; don't
  hide it.
- Look at the saved pictures yourself before claiming success. A page can pass `check` and still
  look wrong.
- A shared component change can make many screens stale: `board capture $(board ls --stale)`.
- Add `--theme both` when dark mode matters.

## Understanding structure

```bash
board show <id|c12>              # details, connections in/out, flows, sources, open comments
board connections --from <id>    # transitions (click · auto · error)
board path <from> <to>           # how a user gets from one screen to another
board map --format mermaid       # whole page / screen map
board source <path[:line]>       # a file of main; `board source <screen>` = files it renders from
board url <id>                   # link to give the team
```

Connections and flows are hand-written. They can lag behind the real UI. When something looks odd,
confirm in the app code (router, buttons) and fix `.board/project.tsx`.

## Ground rules

- Work in a checkout (`board checkout <project>`), then `board push`. The server keeps every
  version; `board versions`, `board diff vN`.
- Comments and captures live on the server; change them only through the CLI or the web tool.
- Leave your own questions on the screen (`board comments add <id> --at x,y|selector -m "…"`), not
  only in chat, so the team finds them where they look.
- When you report back, include `board url <id>` links for what changed.

More: `references/cli.md` (every command), `references/project.md` (the `.board/` contract),
`references/mocks.md` (mock handlers).
