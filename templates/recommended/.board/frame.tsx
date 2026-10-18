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
