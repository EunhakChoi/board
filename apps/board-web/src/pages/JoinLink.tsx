import type { Key } from "@board/i18n";
import { useState } from "react";
import { useNavigate, useParams } from "react-router";
import { AuthCard } from "@/components/AuthCard";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useT } from "@/lib/i18n";
import { useEnterLink, useSharedLink } from "@/lib/queries";

/** `/l/:token`: someone without an account comes in through a share link under a name. */
export function JoinLink() {
  const t = useT();
  const token = useParams().token ?? "";
  const navigate = useNavigate();
  const shared = useSharedLink(token);
  const enter = useEnterLink(token);
  const [name, setName] = useState("");
  if (shared.isPending) return null;
  if (shared.error) {
    return (
      <AuthCard title={t("web.links.joinTitle")}>
        <p className="text-sm text-destructive">{t("web.links.gone")}</p>
      </AuthCard>
    );
  }
  const link = shared.data;
  return (
    <AuthCard title={link.projectTitle}>
      <p className="text-sm text-muted-foreground">
        {t("web.links.joinLead", { role: t(`web.links.role.${link.role}` as Key) })}
      </p>
      <form
        className="flex flex-col gap-3"
        onSubmit={(event) => {
          event.preventDefault();
          enter.mutate(name, {
            onSuccess: ({ project }) => {
              // A fresh page load picks up the new session everywhere.
              window.location.assign(`/p/${project}/screens`);
            },
          });
        }}
      >
        <Input
          autoFocus
          value={name}
          maxLength={40}
          onChange={(event) => setName(event.target.value)}
          placeholder={t("web.links.namePlaceholder")}
          aria-label={t("web.links.name")}
        />
        {enter.error && <p className="text-sm text-destructive">{enter.error.message}</p>}
        <Button type="submit" disabled={!name.trim() || enter.isPending}>
          {t("web.links.join")}
        </Button>
      </form>
      <button
        type="button"
        className="text-xs text-muted-foreground underline"
        onClick={() => navigate("/signin")}
      >
        {t("web.links.haveAccount")}
      </button>
    </AuthCard>
  );
}
