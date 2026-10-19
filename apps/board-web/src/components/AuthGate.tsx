import type { ReactNode } from "react";
import { Navigate, useLocation } from "react-router";
import { AuthCard } from "@/components/AuthCard";
import { Button } from "@/components/ui/button";
import { HttpError } from "@/lib/api";
import { useT } from "@/lib/i18n";
import { useMe, useSignOut } from "@/lib/queries";

/** Shows the board only to people who are in: others get the sign-in or waiting page. */
export function AuthGate({ children }: { children: ReactNode }) {
  const t = useT();
  const me = useMe();
  const location = useLocation();
  const signOut = useSignOut();
  if (me.isPending) return null;
  if (me.error) {
    const error = me.error;
    if (error instanceof HttpError && error.status === 401) {
      const returnTo = `${location.pathname}${location.search}`;
      return <Navigate to={`/signin?returnTo=${encodeURIComponent(returnTo)}`} replace />;
    }
    if (error instanceof HttpError && error.status === 403) {
      return <Navigate to={`/signin?error=${encodeURIComponent(error.code)}`} replace />;
    }
    return (
      <AuthCard title={t("web.auth.signIn")}>
        <p className="text-sm text-destructive">
          {t("web.common.error", { message: error.message })}
        </p>
      </AuthCard>
    );
  }
  if (me.data.kind === "pending") {
    return (
      <AuthCard title={t("web.auth.waitingTitle")}>
        <p className="text-sm text-muted-foreground">
          {t("web.auth.waitingLead", { email: me.data.email ?? me.data.name })}
        </p>
        <Button variant="outline" onClick={() => signOut.mutate()}>
          {t("web.auth.useOther")}
        </Button>
      </AuthCard>
    );
  }
  return <>{children}</>;
}
