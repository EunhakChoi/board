# Put an existing app on board

How to bring an app that already lives in its own repository into board, so the team can review
its screens. The app's repository is never modified: board keeps a copy, and everything board
needs goes into `.board/` inside that copy.

## 1. Import

Run from anywhere; the path is the app folder (in a monorepo, the app inside it).

```bash
board import ../product/apps/web --key web --title "Web" --dry-run   # see what would be copied
board import ../product/apps/web --key web --title "Web"            # version 1
board checkout web ~/board/web                                        # your working copy
```

- The import copies the app, the workspace packages it uses and the root files a build needs
  (lockfile, workspace config). `--include dir` adds anything it missed. Ignored files
  (`node_modules`, `dist`) never travel.
- The checkout remembers its server; commands run inside it need no `--server`.

## 2. Add `.board/` next to the app's `package.json`

```
apps/web/.board/
  package.json         board-only dependencies
  pnpm-workspace.yaml  install settings for that folder
  pnpm-lock.yaml       written by `pnpm install` in .board/
  vendor/              the @board/sdk tarball (until the SDK is published)
  vite.config.ts       the app's own config + board()
  frame.html           entry page of the frame
  frame.tsx            providers + router for a screen, wrapper for a story
  project.tsx          pages, screens, connections, flows, stories
  mocks.ts             MSW handlers (split into mocks/ when it grows)
```

Why a separate `package.json`: the app's own `package.json` and lockfile must stay identical to
its repository, or every later import and handoff turns into a merge. board installs the app's
dependencies first, then `.board/`'s.

### package.json, pnpm-workspace.yaml

```json
{
  "name": "web-board",
  "private": true,
  "type": "module",
  "dependencies": {
    "@board/sdk": "file:vendor/board-sdk-<hash>.tgz",
    "msw": "2.15.0"
  }
}
```

```yaml
# React and vite come from the app; installing them again here would load React twice.
autoInstallPeers: false

allowBuilds:
  msw: false
```

- `"type": "module"` matters: without it vite loads `.board/vite.config.ts` as CommonJS and fails
  with "is not a function" errors.
- The SDK is not on npm yet. Pack it from the board repository and vendor it under a name that
  changes with its content, so the lockfile notices a new SDK:

  ```bash
  pnpm --dir <board-repo>/packages/sdk pack --pack-destination /tmp/sdk
  h=$(sha256sum /tmp/sdk/board-sdk-0.0.0.tgz | cut -c1-8)
  mkdir -p .board/vendor && cp /tmp/sdk/board-sdk-0.0.0.tgz .board/vendor/board-sdk-$h.tgz
  ```

- Mock helpers the project needs (e.g. `openapi-msw`) go here too, never into the app.
- Then `pnpm install` inside `.board/` (commit the lockfile it writes).
- With npm instead of pnpm: same `package.json`, `npm install` inside `.board/`, keep
  `package-lock.json`.

### vite.config.ts

Reuse the app's config so aliases, CSS and plugins match production, and add `board()`:

```ts
import { board } from "@board/sdk/vite";
import { defineConfig, mergeConfig, type UserConfig } from "vite";
import app from "../vite.config.ts";

export default defineConfig(async (env) => {
  const base = (typeof app === "function" ? await app(env) : app) as UserConfig;
  return mergeConfig(base, { plugins: [board()] });
});
```

board runs vite from the app folder with this file, so `root` is the app and the app's `public/`
still works. Steps the app's `prebuild` script normally does (copying assets etc.) must happen in
a plugin here, because board calls vite directly.

### frame.html, frame.tsx

`frame.html` is a plain page loading `./frame.tsx` (see `project.md`). `frame.tsx` renders a
screen the way the app does: same providers (query client, theme, i18n, auth stub) and the app's
routes in a memory router at the screen's route. Read the app's `main.tsx` and copy its provider
tree; leave out anything that talks to a real backend on start.

### project.tsx, mocks

See `project.md` and `mocks.md`. Start small: one page, its main screen, an empty state and an
error state. More screens are cheap once the first one renders.

The manifest is built by importing `project.tsx` in Node, so modules it pulls in must load
without a browser. Lazy-load browser-only libraries (PDF viewers, canvas editors) inside
components: `const Viewer = lazy(() => import("@/features/pdf/Viewer"))`.

## 3. Try it, push, check

```bash
board dev          # installs .board/ and runs vite with .board/vite.config.ts
                   # → open /.board/frame.html?screen=<id> on the printed port
board push -m "board: first screens"
board check        # every screen and story; fix each missing mock and console error
board capture --theme both
board url <id>     # links for the team
```

`check` and `capture` run on the pushed version, so push before checking: an unpushed fix is
invisible to them.

## Common problems

| Symptom | Cause · fix |
|---|---|
| `[board] missing mock GET /api/…` | No handler: add one (screen `mocks` or project `mocks`) |
| Broken images or fonts on previews | Absolute paths into `public/` are rewritten only when written as string literals; build URLs from `import.meta.env.BASE_URL` otherwise |
| Times differ from what people expect | Rendering uses the render service's time zone (`BOARD_TZ`); pin dates in mocks |
| Hooks error / "Invalid hook call" | React installed twice: check `autoInstallPeers: false` and that `.board/package.json` has no `react` |
| Preview build failed | `board check` prints the reason; `curl <server>/api/projects/<key>/preview` lists each step (extract, install, build) with the end of its output |
