import { Camera, ChevronRight, LoaderCircle, Moon, Sun } from "lucide-react";
import { NavLink, Outlet, useParams, useSearchParams } from "react-router";
import { AppHeader } from "@/components/AppHeader";
import { BuildBanner } from "@/components/BuildBanner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Segmented } from "@/components/ui/segmented";
import { useT } from "@/lib/i18n";
import { ProjectContext, useProjectData } from "@/lib/project";
import { useCapture, useProject } from "@/lib/queries";
import { useScreenTheme } from "@/lib/theme";
import { cn } from "@/lib/utils";

const TABS = [
  ["screens", "web.nav.screens"],
  ["flows", "web.nav.flows"],
  ["components", "web.nav.components"],
  ["versions", "web.nav.versions"],
  ["code", "web.nav.code"],
] as const;

export function ProjectLayout() {
  const t = useT();
  const key = useParams().key ?? "";
  const [search] = useSearchParams();
  const project = useProject(key);
  const data = useProjectData(key);
  const [theme, setTheme] = useScreenTheme();
  const capture = useCapture(key);
  const keep = search.get("theme") ? `?theme=${search.get("theme")}` : "";
  return (
    <ProjectContext value={data}>
      <div className="flex h-svh flex-col">
        <AppHeader
          actions={
            <>
              {data.staleCount > 0 && (
                <Badge variant="warning">
                  {t("web.capture.stale", { count: data.staleCount })}
                </Badge>
              )}
              <Button
                size="sm"
                variant="outline"
                disabled={capture.isPending}
                onClick={() => capture.mutate({ themes: ["light", "dark"] })}
              >
                {capture.isPending ? <LoaderCircle className="animate-spin" /> : <Camera />}
                {t(capture.isPending ? "web.capture.running" : "web.capture.run")}
              </Button>
              <Segmented
                label={t("web.theme.toggle")}
                value={theme}
                onChange={setTheme}
                options={[
                  { value: "light", label: <Sun aria-label={t("web.theme.light")} /> },
                  { value: "dark", label: <Moon aria-label={t("web.theme.dark")} /> },
                ]}
              />
            </>
          }
        >
          <ChevronRight className="size-4 text-muted-foreground" />
          <span className="truncate font-medium">{project.data?.title ?? key}</span>
          <nav className="ml-4 flex items-center gap-1">
            {TABS.map(([tab, label]) => (
              <NavLink
                key={tab}
                to={`/p/${key}/${tab}${keep}`}
                className={({ isActive }) =>
                  cn(
                    "rounded-md px-2.5 py-1.5 text-sm text-muted-foreground transition-colors hover:text-foreground",
                    isActive && "bg-muted text-foreground",
                  )
                }
              >
                {t(label)}
              </NavLink>
            ))}
          </nav>
        </AppHeader>
        <BuildBanner projectKey={key} />
        {capture.error && (
          <p className="border-b bg-destructive/10 px-4 py-2 text-sm text-destructive">
            {t("web.common.error", { message: capture.error.message })}
          </p>
        )}
        <Outlet />
      </div>
    </ProjectContext>
  );
}
