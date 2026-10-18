# The `.board/` contract

A board project is an ordinary vite + React app with a `.board/` folder:

```
.board/
  project.tsx   what the board shows: pages, screens, connections, flows, stories
  frame.tsx     how a screen or story is rendered (router, providers, global CSS)
  frame.html    the frame's HTML entry
  mocks.ts      mock data and handlers (any name; imported by project.tsx)
```

The `board()` vite plugin (from `@board/sdk/vite`) builds the frame and the manifest the server
reads. It only acts in dev and in board's preview builds; the app's own build is untouched.

## project.tsx

```tsx
import { defineProject } from "@board/sdk";
import { HttpResponse, http } from "msw";
import { Button } from "@/components/ui/button";
import { sampleTasks, tasks } from "./mocks.ts";

export default defineProject({
  title: "Tasks",
  screenSize: { w: 1440, h: 900 },          // default viewport
  mocks: [],                                // handlers every screen falls back to
  pages: [{ id: "tasks", route: "/", title: "Tasks" }],
  screens: [
    { id: "tasks-list", page: "tasks", title: "Task list", mocks: [tasks(sampleTasks)] },
    {
      id: "tasks-open",
      page: "tasks",
      title: "Open tasks only",
      mocks: [tasks(sampleTasks)],
      setup: (kit) => kit.click("button", "Open"),   // bring the screen into its state
    },
    {
      id: "tasks-error",
      page: "tasks",
      title: "Load failed",
      mocks: [http.get("/api/tasks", () => new HttpResponse(null, { status: 500 }))],
    },
  ],
  connections: [{ from: "tasks-list", to: "tasks-open", label: "Open", kind: "click" }],
  flows: [
    {
      id: "triage",
      title: "Triage tasks",
      steps: ["tasks-list", "tasks-open", { id: "task-detail", title: "Task detail", note: "planned" }],
    },
  ],
  stories: [
    { id: "button-default", component: "Button", title: "Default", render: () => <Button>Save</Button> },
  ],
});
```

Rules (checked when the project loads):

- Ids are lowercase letters, digits and dashes, unique within their kind.
- A screen's `page` names a page; `route` defaults to the page's route; `size` to `screenSize`.
- Connections and non-planned flow steps name screens. Kinds: `click` (default), `auto`, `error`.
- A planned step (`{ id, title, note? }`) is a screen that does not exist yet. Adding a screen with
  that id turns it into a real step.
- A flow step object whose id is an existing screen stays a real step with its own `title` / `note`.
- Pages and flows take an optional `note`, shown in the web tool and the CLI.
- A screen's `query` (`"?tab=runs"`) is set on the URL before it renders.
- `setup(kit)`: `kit.click(selector, text?)`, `kit.fill(selector, value)`, `kit.find(selector, text?)`,
  `kit.wait(ms)`. Selectors are CSS, searched in the whole document (dialogs in portals too); text
  matches the trimmed text or the `aria-label`.
- Stories are grouped by `component` in the Components view. A story's `mocks` start MSW like a
  screen's.

## Imported apps

An imported app keeps its own `package.json` and lockfile untouched; board-only dependencies and
the vite config live in `.board/` (`package.json`, `pnpm-workspace.yaml`, `vite.config.ts`). See
`import.md`.

## Static files

Files in `.board/public/` are served at the root of the frame (dev and preview builds), next to
the app's own `public/`. The MSW worker comes from the `msw` package unless the app ships its own
`mockServiceWorker.js`. Previews are served under a sub path, so string literals naming a public
file (`"/logo.png"`) are rewritten to `import.meta.env.BASE_URL` at build time.

## frame.tsx

```tsx
import { startFrame } from "@board/sdk/frame";
import { createMemoryRouter, RouterProvider } from "react-router";
import { routes } from "@/routes";
import "@/index.css";
import project from "./project.tsx";

void startFrame({
  project,
  renderScreen: (_screen, route) => (
    <RouterProvider router={createMemoryRouter(routes, { initialEntries: [route] })} />
  ),
  wrapStory: (node) => <div className="p-6">{node}</div>,
});
```

Wrap screens in whatever providers the app needs (query client, theme, auth stubs). The frame
reads `?screen=` / `?story=` and `?theme=dark` (it sets the `dark` class on `<html>`).

## board.json

Written by `board import` at the repository root: where the project came from, the app folder
(`app`), the workspace packages it uses and the root files kept. Paths in captures and comment
sources are relative to the app folder.
