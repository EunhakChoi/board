import { useParams } from "react-router";
import { Badge } from "@/components/ui/badge";
import { useLocale, useT } from "@/lib/i18n";
import { useVersions } from "@/lib/queries";
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
  if (!versions.data) return <Empty text={t("web.common.loading")} />;
  return (
    <div className="flex-1 overflow-auto p-6">
      <h1 className="mb-4 text-lg font-semibold">{t("web.versions.title")}</h1>
      <ol className="flex max-w-3xl flex-col divide-y rounded-lg border">
        {versions.data.versions.map((version) => (
          <li key={version.number} className="flex items-center gap-3 px-4 py-3 text-sm">
            <span className="w-12 font-semibold tabular-nums">v{version.number}</span>
            <Badge variant="secondary">{t(KIND[version.kind])}</Badge>
            <span className="min-w-0 flex-1 truncate">{version.note}</span>
            <span className="text-muted-foreground">{version.actor}</span>
            <span className="font-mono text-xs text-muted-foreground">
              {version.commit.slice(0, 7)}
            </span>
            <span className="text-xs text-muted-foreground">
              {new Date(version.createdAt).toLocaleString(locale === "ko" ? "ko-KR" : "en-US", {
                dateStyle: "medium",
                timeStyle: "short",
              })}
            </span>
          </li>
        ))}
      </ol>
    </div>
  );
}
