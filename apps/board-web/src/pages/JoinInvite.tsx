import type { Key } from "@board/i18n";
import { useState } from "react";
import { useParams } from "react-router";
import { AuthCard } from "@/components/AuthCard";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useT } from "@/lib/i18n";
import { useAcceptInvite, useInvite } from "@/lib/queries";

/** `/i/:token`: a new member joins under a name, or a member signs in on this device. */
export function JoinInvite() {
  const t = useT();
  const token = useParams().token ?? "";
  const invite = useInvite(token);
  const accept = useAcceptInvite(token);
  const [name, setName] = useState("");
  if (invite.isPending) return null;
  if (invite.error) {
    return (
      <AuthCard title={t("web.invites.title")}>
        <p className="text-sm text-destructive">{t("web.invites.gone")}</p>
      </AuthCard>
    );
  }
  // A fresh page load picks up the new session everywhere.
  const done = { onSuccess: () => window.location.assign("/") };
  if (invite.data.kind === "sign-in") {
    return (
      <AuthCard title={t("web.invites.signInTitle")}>
        <p className="text-sm text-muted-foreground">
          {t("web.invites.signInLead", { name: invite.data.name ?? "" })}
        </p>
        {accept.error && <p className="text-sm text-destructive">{accept.error.message}</p>}
        <Button onClick={() => accept.mutate("", done)} disabled={accept.isPending}>
          {t("web.invites.signIn", { name: invite.data.name ?? "" })}
        </Button>
      </AuthCard>
    );
  }
  return (
    <AuthCard title={t("web.invites.title")}>
      <p className="text-sm text-muted-foreground">
        {t("web.invites.joinLead", { role: t(`web.team.role.${invite.data.role}` as Key) })}
      </p>
      <form
        className="flex flex-col gap-3"
        onSubmit={(event) => {
          event.preventDefault();
          accept.mutate(name, done);
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
        {accept.error && <p className="text-sm text-destructive">{accept.error.message}</p>}
        <Button type="submit" disabled={!name.trim() || accept.isPending}>
          {t("web.invites.join")}
        </Button>
      </form>
    </AuthCard>
  );
}
