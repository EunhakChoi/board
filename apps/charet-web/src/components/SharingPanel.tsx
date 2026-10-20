import { Globe } from "lucide-react";
import { type FormEvent, useEffect, useId, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Segmented } from "@/components/ui/segmented";
import type { ShareMode } from "@/lib/api";
import { useT } from "@/lib/i18n";
import { useSharing, useSharingActions } from "@/lib/queries";

/** Port of the remote door; tunnels and reverse proxies point here. */
const REMOTE_PORT = 5284;

/**
 * Sharing a local charet with other machines: the tunnel that runs and the address links use.
 * Members come in with their token through any address; this machine stays the admin.
 */
export function SharingPanel() {
  const t = useT();
  const sharing = useSharing(true);
  const { start, stop } = useSharingActions();
  const [url, setUrl] = useState("");
  const [mode, setMode] = useState<ShareMode>("address");
  const [authtoken, setAuthtoken] = useState("");
  const [domain, setDomain] = useState("");
  const tokenId = useId();
  const domainId = useId();
  const urlId = useId();
  const data = sharing.data;
  // While shared, the form starts from what is on, so a changed domain or address is applied as is.
  const current = data?.mode ?? null;
  const currentUrl = data?.url ?? "";
  const currentDomain = data?.domain ?? "";
  useEffect(() => {
    if (!current) return;
    setMode(current);
    setUrl(current === "address" ? currentUrl : "");
    setDomain(currentDomain);
  }, [current, currentUrl, currentDomain]);
  if (!data) return null;
  const on = data.url !== null;
  const chosen = data.modes.find((m) => m.mode === mode);
  const submit = (event: FormEvent) => {
    event.preventDefault();
    const body =
      mode === "address"
        ? { mode, url }
        : mode === "ngrok"
          ? { mode, ...(authtoken ? { authtoken } : {}), ...(domain ? { domain } : {}) }
          : { mode };
    start.mutate(body, { onSuccess: () => setAuthtoken("") });
  };
  const error = start.error ?? stop.error;
  return (
    <section aria-label={t("web.sharing.title")} className="flex flex-col gap-3">
      <h2 className="flex items-center gap-2 text-sm font-medium text-muted-foreground">
        <Globe className="size-4" />
        {t("web.sharing.title")}
        <Badge variant={on ? "default" : "secondary"}>
          {on ? t("web.sharing.on") : t("web.sharing.off")}
        </Badge>
      </h2>
      <div className="flex flex-col gap-3 rounded-xl border p-4">
        {data.fixed ? (
          <p className="text-sm text-muted-foreground">{t("web.sharing.fixed")}</p>
        ) : (
          <form className="flex flex-col gap-3" onSubmit={submit}>
            {on && (
              <>
                <p className="text-sm">
                  {t("web.sharing.openAt")}{" "}
                  <a className="font-medium underline" href={data.url ?? undefined}>
                    {data.url}
                  </a>
                </p>
                <p className="text-sm text-muted-foreground">{t("web.sharing.onLead")}</p>
              </>
            )}
            {data.modes.length > 1 && (
              <Segmented
                label={t("web.sharing.mode")}
                value={mode}
                onChange={setMode}
                options={data.modes.map((m) => ({
                  value: m.mode,
                  label: t(`web.sharing.mode.${m.mode}`),
                }))}
              />
            )}
            <p className="text-sm text-muted-foreground">
              {mode === "address"
                ? t("web.sharing.lead", { target: `127.0.0.1:${REMOTE_PORT}` })
                : t(`web.sharing.lead.${mode}`)}
            </p>
            {chosen && mode !== "address" && chosen.detail && (
              <p className={chosen.available ? "text-sm" : "text-sm text-destructive"}>
                {chosen.available
                  ? t("web.sharing.willOpen", { url: chosen.detail })
                  : chosen.detail}
              </p>
            )}
            {mode === "address" && (
              <div className="flex flex-col gap-1 text-sm">
                <label htmlFor={urlId}>{t("web.sharing.address")}</label>
                <Input
                  id={urlId}
                  value={url}
                  onChange={(event) => setUrl(event.target.value)}
                  placeholder="https://charet.example.com"
                  required
                />
              </div>
            )}
            {mode === "ngrok" && (
              <>
                <div className="flex flex-col gap-1 text-sm">
                  <label htmlFor={tokenId}>{t("web.sharing.ngrokToken")}</label>
                  <Input
                    id={tokenId}
                    type="password"
                    autoComplete="off"
                    value={authtoken}
                    onChange={(event) => setAuthtoken(event.target.value)}
                    placeholder={
                      data.ngrokToken
                        ? t("web.sharing.ngrokSaved", { last: data.ngrokToken })
                        : undefined
                    }
                    required={!data.ngrokToken}
                  />
                </div>
                <div className="flex flex-col gap-1 text-sm">
                  <label htmlFor={domainId}>{t("web.sharing.ngrokDomain")}</label>
                  <Input
                    id={domainId}
                    value={domain}
                    onChange={(event) => setDomain(event.target.value)}
                    placeholder="my-team.ngrok.app"
                  />
                </div>
              </>
            )}
            <div className="flex gap-2">
              <Button type="submit" disabled={start.isPending || chosen?.available === false}>
                {on ? t("web.sharing.apply") : t("web.sharing.start")}
              </Button>
              {on && (
                <Button
                  type="button"
                  variant="outline"
                  disabled={stop.isPending}
                  onClick={() => stop.mutate()}
                >
                  {t("web.sharing.stop")}
                </Button>
              )}
            </div>
          </form>
        )}
        {data.warning && <p className="text-sm text-destructive">{data.warning}</p>}
        {error && (
          <p className="text-sm text-destructive">
            {t("web.common.error", { message: error.message })}
          </p>
        )}
      </div>
    </section>
  );
}
