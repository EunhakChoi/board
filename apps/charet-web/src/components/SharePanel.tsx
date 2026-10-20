import { Lock } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Segmented } from "@/components/ui/segmented";
import { useT } from "@/lib/i18n";
import {
  useAccess,
  useMe,
  useProject,
  useProjectAccess,
  useProjectAccessActions,
  useUsers,
} from "@/lib/queries";

/** The admin's button for who sees this project: the whole team, or picked members. */
export function SharePanel({ projectKey }: { projectKey: string }) {
  const t = useT();
  const me = useMe();
  const [open, setOpen] = useState(false);
  const panel = useRef<HTMLDivElement>(null);
  const admin = me.data?.role === "admin";
  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      if (!panel.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);
  if (!admin) return null;
  return (
    <div className="relative" ref={panel}>
      <Button size="sm" variant="outline" onClick={() => setOpen(!open)} aria-expanded={open}>
        <Lock />
        {t("web.access.button")}
      </Button>
      {open && (
        <section
          aria-label={t("web.access.title")}
          className="absolute right-0 top-10 z-50 flex max-h-svh w-96 flex-col gap-5 overflow-auto rounded-xl border bg-card p-4 text-card-foreground shadow-lg"
        >
          <Access projectKey={projectKey} />
        </section>
      )}
    </div>
  );
}

/** Who sees the project: the whole team, or only the members picked here (and admins). */
function Access({ projectKey }: { projectKey: string }) {
  const t = useT();
  const project = useProject(projectKey);
  const isPrivate = project.data?.visibility === "private";
  const users = useUsers();
  const members = useProjectAccess(projectKey, isPrivate);
  const { visibility, member, visitors } = useProjectAccessActions(projectKey);
  const server = useAccess(true).data?.visitors;
  const level = (value: string | undefined) =>
    value === "none"
      ? t("web.team.visitorsOff")
      : value === "comment"
        ? t("web.team.visitorsComment")
        : t("web.team.visitorsOn");
  const chosen = new Set(members.data?.members ?? []);
  const people = (users.data?.users ?? []).filter((u) => u.role !== "admin");
  return (
    <section aria-label={t("web.access.title")} className="flex flex-col gap-3">
      <h2 className="text-sm font-semibold">{t("web.access.title")}</h2>
      <Segmented
        label={t("web.access.title")}
        value={isPrivate ? "private" : "team"}
        onChange={(value) => visibility.mutate(value)}
        options={[
          { value: "team", label: t("web.access.team") },
          { value: "private", label: t("web.access.private") },
        ]}
      />
      <p className="text-xs text-muted-foreground">
        {t(isPrivate ? "web.access.privateLead" : "web.access.teamLead")}
      </p>
      {isPrivate && (
        <ul className="flex max-h-48 flex-col divide-y overflow-auto rounded-lg border">
          {people.map((user) => (
            <li key={user.id}>
              <label className="flex cursor-pointer items-center gap-2 p-2 text-sm">
                <input
                  type="checkbox"
                  checked={chosen.has(user.id)}
                  onChange={(event) =>
                    member.mutate({ user: user.id, member: event.target.checked })
                  }
                />
                <span className="min-w-0 flex-1 truncate">{user.name}</span>
              </label>
            </li>
          ))}
          {people.length === 0 && (
            <li className="p-2 text-xs text-muted-foreground">{t("web.access.nobody")}</li>
          )}
        </ul>
      )}
      {!isPrivate && project.data && (
        <section aria-label={t("web.access.visitors")} className="flex flex-col gap-2">
          <h3 className="text-sm font-semibold">{t("web.access.visitors")}</h3>
          <Segmented
            label={t("web.access.visitors")}
            value={project.data.visitors ?? "server"}
            onChange={(value) => visitors.mutate(value === "server" ? null : value)}
            options={[
              { value: "server", label: t("web.access.visitorsServer") },
              { value: "none", label: t("web.team.visitorsOff") },
              { value: "view", label: t("web.team.visitorsOn") },
              { value: "comment", label: t("web.team.visitorsComment") },
            ]}
          />
          <p className="text-xs text-muted-foreground">
            {project.data.visitors === null
              ? t("web.access.visitorsFollows", { level: level(server) })
              : t("web.access.visitorsOwn")}
          </p>
          {visitors.error && <p className="text-sm text-destructive">{visitors.error.message}</p>}
        </section>
      )}
    </section>
  );
}
