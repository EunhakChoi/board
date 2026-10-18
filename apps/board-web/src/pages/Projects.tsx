import { FolderOpen } from "lucide-react";
import { Link } from "react-router";
import { AppHeader } from "@/components/AppHeader";
import { Card, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { useT } from "@/lib/i18n";
import { useProjects } from "@/lib/queries";

export function Projects() {
  const t = useT();
  const projects = useProjects();
  const list = projects.data?.projects ?? [];
  return (
    <div className="flex min-h-svh flex-col">
      <AppHeader />
      <main className="mx-auto w-full max-w-5xl flex-1 p-6">
        <h1 className="mb-6 text-2xl font-semibold">{t("web.projects.title")}</h1>
        {projects.isLoading && (
          <p className="text-sm text-muted-foreground">{t("web.common.loading")}</p>
        )}
        {projects.error && (
          <p className="text-sm text-destructive">
            {t("web.common.error", { message: projects.error.message })}
          </p>
        )}
        {projects.isSuccess && list.length === 0 && (
          <div className="rounded-xl border border-dashed p-8 text-center">
            <FolderOpen className="mx-auto mb-3 size-8 text-muted-foreground" />
            <p className="mb-3 text-sm text-muted-foreground">{t("web.projects.empty")}</p>
            <code className="rounded-md bg-muted px-2 py-1 text-sm">board import path/to/app</code>
          </div>
        )}
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {list.map((project) => (
            <Link
              key={project.key}
              to={`/p/${project.key}/screens`}
              className="rounded-xl outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
            >
              <Card interactive>
                <CardHeader>
                  <CardTitle>{project.title}</CardTitle>
                  <CardDescription>{project.key}</CardDescription>
                </CardHeader>
              </Card>
            </Link>
          ))}
        </div>
      </main>
    </div>
  );
}
