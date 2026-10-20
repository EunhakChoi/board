import { GitBranch, Undo2 } from "lucide-react";
import { useState } from "react";
import { Link, useParams } from "react-router";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { useLocale, useT } from "@/lib/i18n";
import { useBranchActions, useVersions } from "@/lib/queries";
import { useCan } from "@/lib/role";
import { when } from "./Branches";
import { Empty } from "./Screens";

const KIND = {
  initial: "web.versions.kind.initial",
  import: "web.versions.kind.import",
  push: "web.versions.kind.push",
  merge: "web.versions.kind.merge",
  restore: "web.versions.kind.restore",
} as const;

export function Versions() {
  const t = useT();
  const { locale } = useLocale();
  const key = useParams().key ?? "";
  const versions = useVersions(key);
  const canEdit = useCan("editor");
  const { restore } = useBranchActions(key);
  // Restoring asks once more on the same row before it runs.
  const [asking, setAsking] = useState<number>();
  if (!versions.data) return <Empty text={t("web.common.loading")} />;
  return (
    <div className="flex-1 overflow-auto p-6">
      <h1 className="mb-4 text-lg font-semibold">{t("web.versions.title")}</h1>
      {restore.error && (
        <p className="mb-3 max-w-3xl text-sm text-destructive">
          {t("web.common.error", { message: restore.error.message })}
        </p>
      )}
      <ol className="flex max-w-3xl flex-col divide-y rounded-lg border">
        {versions.data.versions.map((version) => (
          <li key={version.number} className="flex items-center gap-3 px-4 py-3 text-sm">
            <span className="w-12 font-semibold tabular-nums">v{version.number}</span>
            <Badge variant="secondary">{t(KIND[version.kind])}</Badge>
            <span className="min-w-0 flex-1 truncate">
              {version.note}
              {version.branch !== undefined && (
                <Link
                  to={`/p/${key}/branches/${version.branch}`}
                  className="ml-2 inline-flex items-center gap-1 text-xs text-muted-foreground underline"
                >
                  <GitBranch className="size-3" />#{version.branch}
                </Link>
              )}
            </span>
            <span className="text-muted-foreground">{version.actor}</span>
            <span className="font-mono text-xs text-muted-foreground">
              {version.commit.slice(0, 7)}
            </span>
            <span className="text-xs text-muted-foreground">{when(version.createdAt, locale)}</span>
            {canEdit &&
              (version.kind === "push" || version.kind === "merge" || version.kind === "restore") &&
              (asking === version.number ? (
                <Button
                  size="sm"
                  variant="destructive"
                  disabled={restore.isPending}
                  onClick={() =>
                    restore.mutate(version.number, { onSettled: () => setAsking(undefined) })
                  }
                >
                  {t("web.versions.restoreConfirm", { number: version.number })}
                </Button>
              ) : (
                <Button
                  size="icon-sm"
                  variant="ghost"
                  aria-label={t("web.versions.restore", { number: version.number })}
                  title={t("web.versions.restore", { number: version.number })}
                  onClick={() => setAsking(version.number)}
                >
                  <Undo2 />
                </Button>
              ))}
          </li>
        ))}
      </ol>
    </div>
  );
}
