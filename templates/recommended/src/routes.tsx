import type { RouteObject } from "react-router";
import { TasksPage } from "@/pages/tasks";

export const routes: RouteObject[] = [{ path: "/", element: <TasksPage /> }];
