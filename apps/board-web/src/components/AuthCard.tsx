import type { ReactNode } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";

/** A centered card for the pages people see before they are in. */
export function AuthCard({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="flex min-h-svh items-center justify-center bg-muted/40 p-4">
      <Card className="w-full max-w-sm">
        <CardHeader className="items-center text-center">
          <img src="/favicon.svg" alt="" className="mb-2 size-10" />
          <CardTitle>{title}</CardTitle>
        </CardHeader>
        <CardContent>
          <div className="flex flex-col gap-4 text-center">{children}</div>
        </CardContent>
      </Card>
    </div>
  );
}

/** The server route that sends the browser to the identity provider. */
export function signInUrl({
  setup,
  returnTo,
}: {
  setup?: string | undefined;
  returnTo?: string | undefined;
}) {
  const query = new URLSearchParams();
  if (setup) query.set("setup", setup);
  if (returnTo) query.set("returnTo", returnTo);
  const text = query.toString();
  return `/api/auth/oidc/start${text ? `?${text}` : ""}`;
}
