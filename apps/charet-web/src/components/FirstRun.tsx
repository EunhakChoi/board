import { KeyRound } from "lucide-react";
import { useT } from "@/lib/i18n";
import { useSystem } from "@/lib/queries";

/**
 * Right after install, at this machine: says where the admin link is. The link itself never
 * shows here; it is in the terminal of whoever started the server.
 */
export function FirstRun() {
  const t = useT();
  const system = useSystem();
  if (!system.data?.firstRun) return null;
  return (
    <aside
      aria-label={t("web.firstRun.title")}
      className="border-b border-warning/30 bg-warning/10 px-4 py-3 text-sm text-foreground"
    >
      <div className="mx-auto flex max-w-5xl items-start gap-3">
        <KeyRound className="mt-0.5 size-4 shrink-0 text-warning" aria-hidden />
        <div className="flex flex-col gap-1">
          <p className="font-medium">{t("web.firstRun.title")}</p>
          <p>{t("web.firstRun.body")}</p>
          <p className="flex flex-wrap gap-2">
            <code className="rounded bg-muted px-1.5 py-0.5">./start.sh</code>
            <code className="rounded bg-muted px-1.5 py-0.5">charet setup</code>
          </p>
        </div>
      </div>
    </aside>
  );
}
