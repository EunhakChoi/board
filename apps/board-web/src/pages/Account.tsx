import { Check, Copy, Trash2 } from "lucide-react";
import { useState } from "react";
import { AppHeader } from "@/components/AppHeader";
import { CopyOnce } from "@/components/CopyOnce";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useLocale, useT } from "@/lib/i18n";
import { useCliTokenActions, useCliTokens, useMe, useOwnSignInLink } from "@/lib/queries";

/** `/account`: who you are and the CLI tokens your coding agents use. */
export function Account() {
  const t = useT();
  const { locale } = useLocale();
  const me = useMe();
  const tokens = useCliTokens();
  const { create, revoke } = useCliTokenActions();
  const [name, setName] = useState("");
  const [copied, setCopied] = useState(false);
  const when = (iso: string) =>
    new Date(iso).toLocaleString(locale === "ko" ? "ko-KR" : "en-US", {
      dateStyle: "medium",
      timeStyle: "short",
    });
  const secret = create.data?.token;
  return (
    <div className="flex min-h-svh flex-col">
      <AppHeader />
      <main className="mx-auto flex w-full max-w-3xl flex-1 flex-col gap-8 p-6">
        <div>
          <h1 className="text-2xl font-semibold">{me.data?.name}</h1>
          <p className="text-sm text-muted-foreground">
            {[me.data?.email, me.data && t(`web.team.role.${me.data.role}`)]
              .filter(Boolean)
              .join(" · ")}
          </p>
        </div>
        <OtherDevice />
        <section aria-label={t("web.account.tokens")} className="flex flex-col gap-3">
          <h2 className="text-sm font-medium text-muted-foreground">{t("web.account.tokens")}</h2>
          <p className="text-sm text-muted-foreground">
            {t("web.account.tokensLead")} <code className="rounded bg-muted px-1">board login</code>
          </p>
          <form
            className="flex gap-2"
            onSubmit={(event) => {
              event.preventDefault();
              setCopied(false);
              create.mutate(name, { onSuccess: () => setName("") });
            }}
          >
            <Input
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder={t("web.account.tokenName")}
              aria-label={t("web.account.tokenName")}
            />
            <Button type="submit" disabled={create.isPending}>
              {t("web.account.create")}
            </Button>
          </form>
          {secret && (
            <div className="flex flex-col gap-1">
              <div className="flex gap-2">
                <Input readOnly value={secret} aria-label={t("web.account.secret")} />
                <Button
                  variant="outline"
                  size="icon"
                  aria-label={t("web.links.copy")}
                  onClick={async () => {
                    await navigator.clipboard?.writeText(secret).catch(() => undefined);
                    setCopied(true);
                  }}
                >
                  {copied ? <Check /> : <Copy />}
                </Button>
              </div>
              <p className="text-xs text-warning">{t("web.account.once")}</p>
            </div>
          )}
          <ul className="divide-y rounded-xl border">
            {(tokens.data?.tokens ?? []).map((token) => (
              <li key={token.id} className="flex items-center gap-3 p-3 text-sm">
                <span className="min-w-0 flex-1 truncate font-medium">{token.name}</span>
                <Badge variant="secondary">{t(`web.account.via.${token.via}`)}</Badge>
                <span className="text-xs text-muted-foreground">
                  {token.lastUsedAt
                    ? t("web.account.used", { when: when(token.lastUsedAt) })
                    : t("web.account.unused")}
                </span>
                <Button
                  size="icon-sm"
                  variant="ghost"
                  onClick={() => revoke.mutate(token.id)}
                  aria-label={t("web.account.revoke", { name: token.name })}
                  title={t("web.account.revoke", { name: token.name })}
                >
                  <Trash2 />
                </Button>
              </li>
            ))}
            {tokens.data?.tokens.length === 0 && (
              <li className="p-3 text-sm text-muted-foreground">{t("web.account.none")}</li>
            )}
          </ul>
        </section>
      </main>
    </div>
  );
}

/** A 15-minute link that signs this account in on a phone or another computer. */
function OtherDevice() {
  const t = useT();
  const link = useOwnSignInLink();
  return (
    <section aria-label={t("web.account.device")} className="flex flex-col gap-3">
      <h2 className="text-sm font-medium text-muted-foreground">{t("web.account.device")}</h2>
      <p className="text-sm text-muted-foreground">{t("web.account.deviceLead")}</p>
      <div>
        <Button variant="outline" onClick={() => link.mutate()} disabled={link.isPending}>
          {t("web.account.deviceCreate")}
        </Button>
      </div>
      {link.error && <p className="text-sm text-destructive">{link.error.message}</p>}
      {link.data && (
        <CopyOnce
          value={link.data.url}
          label={t("web.invites.address")}
          note={t("web.account.deviceOnce")}
        />
      )}
    </section>
  );
}
