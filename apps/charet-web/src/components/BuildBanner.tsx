import { CircleAlert, LoaderCircle, RotateCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { HttpError } from "@/lib/api";
import { useT } from "@/lib/i18n";
import { usePreview, useRetryBuild } from "@/lib/queries";

/** Shows the preview build while it runs or after it failed; nothing once it is ready. */
export function BuildBanner({ projectKey }: { projectKey: string }) {
  const t = useT();
  const preview = usePreview(projectKey);
  const retry = useRetryBuild(projectKey);
  if (preview.error instanceof HttpError && preview.error.status === 503) {
    return <Banner tone="error">{t("web.build.workspaceDown")}</Banner>;
  }
  const state = preview.data?.state;
  if (state === "queued" || state === "running") {
    const steps = preview.data?.steps.map((s) => s.name).join(" → ");
    return (
      <Banner tone="info">
        <LoaderCircle className="size-4 animate-spin" />
        {t(state === "queued" ? "web.build.queued" : "web.build.running")}
        {steps && <span className="text-muted-foreground">{steps}</span>}
      </Banner>
    );
  }
  if (state === "failed") {
    const failed = preview.data?.steps.find((s) => s.state === "failed");
    return (
      <Banner tone="error">
        <CircleAlert className="size-4" />
        <span className="flex-1">
          {t("web.build.failed", { error: preview.data?.error ?? "" })}
        </span>
        <Button
          size="sm"
          variant="outline"
          onClick={() => retry.mutate()}
          disabled={retry.isPending}
        >
          <RotateCw />
          {t("web.build.retry")}
        </Button>
        {failed?.detail && (
          <pre className="mt-2 max-h-40 w-full overflow-auto rounded-md bg-muted p-2 text-xs">
            {failed.detail}
          </pre>
        )}
      </Banner>
    );
  }
  return null;
}

function Banner({ tone, children }: { tone: "info" | "error"; children: React.ReactNode }) {
  return (
    <div
      role="status"
      className={
        tone === "error"
          ? "flex flex-wrap items-center gap-2 border-b bg-destructive/10 px-4 py-2 text-sm text-destructive"
          : "flex flex-wrap items-center gap-2 border-b bg-info/10 px-4 py-2 text-sm"
      }
    >
      {children}
    </div>
  );
}
