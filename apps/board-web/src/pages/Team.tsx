import { Check, Trash2, X } from "lucide-react";
import { type FormEvent, useState } from "react";
import { AppHeader } from "@/components/AppHeader";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Segmented } from "@/components/ui/segmented";
import type { Role, User } from "@/lib/api";
import { useT } from "@/lib/i18n";
import { useAdminActions, useAllowRules, useMe, useUsers } from "@/lib/queries";

const ROLES: Role[] = ["admin", "editor", "commenter", "viewer"];

/** `/team`: who is in, who is waiting, and who gets in on sight (admins). */
export function Team() {
  const t = useT();
  const users = useUsers();
  const all = users.data?.users ?? [];
  const pending = all.filter((u) => u.status === "pending");
  const members = all.filter((u) => u.status === "active");
  const out = all.filter((u) => u.status === "rejected" || u.status === "removed");
  return (
    <div className="flex min-h-svh flex-col">
      <AppHeader />
      <main className="mx-auto flex w-full max-w-4xl flex-1 flex-col gap-8 p-6">
        <h1 className="text-2xl font-semibold">{t("web.team.title")}</h1>
        {users.error && (
          <p className="text-sm text-destructive">
            {t("web.common.error", { message: users.error.message })}
          </p>
        )}
        <Section title={t("web.team.requests")} count={pending.length}>
          {pending.length === 0 ? (
            <Empty text={t("web.team.noRequests")} />
          ) : (
            pending.map((user) => <RequestRow key={user.id} user={user} />)
          )}
        </Section>
        <Section title={t("web.team.members")} count={members.length}>
          {members.map((user) => (
            <MemberRow key={user.id} user={user} />
          ))}
        </Section>
        <Allowlist />
        {out.length > 0 && (
          <Section title={t("web.team.removed")} count={out.length}>
            {out.map((user) => (
              <RemovedRow key={user.id} user={user} />
            ))}
          </Section>
        )}
      </main>
    </div>
  );
}

function Section({
  title,
  count,
  children,
}: {
  title: string;
  count?: number;
  children: React.ReactNode;
}) {
  return (
    <section aria-label={title}>
      <h2 className="mb-2 flex items-center gap-2 text-sm font-medium text-muted-foreground">
        {title}
        {count !== undefined && <Badge variant="secondary">{count}</Badge>}
      </h2>
      <div className="divide-y rounded-xl border">{children}</div>
    </section>
  );
}

function Empty({ text }: { text: string }) {
  return <p className="p-4 text-sm text-muted-foreground">{text}</p>;
}

function Person({ user }: { user: User }) {
  return (
    <div className="min-w-0 flex-1">
      <div className="truncate text-sm font-medium">{user.name}</div>
      <div className="truncate text-xs text-muted-foreground">{user.email}</div>
    </div>
  );
}

function RequestRow({ user }: { user: User }) {
  const t = useT();
  const { approve, reject } = useAdminActions();
  return (
    <div className="flex items-center gap-3 p-3">
      <Person user={user} />
      <Button size="sm" onClick={() => approve.mutate(user.id)} disabled={approve.isPending}>
        <Check />
        {t("web.team.approve")}
      </Button>
      <Button
        size="sm"
        variant="outline"
        onClick={() => reject.mutate(user.id)}
        disabled={reject.isPending}
      >
        <X />
        {t("web.team.reject")}
      </Button>
    </div>
  );
}

function MemberRow({ user }: { user: User }) {
  const t = useT();
  const me = useMe();
  const { change } = useAdminActions();
  const self = me.data?.id === user.id;
  return (
    <div className="flex flex-wrap items-center gap-3 p-3">
      <Person user={user} />
      {self && <Badge variant="outline">{t("web.team.you")}</Badge>}
      <Segmented
        label={t("web.team.role")}
        value={user.role}
        onChange={(role) => change.mutate({ id: user.id, role })}
        options={ROLES.map((role) => ({ value: role, label: t(`web.team.role.${role}`) }))}
      />
      <Button
        size="sm"
        variant="ghost"
        onClick={() => change.mutate({ id: user.id, status: "removed" })}
        aria-label={t("web.team.remove", { name: user.name })}
        title={t("web.team.remove", { name: user.name })}
      >
        <Trash2 />
      </Button>
      {change.error && <p className="w-full text-sm text-destructive">{change.error.message}</p>}
    </div>
  );
}

function RemovedRow({ user }: { user: User }) {
  const t = useT();
  const { change } = useAdminActions();
  return (
    <div className="flex items-center gap-3 p-3">
      <Person user={user} />
      <Badge variant="outline">
        {t(`web.team.status.${user.status as "rejected" | "removed"}`)}
      </Badge>
      <Button
        size="sm"
        variant="outline"
        onClick={() => change.mutate({ id: user.id, status: "active" })}
      >
        {t("web.team.letBackIn")}
      </Button>
    </div>
  );
}

function Allowlist() {
  const t = useT();
  const rules = useAllowRules();
  const { addRule, removeRule } = useAdminActions();
  const [value, setValue] = useState("");
  const submit = (event: FormEvent) => {
    event.preventDefault();
    const text = value.trim();
    if (!text) return;
    const kind = text.includes("@") && !text.startsWith("@") ? "email" : "domain";
    addRule.mutate({ kind, value: text }, { onSuccess: () => setValue("") });
  };
  return (
    <Section title={t("web.team.allowlist")}>
      <form onSubmit={submit} className="flex gap-2 p-3">
        <Input
          value={value}
          onChange={(event) => setValue(event.target.value)}
          placeholder={t("web.team.allowPlaceholder")}
          aria-label={t("web.team.allowlist")}
        />
        <Button type="submit" disabled={addRule.isPending || !value.trim()}>
          {t("web.team.allowAdd")}
        </Button>
      </form>
      {addRule.error && (
        <p className="px-3 pb-3 text-sm text-destructive">{addRule.error.message}</p>
      )}
      {(rules.data?.rules ?? []).map((rule) => (
        <div key={rule.id} className="flex items-center gap-3 p-3">
          <Badge variant="secondary">{t(`web.team.kind.${rule.kind}`)}</Badge>
          <span className="flex-1 text-sm">{rule.value}</span>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => removeRule.mutate(rule.id)}
            aria-label={t("web.team.allowRemove", { value: rule.value })}
          >
            <Trash2 />
          </Button>
        </div>
      ))}
      {rules.data?.rules.length === 0 && <Empty text={t("web.team.allowEmpty")} />}
    </Section>
  );
}
