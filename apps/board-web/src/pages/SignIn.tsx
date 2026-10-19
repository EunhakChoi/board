import type { Key } from "@board/i18n";
import { useSearchParams } from "react-router";
import { AuthCard, signInUrl } from "@/components/AuthCard";
import { Button } from "@/components/ui/button";
import { useT } from "@/lib/i18n";

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
  return (
    <AuthCard title={t("web.auth.signIn")}>
      <p className="text-sm text-muted-foreground">{t("web.auth.signInLead")}</p>
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {t((ERRORS.has(error) ? `web.auth.error.${error}` : "web.auth.error.failed") as Key)}
        </p>
      )}
      <Button asChild>
        <a href={signInUrl({ returnTo })}>{t("web.auth.signInWith")}</a>
      </Button>
    </AuthCard>
  );
}

/** `/setup?token=…`: the one-time link printed by the server; its first user becomes the admin. */
export function Setup() {
  const t = useT();
  const [params] = useSearchParams();
  const token = params.get("token");
  return (
    <AuthCard title={t("web.auth.setupTitle")}>
      {token ? (
        <>
          <p className="text-sm text-muted-foreground">{t("web.auth.setupLead")}</p>
          <Button asChild>
            <a href={signInUrl({ setup: token })}>{t("web.auth.signInWith")}</a>
          </Button>
        </>
      ) : (
        <p className="text-sm text-destructive">{t("web.auth.setupMissing")}</p>
      )}
    </AuthCard>
  );
}
