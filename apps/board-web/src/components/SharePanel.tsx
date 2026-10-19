import { Check, Copy, Link2, Trash2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Segmented } from "@/components/ui/segmented";
import { useT } from "@/lib/i18n";
import { useMe, useShareLinkActions, useShareLinks } from "@/lib/queries";

type LinkRole = "editor" | "commenter" | "viewer";

/** The admin's Share button: create share links into this project and revoke them. */
export function SharePanel({ projectKey }: { projectKey: string }) {
  const t = useT();
  const me = useMe();
  const [open, setOpen] = useState(false);
  const panel = useRef<HTMLDivElement>(null);
  const admin = me.data?.role === "admin" && !me.data.localMode;
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
        <Link2 />
        {t("web.links.share")}
      </Button>
      {open && <Links projectKey={projectKey} />}
    </div>
  );
}

function Links({ projectKey }: { projectKey: string }) {
  const t = useT();
  const links = useShareLinks(projectKey, true);
  const { create, revoke } = useShareLinkActions(projectKey);
  const [role, setRole] = useState<LinkRole>("commenter");
  const [label, setLabel] = useState("");
  const [copied, setCopied] = useState(false);
  const url = create.data?.url;
  const copy = async () => {
    if (!url) return;
    await navigator.clipboard?.writeText(url).catch(() => undefined);
    setCopied(true);
  };
  return (
    <section
      aria-label={t("web.links.title")}
      className="absolute right-0 top-10 z-50 flex w-96 flex-col gap-3 rounded-xl border bg-card p-4 text-card-foreground shadow-lg"
    >
      <h2 className="text-sm font-semibold">{t("web.links.title")}</h2>
      <p className="text-xs text-muted-foreground">{t("web.links.lead")}</p>
      <Segmented
        label={t("web.links.role")}
        value={role}
        onChange={setRole}
        options={(["viewer", "commenter", "editor"] as const).map((value) => ({
          value,
          label: t(`web.links.role.${value}`),
        }))}
      />
      <form
        className="flex gap-2"
        onSubmit={(event) => {
          event.preventDefault();
          setCopied(false);
          create.mutate({ role, label }, { onSuccess: () => setLabel("") });
        }}
      >
        <Input
          value={label}
          onChange={(event) => setLabel(event.target.value)}
          placeholder={t("web.links.labelPlaceholder")}
          aria-label={t("web.links.label")}
        />
        <Button type="submit" disabled={create.isPending}>
          {t("web.links.create")}
        </Button>
      </form>
      {url && (
        <div className="flex flex-col gap-1">
          <div className="flex gap-2">
            <Input readOnly value={url} aria-label={t("web.links.address")} />
            <Button variant="outline" size="icon" onClick={copy} aria-label={t("web.links.copy")}>
              {copied ? <Check /> : <Copy />}
            </Button>
          </div>
          <p className="text-xs text-warning">{t("web.links.once")}</p>
        </div>
      )}
      {create.error && <p className="text-xs text-destructive">{create.error.message}</p>}
      <ul className="flex max-h-64 flex-col divide-y overflow-auto rounded-lg border">
        {(links.data?.links ?? []).map((link) => (
          <li
            key={link.id}
            className={`flex items-center gap-2 p-2 text-sm ${link.revokedAt ? "opacity-50" : ""}`}
          >
            <Badge variant="secondary">{t(`web.links.role.${link.role as LinkRole}`)}</Badge>
            <span className="min-w-0 flex-1 truncate">
              {link.label || link.createdAt.slice(0, 10)}
            </span>
            <span className="text-xs text-muted-foreground">
              {t("web.links.guests", { count: String(link.guests) })}
            </span>
            {link.revokedAt ? (
              <span className="text-xs text-muted-foreground">{t("web.links.revoked")}</span>
            ) : (
              <Button
                size="icon-sm"
                variant="ghost"
                onClick={() => revoke.mutate(link.id)}
                aria-label={t("web.links.revoke", { label: link.label || link.id })}
                title={t("web.links.revoke", { label: link.label || link.id })}
              >
                <Trash2 />
              </Button>
            )}
          </li>
        ))}
        {links.data?.links.length === 0 && (
          <li className="p-2 text-xs text-muted-foreground">{t("web.links.none")}</li>
        )}
      </ul>
    </section>
  );
}
