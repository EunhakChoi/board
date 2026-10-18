import { HttpResponse, http } from "msw";
import type { Task } from "@/api";

export const sampleTasks: Task[] = [
  { id: 1, title: "Write the release notes", done: false },
  { id: 2, title: "Review the onboarding flow", done: true },
  { id: 3, title: "Fix the login redirect", done: false },
];

export const tasks = (list: Task[]) => http.get("/api/tasks", () => HttpResponse.json(list));
