import { Globe } from "lucide-react";
import { type FormEvent, useId, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useT } from "@/lib/i18n";
import { useSharing, useSharingActions } from "@/lib/queries";

/** Port of the remote door; tunnels and reverse proxies point here. */
const REMOTE_PORT = 5284;

/**
 * Sharing a local board with other machines (only shown through the local door): the address
 * people open, and the owner account this machine acts as from then on.
 */
export function SharingPanel() {
  const t = useT();
  const sharing = useSharing(true);
  const { start, stop } = useSharingActions();
  const [url, setUrl] = useState("");
  const [name, setName] = useState("");
  const nameId = useId();
  const urlId = useId();
  const data = sharing.data;
  if (!data) return null;
  const on = data.url !== null;
  const submit = (event: FormEvent) => {
    event.preventDefault();
    start.mutate(
      data.owner ? { mode: "address", url } : { mode: "address", url, ownerName: name },
      { onSuccess: () => setUrl("") },
    );
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
        ) : on ? (
          <>
            <p className="text-sm">
              {t("web.sharing.openAt")}{" "}
              <a className="font-medium underline" href={data.url ?? undefined}>
                {data.url}
              </a>
            </p>
            <p className="text-sm text-muted-foreground">{t("web.sharing.onLead")}</p>
            <div>
              <Button variant="outline" disabled={stop.isPending} onClick={() => stop.mutate()}>
                {t("web.sharing.stop")}
              </Button>
            </div>
          </>
        ) : (
          <form className="flex flex-col gap-3" onSubmit={submit}>
            <p className="text-sm text-muted-foreground">
              {t("web.sharing.lead", { target: `127.0.0.1:${REMOTE_PORT}` })}
            </p>
            {!data.owner && (
              <div className="flex flex-col gap-1 text-sm">
                <label htmlFor={nameId}>{t("web.sharing.ownerName")}</label>
                <Input
                  id={nameId}
                  value={name}
                  onChange={(event) => setName(event.target.value)}
                  maxLength={40}
                  required
                />
              </div>
            )}
            <div className="flex flex-col gap-1 text-sm">
              <label htmlFor={urlId}>{t("web.sharing.address")}</label>
              <Input
                id={urlId}
                value={url}
                onChange={(event) => setUrl(event.target.value)}
                placeholder="https://board.example.com"
                required
              />
            </div>
            <div>
              <Button type="submit" disabled={start.isPending}>
                {t("web.sharing.start")}
              </Button>
            </div>
          </form>
        )}
        {error && (
          <p className="text-sm text-destructive">
            {t("web.common.error", { message: error.message })}
          </p>
        )}
      </div>
    </section>
  );
}
