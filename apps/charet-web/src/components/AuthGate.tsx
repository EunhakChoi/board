import { type ReactNode, useState } from "react";
import { AuthCard } from "@/components/AuthCard";
import { FirstRun } from "@/components/FirstRun";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { HttpError } from "@/lib/api";
import { useT } from "@/lib/i18n";
import { useEnterToken, useMe } from "@/lib/queries";

/** Shows the charet to members and visitors; asks for a token when the charet needs one. */
export function AuthGate({ children }: { children: ReactNode }) {
  const t = useT();
  const me = useMe();
  if (me.isPending) return null;
  if (me.error) {
    const error = me.error;
    if (error instanceof HttpError && error.status === 401) {
      return (
        <>
          <FirstRun />
          <TokenPage reason={error.code} />
        </>
      );
    }
    return (
      <AuthCard title={t("web.auth.tokenTitle")}>
        <p className="text-sm text-destructive">
          {t("web.common.error", { message: error.message })}
        </p>
      </AuthCard>
    );
  }
  return (
    <>
      {me.data.kind === "visitor" && <FirstRun />}
      {children}
    </>
  );
}

/** `reason`: why the charet asks (`token_required`, `replaced`, …). */
function TokenPage({ reason }: { reason: string }) {
  const t = useT();
  return (
    <AuthCard title={t("web.auth.tokenTitle")}>
      {reason === "replaced" ? (
        <p role="alert" className="text-sm text-destructive">
          {t("web.auth.replaced")}
        </p>
      ) : (
        <p className="text-sm text-muted-foreground">{t("web.auth.tokenRequired")}</p>
      )}
      <TokenForm />
    </AuthCard>
  );
}

/** Enters a member's token; the page reloads as them. */
export function TokenForm({ autoFocus = true }: { autoFocus?: boolean }) {
  const t = useT();
  const enter = useEnterToken();
  const [token, setToken] = useState("");
  const bad = enter.error instanceof HttpError && enter.error.status === 401;
  return (
    <form
      className="flex flex-col gap-3"
      onSubmit={(event) => {
        event.preventDefault();
        enter.mutate(token.trim(), { onSuccess: () => window.location.reload() });
      }}
    >
      <p className="text-sm text-muted-foreground">{t("web.auth.tokenLead")}</p>
      <Input
        autoFocus={autoFocus}
        type="password"
        autoComplete="off"
        value={token}
        onChange={(event) => setToken(event.target.value)}
        aria-label={t("web.auth.token")}
        placeholder={t("web.auth.token")}
      />
      {enter.error && (
        <p role="alert" className="text-sm text-destructive">
          {bad ? t("web.auth.badToken") : enter.error.message}
        </p>
      )}
      <Button type="submit" disabled={!token.trim() || enter.isPending}>
        {t("web.auth.enter")}
      </Button>
    </form>
  );
}
