import { defineProject } from "@board/sdk";
import { HttpResponse, http } from "msw";
import { Button } from "@/components/ui/button";
import { sampleTasks, tasks } from "./mocks.ts";

export default defineProject({
  title: "Tasks",
  pages: [{ id: "tasks", route: "/", title: "Tasks" }],
  screens: [
    { id: "tasks-list", page: "tasks", title: "Task list", mocks: [tasks(sampleTasks)] },
    { id: "tasks-empty", page: "tasks", title: "No tasks", mocks: [tasks([])] },
    {
      id: "tasks-open",
      page: "tasks",
      title: "Open tasks only",
      mocks: [tasks(sampleTasks)],
      setup: (kit) => kit.click("button", "Open"),
    },
    {
      id: "tasks-error",
      page: "tasks",
      title: "Load failed",
      mocks: [http.get("/api/tasks", () => new HttpResponse(null, { status: 500 }))],
    },
  ],
  connections: [{ from: "tasks-list", to: "tasks-open", label: "Open" }],
  flows: [
    {
      id: "triage",
      title: "Triage tasks",
      steps: [
        "tasks-list",
        "tasks-open",
        { id: "task-detail", title: "Task detail", note: "planned" },
      ],
    },
  ],
  stories: [
    {
      id: "button-default",
      component: "Button",
      title: "Default",
      render: () => <Button>Save</Button>,
    },
    {
      id: "button-outline",
      component: "Button",
      title: "Outline",
      render: () => <Button variant="outline">Cancel</Button>,
    },
  ],
});
