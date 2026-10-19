import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { createBrowserRouter, Navigate, RouterProvider } from "react-router";
import { AuthGate } from "@/components/AuthGate";
import { LocaleProvider } from "@/lib/i18n";
import { Code } from "@/pages/Code";
import { Components } from "@/pages/Components";
import { Flows } from "@/pages/Flows";
import { ProjectLayout } from "@/pages/ProjectLayout";
import { Projects } from "@/pages/Projects";
import { Screens } from "@/pages/Screens";
import { Setup, SignIn } from "@/pages/SignIn";
import { Target } from "@/pages/Target";
import { Team } from "@/pages/Team";
import { Versions } from "@/pages/Versions";
import "./index.css";

const router = createBrowserRouter([
  { path: "/signin", element: <SignIn /> },
  { path: "/setup", element: <Setup /> },
  {
    path: "/",
    element: (
      <AuthGate>
        <Projects />
      </AuthGate>
    ),
  },
  {
    path: "/team",
    element: (
      <AuthGate>
        <Team />
      </AuthGate>
    ),
  },
  {
    path: "/p/:key",
    element: (
      <AuthGate>
        <ProjectLayout />
      </AuthGate>
    ),
    children: [
      { index: true, element: <Navigate to="screens" replace /> },
      { path: "screens", element: <Screens /> },
      { path: "flows", element: <Flows /> },
      { path: "components", element: <Components /> },
      { path: "versions", element: <Versions /> },
      { path: "code", element: <Code /> },
      { path: "t/:kind/:target", element: <Target /> },
    ],
  },
]);

const queries = new QueryClient({
  defaultOptions: { queries: { staleTime: 5_000, refetchOnWindowFocus: true } },
});

const root = document.getElementById("root");
if (!root) throw new Error("no #root element");
createRoot(root).render(
  <StrictMode>
    <QueryClientProvider client={queries}>
      <LocaleProvider>
        <RouterProvider router={router} />
      </LocaleProvider>
    </QueryClientProvider>
  </StrictMode>,
);
