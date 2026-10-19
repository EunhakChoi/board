import { Terminal } from "lucide-react";
import { useState } from "react";
import { useSearchParams } from "react-router";
import { AuthCard } from "@/components/AuthCard";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useT } from "@/lib/i18n";
import { useDecideDevice, useDeviceLogin, useMe } from "@/lib/queries";

/** `/device?code=…`: approve a `board login` so the CLI acts as you. */
export function Device() {
  const t = useT();
  const [params, setParams] = useSearchParams();
  const code = params.get("code") ?? "";
  const [typed, setTyped] = useState("");
  const me = useMe();
  const request = useDeviceLogin(code);
  const decide = useDecideDevice(code);
  if (!code) {
    return (
      <AuthCard title={t("web.device.title")}>
        <form
          className="flex flex-col gap-3"
          onSubmit={(event) => {
            event.preventDefault();
            setParams({ code: typed.trim() });
          }}
        >
          <p className="text-sm text-muted-foreground">{t("web.device.enterCode")}</p>
          <Input
            value={typed}
            onChange={(event) => setTyped(event.target.value)}
            placeholder="BCDF-GHJK"
            aria-label={t("web.device.code")}
          />
          <Button type="submit" disabled={!typed.trim()}>
            {t("web.device.next")}
          </Button>
        </form>
      </AuthCard>
    );
  }
  if (request.isPending) return null;
  if (request.error) {
    return (
      <AuthCard title={t("web.device.title")}>
        <p className="text-sm text-destructive">{t("web.device.gone")}</p>
      </AuthCard>
    );
  }
  const { name, status, userCode } = request.data;
  if (status !== "pending") {
    return (
      <AuthCard title={t("web.device.title")}>
        <p className="text-sm text-muted-foreground">
          {t(status === "approved" ? "web.device.approved" : "web.device.denied")}
        </p>
      </AuthCard>
    );
  }
  return (
    <AuthCard title={t("web.device.title")}>
      <Terminal className="mx-auto size-8 text-muted-foreground" />
      <p className="text-sm">
        {t("web.device.ask", { name, who: me.data?.email ?? me.data?.name ?? "" })}
      </p>
      <p className="font-mono text-lg tracking-widest">{userCode}</p>
      <p className="text-xs text-muted-foreground">{t("web.device.check")}</p>
      {decide.error && <p className="text-sm text-destructive">{decide.error.message}</p>}
      <div className="flex gap-2">
        <Button
          className="flex-1"
          variant="outline"
          disabled={decide.isPending}
          onClick={() => decide.mutate(false)}
        >
          {t("web.device.deny")}
        </Button>
        <Button className="flex-1" disabled={decide.isPending} onClick={() => decide.mutate(true)}>
          {t("web.device.approve")}
        </Button>
      </div>
    </AuthCard>
  );
}
