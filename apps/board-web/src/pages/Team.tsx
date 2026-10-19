import { Check, KeyRound, RefreshCw, Send, Trash2, X } from "lucide-react";
import { type FormEvent, useState } from "react";
import { AppHeader } from "@/components/AppHeader";
import { CopyOnce } from "@/components/CopyOnce";
import { SharingPanel } from "@/components/SharingPanel";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Segmented } from "@/components/ui/segmented";
import type { Role, User, Webhook } from "@/lib/api";
import { useT } from "@/lib/i18n";
import {
  useAdminActions,
  useAllowRules,
  useInviteActions,
  useInvites,
  useMe,
  useProjects,
  useUsers,
  useWebhookActions,
  useWebhooks,
} from "@/lib/queries";

const ROLES: Role[] = ["admin", "editor", "commenter", "viewer"];

/** `/team`: who is in, who is waiting, and who gets in on sight (admins). */
export function Team() {
  const t = useT();
  const me = useMe();
  if (me.data?.kind === "local") {
    // Local mode has no accounts; webhooks still work.
    return (
      <div className="flex min-h-svh flex-col">
        <AppHeader />
        <main className="mx-auto flex w-full max-w-4xl flex-1 flex-col gap-8 p-6">
          <h1 className="text-2xl font-semibold">{t("web.team.title")}</h1>
          <SharingPanel />
          <Webhooks />
        </main>
      </div>
    );
  }
  return <Members sharing={me.data?.localMode === true} />;
}

function Members({ sharing }: { sharing: boolean }) {
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
        {sharing && <SharingPanel />}
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
        <Invites />
        <Allowlist />
        <Webhooks />
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
  const { memberLink } = useInviteActions();
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
      <Button
        size="sm"
        variant="ghost"
        onClick={() => memberLink.mutate(user.id)}
        aria-label={t("web.invites.memberLink", { name: user.name })}
        title={t("web.invites.memberLink", { name: user.name })}
      >
        <KeyRound />
      </Button>
      {change.error && <p className="w-full text-sm text-destructive">{change.error.message}</p>}
      {memberLink.data && (
        <div className="w-full">
          <CopyOnce
            value={memberLink.data.url}
            label={t("web.invites.address")}
            note={t("web.invites.memberLinkOnce", { name: user.name })}
          />
        </div>
      )}
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

/** Addresses that get a signed POST when comments, versions or access requests happen. */
function Webhooks() {
  const t = useT();
  const hooks = useWebhooks();
  const projects = useProjects();
  const { create } = useWebhookActions();
  const [url, setUrl] = useState("");
  const [project, setProject] = useState("");
  const [events, setEvents] = useState<Set<string>>(new Set());
  const [secret, setSecret] = useState<{ id: string; value: string } | undefined>();
  const all = hooks.data?.events ?? [];
  const submit = (event: FormEvent) => {
    event.preventDefault();
    create.mutate(
      { url: url.trim(), events: [...events], project: project || null },
      {
        onSuccess: (created) => {
          setSecret({ id: created.webhook.id, value: created.secret });
          setUrl("");
          setEvents(new Set());
        },
      },
    );
  };
  const toggle = (name: string) => {
    const next = new Set(events);
    if (next.has(name)) next.delete(name);
    else next.add(name);
    setEvents(next);
  };
  return (
    <Section title={t("web.webhooks.title")}>
      <form onSubmit={submit} className="flex flex-col gap-3 p-3">
        <div className="flex gap-2">
          <Input
            value={url}
            onChange={(event) => setUrl(event.target.value)}
            placeholder="https://hooks.example.com/board"
            aria-label={t("web.webhooks.url")}
          />
          <Button type="submit" disabled={create.isPending || !url.trim()}>
            {t("web.webhooks.add")}
          </Button>
        </div>
        <fieldset className="flex flex-wrap gap-x-4 gap-y-1 text-sm">
          <legend className="mb-1 text-xs text-muted-foreground">{t("web.webhooks.events")}</legend>
          {all.map((name) => (
            <label key={name} className="flex items-center gap-1.5">
              <input
                type="checkbox"
                checked={events.has(name)}
                onChange={() => toggle(name)}
                className="accent-primary"
              />
              <code className="text-xs">{name}</code>
            </label>
          ))}
        </fieldset>
        <label className="flex items-center gap-2 text-sm">
          <span className="text-xs text-muted-foreground">{t("web.webhooks.project")}</span>
          <select
            value={project}
            onChange={(event) => setProject(event.target.value)}
            className="rounded-md border bg-background px-2 py-1 text-sm"
          >
            <option value="">{t("web.webhooks.allProjects")}</option>
            {(projects.data?.projects ?? []).map((p) => (
              <option key={p.key} value={p.key}>
                {p.title}
              </option>
            ))}
          </select>
        </label>
        {create.error && <p className="text-sm text-destructive">{create.error.message}</p>}
      </form>
      {(hooks.data?.webhooks ?? []).map((hook) => (
        <WebhookRow
          key={hook.id}
          hook={hook}
          secret={secret?.id === hook.id ? secret.value : undefined}
          onSecret={(value) => setSecret({ id: hook.id, value })}
        />
      ))}
      {hooks.data?.webhooks.length === 0 && <Empty text={t("web.webhooks.empty")} />}
    </Section>
  );
}

function WebhookRow({
  hook,
  secret,
  onSecret,
}: {
  hook: Webhook;
  secret: string | undefined;
  onSecret: (secret: string) => void;
}) {
  const t = useT();
  const { remove, rotate, test } = useWebhookActions();
  const ok = hook.lastStatus !== null && hook.lastStatus >= 200 && hook.lastStatus < 300;
  return (
    <div className="flex flex-col gap-2 p-3">
      <div className="flex items-center gap-2">
        <span className="min-w-0 flex-1 truncate text-sm font-medium">{hook.url}</span>
        {hook.lastDeliveryAt && (
          <Badge variant={ok ? "secondary" : "destructive"}>
            {hook.lastStatus ?? t("web.webhooks.failed")}
          </Badge>
        )}
        <Button
          size="sm"
          variant="outline"
          onClick={() => test.mutate(hook.id)}
          disabled={test.isPending}
        >
          <Send />
          {t("web.webhooks.test")}
        </Button>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => rotate.mutate(hook.id, { onSuccess: (r) => onSecret(r.secret) })}
          aria-label={t("web.webhooks.rotate", { url: hook.url })}
          title={t("web.webhooks.rotate", { url: hook.url })}
        >
          <RefreshCw />
        </Button>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => remove.mutate(hook.id)}
          aria-label={t("web.webhooks.remove", { url: hook.url })}
          title={t("web.webhooks.remove", { url: hook.url })}
        >
          <Trash2 />
        </Button>
      </div>
      <div className="text-xs text-muted-foreground">
        {hook.events.join(", ")}
        {hook.project ? ` · ${hook.project}` : ""} · {t("web.webhooks.secretEnds")} …
        {hook.secretLast4}
      </div>
      {hook.lastError && <p className="text-xs text-destructive">{hook.lastError}</p>}
      {secret && (
        <CopyOnce
          value={secret}
          label={t("web.webhooks.secret")}
          note={t("web.webhooks.secretOnce")}
        />
      )}
    </div>
  );
}

/** One-time links for new members: they open it, pick a name and are in with the role. */
function Invites() {
  const t = useT();
  const invites = useInvites();
  const { create, revoke } = useInviteActions();
  const [role, setRole] = useState<Role>("editor");
  const [label, setLabel] = useState("");
  const now = Date.now() / 1000;
  const open = (invites.data?.invites ?? []).filter((i) => !i.usedAt && i.expiresAt > now);
  return (
    <Section title={t("web.invites.section")} count={open.length}>
      <form
        className="flex flex-wrap items-center gap-2 p-3"
        onSubmit={(event) => {
          event.preventDefault();
          create.mutate({ role, label }, { onSuccess: () => setLabel("") });
        }}
      >
        <Segmented
          label={t("web.team.role")}
          value={role}
          onChange={setRole}
          options={ROLES.map((r) => ({ value: r, label: t(`web.team.role.${r}`) }))}
        />
        <Input
          className="w-48 flex-1"
          value={label}
          onChange={(event) => setLabel(event.target.value)}
          placeholder={t("web.links.labelPlaceholder")}
          aria-label={t("web.links.label")}
        />
        <Button type="submit" disabled={create.isPending}>
          {t("web.invites.create")}
        </Button>
      </form>
      {create.error && <p className="px-3 pb-3 text-sm text-destructive">{create.error.message}</p>}
      {create.data && (
        <div className="p-3">
          <CopyOnce
            value={create.data.url}
            label={t("web.invites.address")}
            note={t("web.invites.once")}
          />
        </div>
      )}
      {open.map((invite) => (
        <div key={invite.id} className="flex items-center gap-3 p-3">
          <Badge variant="secondary">{t(`web.team.role.${invite.role}`)}</Badge>
          <span className="flex-1 truncate text-sm">
            {invite.label || t("web.invites.unnamed")}
          </span>
          <span className="text-xs text-muted-foreground">
            {t("web.invites.until", {
              date: new Date(invite.expiresAt * 1000).toLocaleDateString(),
            })}
          </span>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => revoke.mutate(invite.id)}
            aria-label={t("web.invites.revoke", { label: invite.label || invite.id })}
          >
            <Trash2 />
          </Button>
        </div>
      ))}
      {open.length === 0 && !create.data && <Empty text={t("web.invites.none")} />}
    </Section>
  );
}
