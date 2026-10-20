import { Bell, CheckCheck } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router";
import { Button } from "@/components/ui/button";
import type { Notification } from "@/lib/api";
import { useT } from "@/lib/i18n";
import { useInbox, useMarkRead, useMe } from "@/lib/queries";
import { cn } from "@/lib/utils";

/** The bell: new comments, answers in your threads and (for admins) access requests. */
export function Inbox() {
  const t = useT();
  const me = useMe();
  const member = me.data?.kind === "user";
  const inbox = useInbox(member);
  const markRead = useMarkRead();
  const navigate = useNavigate();
  const [open, setOpen] = useState(false);
  const panel = useRef<HTMLDivElement>(null);
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
  if (!member) return null;
  const unread = inbox.data?.unread ?? 0;
  const items = inbox.data?.notifications ?? [];
  const go = (n: Notification) => {
    if (!n.readAt) markRead.mutate([n.id]);
    setOpen(false);
    navigate(n.link);
  };
  return (
    <div className="relative" ref={panel}>
      <Button
        variant="ghost"
        size="icon-sm"
        onClick={() => setOpen(!open)}
        aria-expanded={open}
        aria-label={t("web.inbox.open", { count: unread })}
        title={t("web.inbox.title")}
        className="relative"
      >
        <Bell />
        {unread > 0 && (
          <span className="absolute -top-0.5 -right-0.5 flex size-4 items-center justify-center rounded-full bg-primary text-xs leading-none text-primary-foreground">
            {unread > 9 ? "9+" : unread}
          </span>
        )}
      </Button>
      {open && (
        <section
          aria-label={t("web.inbox.title")}
          className="absolute right-0 top-10 z-50 flex max-h-96 w-96 flex-col overflow-hidden rounded-xl border bg-card text-card-foreground shadow-lg"
        >
          <div className="flex items-center justify-between border-b px-3 py-2">
            <h2 className="text-sm font-semibold">{t("web.inbox.title")}</h2>
            <Button
              size="sm"
              variant="ghost"
              disabled={unread === 0 || markRead.isPending}
              onClick={() => markRead.mutate(undefined)}
            >
              <CheckCheck />
              {t("web.inbox.markAll")}
            </Button>
          </div>
          {items.length === 0 ? (
            <p className="p-4 text-sm text-muted-foreground">{t("web.inbox.empty")}</p>
          ) : (
            <ul className="divide-y overflow-auto">
              {items.map((n) => (
                <li key={n.id}>
                  <button
                    type="button"
                    onClick={() => go(n)}
                    className={cn(
                      "flex w-full flex-col gap-0.5 px-3 py-2 text-left text-sm hover:bg-muted",
                      !n.readAt && "bg-primary/5",
                    )}
                  >
                    <span className="flex items-center gap-2">
                      {!n.readAt && <span className="size-2 shrink-0 rounded-full bg-primary" />}
                      <span className="truncate">{describe(t, n)}</span>
                    </span>
                    {typeof n.data.body === "string" && n.data.body && (
                      <span className="truncate text-xs text-muted-foreground">{n.data.body}</span>
                    )}
                    <span className="text-xs text-muted-foreground">
                      {[n.project, n.createdAt.slice(0, 16).replace("T", " ")]
                        .filter(Boolean)
                        .join(" · ")}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>
      )}
    </div>
  );
}

function describe(t: ReturnType<typeof useT>, n: Notification): string {
  const target = typeof n.data.target === "string" ? n.data.target : "";
  switch (n.kind) {
    case "comment.added":
      return t("web.inbox.added", { actor: n.actor, target });
    case "comment.replied":
      return t("web.inbox.replied", { actor: n.actor, target });
    case "comment.resolved":
      return t("web.inbox.resolved", { actor: n.actor, target });
    case "comment.reopened":
      return t("web.inbox.reopened", { actor: n.actor, target });
    default:
      return n.text;
  }
}
