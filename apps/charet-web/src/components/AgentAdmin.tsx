import { type FormEvent, useEffect, useState } from "react";
import { Section } from "@/components/Section";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Segmented } from "@/components/ui/segmented";
import type { AgentKind } from "@/lib/api";
import { useLocale, useT } from "@/lib/i18n";
import { useAgentCosts, useAgentSettings } from "@/lib/queries";

/** The web agent: which agent runs by default, with which model, and the API keys. */
export function AgentSettings() {
  const t = useT();
  const { status, save } = useAgentSettings(true);
  const [models, setModels] = useState<Record<string, string>>({});
  const [keys, setKeys] = useState<Record<string, string>>({});
  useEffect(() => {
    if (status.data) {
      setModels(Object.fromEntries(status.data.agents.map((a) => [a.agent, a.model])));
    }
  }, [status.data]);
  const data = status.data;
  const submit = (event: FormEvent) => {
    event.preventDefault();
    const typed = Object.fromEntries(Object.entries(keys).filter(([, v]) => v.trim()));
    save.mutate({ models, keys: typed }, { onSuccess: () => setKeys({}) });
  };
  return (
    <Section title={t("web.agentAdmin.title")} lead={t("web.agentAdmin.lead")}>
      {status.error && <p className="p-3 text-sm text-destructive">{status.error.message}</p>}
      {data && (
        <form onSubmit={submit} className="flex flex-col divide-y">
          <div className="flex flex-wrap items-center gap-3 p-3 text-sm">
            <Badge variant={data.enabled ? "info" : "outline"}>
              {t(data.enabled ? "web.agentAdmin.on" : "web.agentAdmin.off")}
            </Badge>
            {data.reason && <span className="text-muted-foreground">{data.reason}</span>}
            <span className="flex-1" />
            {data.agents.filter((a) => a.ready).length > 1 && data.default && (
              <Segmented
                label={t("web.agentAdmin.default")}
                value={data.default}
                onChange={(agent: AgentKind) => save.mutate({ default: agent })}
                options={data.agents
                  .filter((a) => a.ready)
                  .map((a) => ({ value: a.agent, label: a.agent }))}
              />
            )}
          </div>
          {data.agents
            .filter((a) => a.agent !== "script")
            .map((a) => (
              <div key={a.agent} className="flex flex-wrap items-center gap-2 p-3 text-sm">
                <span className="w-16 font-medium">{a.agent}</span>
                <Input
                  className="w-48"
                  value={models[a.agent] ?? ""}
                  onChange={(event) => setModels({ ...models, [a.agent]: event.target.value })}
                  placeholder={t("web.agentAdmin.modelDefault")}
                  aria-label={t("web.agentAdmin.model", { agent: a.agent })}
                />
                <Input
                  className="w-56 flex-1"
                  type="password"
                  autoComplete="off"
                  value={keys[a.agent] ?? ""}
                  onChange={(event) => setKeys({ ...keys, [a.agent]: event.target.value })}
                  disabled={a.key === "env"}
                  placeholder={
                    a.key === "env"
                      ? t("web.agentAdmin.keyEnv", { last4: a.keyLast4 ?? "" })
                      : a.key === "settings"
                        ? t("web.agentAdmin.keySaved", { last4: a.keyLast4 ?? "" })
                        : t("web.agentAdmin.keyNone")
                  }
                  aria-label={t("web.agentAdmin.key", { agent: a.agent })}
                />
                {a.key === "settings" && (
                  <Button
                    type="button"
                    size="sm"
                    variant="ghost"
                    disabled={save.isPending}
                    onClick={() => save.mutate({ keys: { [a.agent]: "" } })}
                  >
                    {t("web.agentAdmin.removeKey")}
                  </Button>
                )}
              </div>
            ))}
          <div className="flex items-center justify-end gap-2 p-3">
            {save.error && <span className="text-sm text-destructive">{save.error.message}</span>}
            <Button type="submit" size="sm" disabled={save.isPending}>
              {t("web.agentAdmin.save")}
            </Button>
          </div>
        </form>
      )}
    </Section>
  );
}

/** What each person's web agent runs cost. */
export function AgentCosts() {
  const t = useT();
  const { locale } = useLocale();
  const costs = useAgentCosts(true);
  const people = costs.data?.people ?? [];
  const money = (n: number) =>
    new Intl.NumberFormat(locale, {
      style: "currency",
      currency: "USD",
      maximumFractionDigits: 4,
    }).format(n);
  const count = (n: number) => new Intl.NumberFormat(locale).format(n);
  return (
    <Section title={t("web.agentAdmin.costs")} count={costs.data?.runs.length}>
      {costs.error && <p className="p-3 text-sm text-destructive">{costs.error.message}</p>}
      {people.length === 0 ? (
        <p className="p-4 text-sm text-muted-foreground">{t("web.agentAdmin.noRuns")}</p>
      ) : (
        <table className="w-full text-sm">
          <thead className="text-left text-xs text-muted-foreground">
            <tr>
              <th className="p-3 font-normal">{t("web.agentAdmin.person")}</th>
              <th className="p-3 text-right font-normal">{t("web.agentAdmin.runs")}</th>
              <th className="p-3 text-right font-normal">{t("web.agentAdmin.tokens")}</th>
              <th className="p-3 text-right font-normal">{t("web.agentAdmin.cost")}</th>
            </tr>
          </thead>
          <tbody className="divide-y border-t">
            {people.map((p) => (
              <tr key={p.id}>
                <td className="p-3">{p.name}</td>
                <td className="p-3 text-right tabular-nums">{p.runs}</td>
                <td className="p-3 text-right tabular-nums">
                  {count(p.tokensIn)} / {count(p.tokensOut)}
                </td>
                <td className="p-3 text-right tabular-nums">
                  {money(p.costUsd)}
                  {p.unpriced > 0 && (
                    <span
                      className="ml-1 text-xs text-muted-foreground"
                      title={t("web.agentAdmin.unpricedHint")}
                    >
                      {t("web.agentAdmin.unpriced", { count: p.unpriced })}
                    </span>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </Section>
  );
}
