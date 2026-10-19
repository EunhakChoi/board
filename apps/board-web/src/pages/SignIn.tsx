import type { Key } from "@board/i18n";
import { useState } from "react";
import { useSearchParams } from "react-router";
import { AuthCard, signInUrl } from "@/components/AuthCard";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useT } from "@/lib/i18n";
import { useFinishSetup, useSystem } from "@/lib/queries";

const ERRORS = new Set([
  "setup_required",
  "email_unverified",
  "rejected",
  "removed",
  "cancelled",
  "expired",
  "unavailable",
  "failed",
]);

/** `/signin`: the way in, with the reason when the last attempt did not work. */
export function SignIn() {
  const t = useT();
  const [params] = useSearchParams();
  const error = params.get("error");
  const returnTo = params.get("returnTo") ?? undefined;
  const provider = useSystem().data?.signIn?.provider;
  return (
    <AuthCard title={t("web.auth.signIn")}>
      <p className="text-sm text-muted-foreground">
        {t(provider ? "web.auth.signInLead" : "web.auth.inviteOnly")}
      </p>
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {t((ERRORS.has(error) ? `web.auth.error.${error}` : "web.auth.error.failed") as Key)}
        </p>
      )}
      {provider && (
        <Button asChild>
          <a href={signInUrl({ returnTo })}>{t("web.auth.signInWith")}</a>
        </Button>
      )}
    </AuthCard>
  );
}

/** `/setup?token=…`: the one-time link printed by the server; its first user becomes the admin. */
export function Setup() {
  const t = useT();
  const [params] = useSearchParams();
  const token = params.get("token");
  const provider = useSystem().data?.signIn?.provider;
  const finish = useFinishSetup();
  const [name, setName] = useState("");
  return (
    <AuthCard title={t("web.auth.setupTitle")}>
      {token ? (
        <>
          <p className="text-sm text-muted-foreground">{t("web.auth.setupByName")}</p>
          <form
            className="flex flex-col gap-3"
            onSubmit={(event) => {
              event.preventDefault();
              finish.mutate(
                { token, name },
                // A fresh page load picks up the new session everywhere.
                { onSuccess: () => window.location.assign("/team") },
              );
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
            {finish.error && <p className="text-sm text-destructive">{finish.error.message}</p>}
            <Button type="submit" disabled={!name.trim() || finish.isPending}>
              {t("web.auth.setupContinue")}
            </Button>
          </form>
          {provider && (
            <Button variant="outline" asChild>
              <a href={signInUrl({ setup: token })}>{t("web.auth.signInWith")}</a>
            </Button>
          )}
        </>
      ) : (
        <p className="text-sm text-destructive">{t("web.auth.setupMissing")}</p>
      )}
    </AuthCard>
  );
}
