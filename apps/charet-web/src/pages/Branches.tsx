import { GitBranch, TriangleAlert } from "lucide-react";
import { useState } from "react";
import { Link, useParams } from "react-router";
import { Badge } from "@/components/ui/badge";
import { Segmented } from "@/components/ui/segmented";
import type { Branch } from "@/lib/api";
import { useLocale, useT } from "@/lib/i18n";
import { useBranches } from "@/lib/queries";
import { Empty } from "./Screens";

export const STATUS = {
  open: ["web.branches.status.open", "secondary"],
  in_review: ["web.branches.status.in_review", "info"],
  merged: ["web.branches.status.merged", "default"],
  archived: ["web.branches.status.archived", "outline"],
} as const;

export function StatusBadge({ status }: { status: Branch["status"] }) {
  const t = useT();
  const [label, variant] = STATUS[status];
  return <Badge variant={variant}>{t(label)}</Badge>;
}

export function Branches() {
  const t = useT();
  const { locale } = useLocale();
  const key = useParams().key ?? "";
  const branches = useBranches(key);
  const [show, setShow] = useState<"active" | "all">("active");
  if (!branches.data) return <Empty text={t("web.common.loading")} />;
  const shown = branches.data.branches.filter(
    (b) => show === "all" || b.status === "open" || b.status === "in_review",
  );
  return (
    <div className="flex-1 overflow-auto p-6">
      <div className="mb-4 flex max-w-3xl items-center gap-3">
        <h1 className="text-lg font-semibold">{t("web.branches.title")}</h1>
        <span className="flex-1" />
        <Segmented
          label={t("web.branches.show")}
          value={show}
          onChange={setShow}
          options={[
            { value: "active", label: t("web.branches.active") },
            { value: "all", label: t("web.branches.all") },
          ]}
        />
      </div>
      {shown.length === 0 ? (
        <p className="max-w-3xl rounded-lg border border-dashed p-6 text-sm text-muted-foreground">
          {t("web.branches.none")}
        </p>
      ) : (
        <ol className="flex max-w-3xl flex-col divide-y rounded-lg border">
          {shown.map((branch) => (
            <li key={branch.number}>
              <Link
                to={`/p/${key}/branches/${branch.number}`}
                className="flex items-center gap-3 px-4 py-3 text-sm hover:bg-accent"
              >
                <GitBranch className="size-4 shrink-0 text-muted-foreground" />
                <span className="w-10 font-semibold tabular-nums">#{branch.number}</span>
                <span className="min-w-0 flex-1 truncate">{branch.title}</span>
                {branch.conflicts.length > 0 && (
                  <Badge variant="warning">
                    <TriangleAlert />
                    {t("web.branches.clash")}
                  </Badge>
                )}
                <StatusBadge status={branch.status} />
                <span className="text-muted-foreground">{branch.author.name}</span>
                <span className="text-xs text-muted-foreground">
                  {when(branch.updatedAt, locale)}
                </span>
              </Link>
            </li>
          ))}
        </ol>
      )}
      <p className="mt-4 max-w-3xl text-xs text-muted-foreground">{t("web.branches.how")}</p>
    </div>
  );
}

export function when(at: string, locale: string): string {
  const date = new Date(at);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleString(locale === "ko" ? "ko-KR" : "en-US", {
    dateStyle: "medium",
    timeStyle: "short",
  });
}
