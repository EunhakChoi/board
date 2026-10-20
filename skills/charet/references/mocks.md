# Mocks

Screens render with [MSW](https://mswjs.io) handlers instead of a real backend. A screen's own
`mocks` are tried first, then the project's `mocks`. Any request no handler answers is reported by
`charet check` as `[charet] missing mock METHOD /path` — add a handler rather than ignoring it.

```ts
// .charet/mocks.ts
import { delay, HttpResponse, http } from "msw";
import type { Task } from "@/api";

export const sampleTasks: Task[] = [
  { id: 1, title: "Write the release notes", done: false },
];

// A factory per endpoint keeps screens short: tasks([]) for the empty state, etc.
export const tasks = (list: Task[]) => http.get("/api/tasks", () => HttpResponse.json(list));

export const failing = (path: string, status = 500) =>
  http.get(path, () => new HttpResponse(null, { status }));

export const slow = (path: string) =>
  http.get(path, async () => {
    await delay("infinite"); // a loading state that never resolves
    return HttpResponse.json({});
  });
```

Tips:

- Type mock data with the app's own API types (e.g. from a generated client) so mocks break when
  the API changes.
- Keep one factory per endpoint and vary the data per screen; avoid copying handlers.
- Error and empty states are screens too (`tasks-error`, `tasks-empty`): give them their own ids.
- Requests the browser makes on its own (fonts, images from the app's `public/`) are served from
  the build and need no mock.
