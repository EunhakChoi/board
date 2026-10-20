import {
  ArrowLeft,
  CircleCheck,
  Columns2,
  GitMerge,
  Layers,
  LoaderCircle,
  MessageSquarePlus,
  ShieldAlert,
  TriangleAlert,
} from "lucide-react";
import { useMemo, useState } from "react";
import { Link, useParams, useSearchParams } from "react-router";
import { AgentPanel } from "@/components/AgentPanel";
import { CommentThread } from "@/components/CommentThread";
import { Live } from "@/components/Live";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Segmented } from "@/components/ui/segmented";
import { Textarea } from "@/components/ui/textarea";
import { type BranchDetail, HttpError, type Manifest, type TargetKind } from "@/lib/api";
import { diffLines, fold } from "@/lib/diff";
import { useLocale, useT } from "@/lib/i18n";
import {
  useAddComment,
  useAgent,
  useBranch,
  useBranchActions,
  useBranchComments,
  useDiff,
  useFileAt,
  useManifest,
  useMe,
} from "@/lib/queries";
import { useCan } from "@/lib/role";
import { useScreenTheme } from "@/lib/theme";
import { cn } from "@/lib/utils";
import { StatusBadge, when } from "./Branches";
import { Empty } from "./Screens";

type Tab = "screens" | "code" | "history" | "agent";

function active(branch: BranchDetail): boolean {
  return branch.status === "open" || branch.status === "in_review";
}

/** The check of what merging gives: building, passed, or the problems that hold it back. */
function MergeCheck({ check }: { check: NonNullable<BranchDetail["check"]> }) {
  const t = useT();
  if (check.state === "checking") {
    return (
      <div className="flex items-center gap-2 border-b px-6 py-2 text-sm text-muted-foreground">
        <LoaderCircle className="size-4 animate-spin" />
        {t("web.branch.checking")}
      </div>
    );
  }
  if (check.state === "passed") {
    return (
      <div className="flex items-center gap-2 border-b px-6 py-2 text-sm text-success">
        <CircleCheck className="size-4" />
        {t("web.branch.checkPassed")}
      </div>
    );
  }
  return (
    <div className="border-b bg-destructive/10 px-6 py-2 text-sm text-destructive">
      <p className="flex items-center gap-2">
        <TriangleAlert className="size-4" />
        {t("web.branch.checkFailed")}
      </p>
      <ul className="mt-1 list-disc pl-10 font-mono text-xs">
        {check.issues.map((issue) => (
          <li key={issue}>{issue}</li>
        ))}
      </ul>
    </div>
  );
}

/** Where the comparison starts and ends: a merged branch is its version against the one before. */
function revs(branch: BranchDetail): { before: string; after: string } {
  if (branch.status === "merged" && branch.mergedVersion) {
    return { before: `v${branch.mergedVersion - 1}`, after: `v${branch.mergedVersion}` };
  }
  return { before: branch.since, after: branch.head };
}

export function Branch() {
  const t = useT();
  const { locale } = useLocale();
  const params = useParams();
  const key = params.key ?? "";
  const number = Number(params.number);
  const [search, setSearch] = useSearchParams();
  const tab = (search.get("tab") as Tab | null) ?? "screens";
  const branch = useBranch(key, number);
  const canEdit = useCan("editor");
  const agent = useAgent(canEdit);
  if (branch.error)
    return <Empty text={t("web.common.error", { message: branch.error.message })} />;
  if (!branch.data) return <Empty text={t("web.common.loading")} />;
  const data = branch.data;
  const { before, after } = revs(data);
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex flex-wrap items-center gap-3 border-b px-6 py-3">
        <Button asChild variant="ghost" size="icon-sm">
          <Link to={`/p/${key}/branches`} aria-label={t("web.branches.title")}>
            <ArrowLeft />
          </Link>
        </Button>
        <h1 className="min-w-0 truncate text-lg font-semibold">
          <span className="text-muted-foreground">#{data.number}</span> {data.title}
        </h1>
        <StatusBadge status={data.status} />
        {data.behind && <Badge variant="outline">{t("web.branch.behind")}</Badge>}
        <span className="text-sm text-muted-foreground">
          {t("web.branch.by", { name: data.author.name, when: when(data.createdAt, locale) })}
        </span>
        <span className="flex-1" />
        <Actions projectKey={key} branch={data} />
      </header>
      {data.conflicts.length > 0 && (
        <div className="flex items-start gap-2 border-b bg-warning/10 px-6 py-2 text-sm text-warning">
          <TriangleAlert className="mt-0.5 size-4 shrink-0" />
          <span>
            {t("web.branch.clash")}{" "}
            <span className="font-mono text-xs">{data.conflicts.join(", ")}</span>
          </span>
        </div>
      )}
      {active(data) && data.unapproved.length > 0 && (
        <div className="flex items-start gap-2 border-b bg-warning/10 px-6 py-2 text-sm text-warning">
          <ShieldAlert className="mt-0.5 size-4 shrink-0" />
          <span>
            {t("web.branch.protected")}{" "}
            <span className="font-mono text-xs">{data.unapproved.join(", ")}</span>
          </span>
        </div>
      )}
      {active(data) && data.check && data.check.head === data.head && (
        <MergeCheck check={data.check} />
      )}
      {data.status === "merged" && data.mergedVersion && (
        <div className="flex items-center gap-2 border-b bg-muted/50 px-6 py-2 text-sm">
          <GitMerge className="size-4" />
          {t("web.branch.merged", { version: data.mergedVersion })}
          <Link to={`/p/${key}/versions`} className="underline">
            {t("web.nav.versions")}
          </Link>
        </div>
      )}
      <div className="flex items-center gap-3 border-b px-6 py-2">
        <Segmented
          label={t("web.branch.view")}
          value={tab}
          onChange={(value) => setSearch({ tab: value })}
          options={[
            { value: "screens", label: t("web.branch.tab.screens") },
            { value: "code", label: t("web.branch.tab.code") },
            { value: "history", label: t("web.branch.tab.history") },
            ...(canEdit && agent.data?.enabled
              ? [{ value: "agent" as const, label: t("web.agent.title") }]
              : []),
          ]}
        />
        <span className="font-mono text-xs text-muted-foreground">
          {short(before)} → {short(after)}
        </span>
      </div>
      {tab === "screens" && (
        <Screens projectKey={key} branch={data} before={before} after={after} />
      )}
      {tab === "code" && <Code projectKey={key} before={before} after={after} />}
      {tab === "agent" && (
        <div className="mx-auto flex min-h-0 w-full max-w-3xl flex-1 flex-col border-x">
          {!active(data) && (
            <p className="border-b p-3 text-sm text-muted-foreground">{t("web.agent.closed")}</p>
          )}
          <AgentPanel projectKey={key} branch={data.number} readOnly={!active(data)} />
        </div>
      )}
      {tab === "history" && (
        <ol className="flex-1 overflow-auto px-6 py-4 text-sm">
          {data.events.map((event, index) => (
            <li
              // biome-ignore lint/suspicious/noArrayIndexKey: the history only grows
              key={index}
              className="flex gap-3 border-b py-2 last:border-b-0"
            >
              <span className="w-40 shrink-0 text-xs text-muted-foreground">
                {when(event.at, locale)}
              </span>
              <span className="w-32 shrink-0 truncate">{event.actor.name}</span>
              <span>{eventText(t, event.kind)}</span>
              <span className="truncate font-mono text-xs text-muted-foreground">
                {event.detail}
              </span>
            </li>
          ))}
        </ol>
      )}
    </div>
  );
}

const EVENTS = [
  "created",
  "push",
  "review",
  "catch_up",
  "conflict",
  "merge",
  "archive",
  "idle",
  "reopen",
  "duplicate",
  "approve",
] as const;

function eventText(t: ReturnType<typeof useT>, kind: string): string {
  return (EVENTS as readonly string[]).includes(kind)
    ? t(`web.branch.event.${kind as (typeof EVENTS)[number]}`)
    : kind;
}

function short(rev: string): string {
  return /^[0-9a-f]{40}$/.test(rev) ? rev.slice(0, 7) : rev;
}

function Actions({ projectKey, branch }: { projectKey: string; branch: BranchDetail }) {
  const t = useT();
  const me = useMe();
  const canEdit = useCan("editor");
  const isAdmin = useCan("admin");
  const actions = useBranchActions(projectKey);
  if (!canEdit) return null;
  const mine = isAdmin || me.data?.id === branch.author.id;
  const active = branch.status === "open" || branch.status === "in_review";
  const mayChange = branch.status === "in_review" || mine;
  const busy = Object.values(actions).some((a) => a.isPending);
  const error = Object.values(actions).find((a) => a.error)?.error;
  const blocked = branch.unapproved.length > 0 && !isAdmin;
  return (
    <div className="flex flex-wrap items-center gap-2">
      {error && !(error instanceof HttpError && error.code === "check_failed") && (
        <span className="text-sm text-destructive">{error.message}</span>
      )}
      {active && isAdmin && branch.unapproved.length > 0 && (
        <Button
          size="sm"
          variant="outline"
          disabled={busy}
          onClick={() => actions.approve.mutate(branch.number)}
        >
          <ShieldAlert />
          {t("web.branch.approve")}
        </Button>
      )}
      {branch.status === "open" && mine && (
        <Button
          size="sm"
          variant="outline"
          disabled={busy}
          onClick={() => actions.review.mutate(branch.number)}
        >
          {t("web.branch.review")}
        </Button>
      )}
      {active && branch.behind && mayChange && (
        <Button
          size="sm"
          variant="outline"
          disabled={busy}
          onClick={() => actions.catchUp.mutate(branch.number)}
        >
          {t("web.branch.catchUp")}
        </Button>
      )}
      <Button
        size="sm"
        variant="ghost"
        disabled={busy}
        onClick={() => actions.duplicate.mutate(branch.number)}
      >
        {t("web.branch.duplicate")}
      </Button>
      {active && mayChange && (
        <Button
          size="sm"
          variant="ghost"
          disabled={busy}
          onClick={() => actions.archive.mutate(branch.number)}
        >
          {t("web.branch.archive")}
        </Button>
      )}
      {branch.status === "archived" && mine && (
        <Button
          size="sm"
          variant="outline"
          disabled={busy}
          onClick={() => actions.reopen.mutate(branch.number)}
        >
          {t("web.branch.reopen")}
        </Button>
      )}
      {active && (
        <Button
          size="sm"
          disabled={busy || blocked || branch.conflicts.length > 0}
          title={blocked ? t("web.branch.needsApproval") : undefined}
          onClick={() => actions.merge.mutate(branch.number)}
        >
          {actions.merge.isPending ? <LoaderCircle className="animate-spin" /> : <GitMerge />}
          {actions.merge.isPending ? t("web.branch.merging") : t("web.branch.merge")}
        </Button>
      )}
    </div>
  );
}

type Target = { kind: TargetKind; id: string; title: string; size: { w: number; h: number } };

function targetsOf(manifest: Manifest | undefined): Target[] {
  if (!manifest) return [];
  return [
    ...manifest.screens.map((s) => ({
      kind: "screen" as const,
      id: s.id,
      title: s.title,
      size: s.size,
    })),
    ...manifest.stories.map((s) => ({
      kind: "story" as const,
      id: s.id,
      title: s.title,
      size: s.size,
    })),
  ];
}

/** Each screen and story before and after, side by side or one over the other, with the comments
 * made on the branch's screens. */
function Screens({
  projectKey,
  branch,
  before,
  after,
}: {
  projectKey: string;
  branch: BranchDetail;
  before: string;
  after: string;
}) {
  const t = useT();
  const [theme] = useScreenTheme();
  const [search, setSearch] = useSearchParams();
  const canComment = useCan("commenter");
  const comments = useBranchComments(projectKey, branch.number);
  const add = useAddComment(projectKey);
  const [commenting, setCommenting] = useState(false);
  const [draft, setDraft] = useState<{ x: number; y: number }>();
  const [body, setBody] = useState("");
  const [focused, setFocused] = useState<number | undefined>(
    Number(search.get("comment")) || undefined,
  );
  const old = useManifest(projectKey, before);
  const now = useManifest(projectKey, after);
  const targets = useMemo(() => targetsOf(now.data?.manifest), [now.data]);
  const known = useMemo(
    () => new Set(targetsOf(old.data?.manifest).map((x) => `${x.kind}:${x.id}`)),
    [old.data],
  );
  const picked = search.get("target") ?? undefined;
  const setPicked = (id: string) => {
    setDraft(undefined);
    setSearch((now) => {
      const next = new URLSearchParams(now);
      next.set("target", id);
      next.delete("comment");
      return next;
    });
  };
  const [mode, setMode] = useState<"side" | "over">("side");
  const [mix, setMix] = useState(50);
  if (now.error) return <Empty text={t("web.common.error", { message: now.error.message })} />;
  if (!now.data) return <Empty text={t("web.build.running")} />;
  const current = targets.find((x) => `${x.kind}:${x.id}` === picked) ?? targets[0];
  const open = branch.status === "open" || branch.status === "in_review";
  const here = (comments.data?.comments ?? []).filter(
    (c) => current && c.kind === current.kind && c.target === current.id,
  );
  const pins = (scale: number) => (
    // biome-ignore lint/a11y/noStaticElementInteractions: a click picks the spot; the form below takes the text
    // biome-ignore lint/a11y/useKeyWithClickEvents: same
    <div
      className={cn("absolute inset-0", commenting ? "cursor-crosshair" : "pointer-events-none")}
      onClick={(e) => {
        if (!commenting) return;
        const box = e.currentTarget.getBoundingClientRect();
        setDraft({
          x: Math.round((e.clientX - box.left) / scale),
          y: Math.round((e.clientY - box.top) / scale),
        });
      }}
    >
      {here.map((c, index) => (
        <button
          type="button"
          key={c.id}
          data-testid={`pin-${c.id}`}
          onClick={(e) => {
            e.stopPropagation();
            setFocused(c.id);
          }}
          className={cn(
            "pointer-events-auto absolute flex size-6 -translate-x-1/2 -translate-y-1/2 items-center justify-center rounded-full border-2 border-background text-xs font-semibold shadow",
            c.status === "open"
              ? "bg-primary text-primary-foreground"
              : "bg-muted text-muted-foreground",
            focused === c.id && "ring-2 ring-ring",
          )}
          style={{
            left: (c.anchor.x + c.anchor.w / 2) * scale,
            top: (c.anchor.y + c.anchor.h / 2) * scale,
          }}
        >
          {index + 1}
        </button>
      ))}
      {draft && (
        <span
          className="absolute size-4 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-background bg-warning shadow"
          style={{ left: draft.x * scale, top: draft.y * scale }}
        />
      )}
    </div>
  );
  return (
    <div className="flex min-h-0 flex-1">
      <ul className="w-64 shrink-0 overflow-y-auto border-r py-1 text-sm">
        {targets.map((target) => {
          const id = `${target.kind}:${target.id}`;
          return (
            <li key={id}>
              <button
                type="button"
                onClick={() => setPicked(id)}
                className={cn(
                  "flex w-full items-center gap-2 truncate px-3 py-1.5 text-left hover:bg-accent",
                  current && id === `${current.kind}:${current.id}` && "bg-accent font-semibold",
                )}
              >
                <span className="truncate">{target.title}</span>
                {!known.has(id) && old.data ? (
                  <Badge variant="info">{t("web.branch.new")}</Badge>
                ) : (
                  branch.affected?.includes(id) && (
                    <Badge variant="outline">{t("web.branch.changed")}</Badge>
                  )
                )}
              </button>
            </li>
          );
        })}
      </ul>
      {current ? (
        <section className="min-w-0 flex-1 overflow-auto p-4">
          <div className="mb-3 flex items-center gap-3">
            <Segmented
              label={t("web.branch.compare")}
              value={mode}
              onChange={setMode}
              options={[
                { value: "side", label: <Columns2 aria-label={t("web.branch.side")} /> },
                { value: "over", label: <Layers aria-label={t("web.branch.over")} /> },
              ]}
            />
            {canComment && open && (
              <Button
                size="sm"
                variant={commenting ? "default" : "outline"}
                onClick={() => {
                  setCommenting(!commenting);
                  setDraft(undefined);
                }}
              >
                <MessageSquarePlus />
                {t("web.comments.add")}
              </Button>
            )}
            {mode === "over" && (
              <input
                type="range"
                min={0}
                max={100}
                value={mix}
                onChange={(e) => setMix(Number(e.target.value))}
                aria-label={t("web.branch.mix")}
                className="w-48"
              />
            )}
          </div>
          {mode === "side" ? (
            <div className="grid grid-cols-2 gap-4">
              <figure className="min-w-0">
                <figcaption className="mb-1 text-xs text-muted-foreground">
                  {t("web.branch.before")}
                </figcaption>
                {known.has(`${current.kind}:${current.id}`) ? (
                  <Live
                    projectKey={projectKey}
                    rev={before}
                    kind={current.kind}
                    id={current.id}
                    theme={theme}
                    size={current.size}
                  />
                ) : (
                  <p className="rounded-lg border border-dashed p-6 text-sm text-muted-foreground">
                    {t("web.branch.notBefore")}
                  </p>
                )}
              </figure>
              <figure className="min-w-0">
                <figcaption className="mb-1 text-xs text-muted-foreground">
                  {t("web.branch.after")}
                </figcaption>
                <Live
                  projectKey={projectKey}
                  rev={after}
                  kind={current.kind}
                  id={current.id}
                  theme={theme}
                  size={current.size}
                  overlay={pins}
                />
              </figure>
            </div>
          ) : (
            <div className="relative">
              <Live
                projectKey={projectKey}
                rev={before}
                kind={current.kind}
                id={current.id}
                theme={theme}
                size={current.size}
              />
              <div className="absolute inset-0" style={{ opacity: mix / 100 }}>
                <Live
                  projectKey={projectKey}
                  rev={after}
                  kind={current.kind}
                  id={current.id}
                  theme={theme}
                  size={current.size}
                  overlay={pins}
                />
              </div>
            </div>
          )}
          {commenting && !draft && (
            <p className="mt-3 text-sm text-muted-foreground">{t("web.branch.pickSpot")}</p>
          )}
          {draft && (
            <form
              className="mt-3 flex max-w-xl flex-col gap-2"
              onSubmit={(e) => {
                e.preventDefault();
                if (!body.trim()) return;
                add.mutate(
                  {
                    kind: current.kind,
                    target: current.id,
                    theme,
                    at: draft,
                    body,
                    branch: branch.number,
                  },
                  {
                    onSuccess: (comment) => {
                      setDraft(undefined);
                      setBody("");
                      setCommenting(false);
                      setFocused(comment.id);
                    },
                  },
                );
              }}
            >
              <Textarea
                autoFocus
                value={body}
                onChange={(e) => setBody(e.target.value)}
                placeholder={t("web.branch.commentPlaceholder")}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && (e.metaKey || e.ctrlKey))
                    e.currentTarget.form?.requestSubmit();
                }}
              />
              <div className="flex items-center gap-2">
                <Button type="submit" size="sm" disabled={add.isPending || !body.trim()}>
                  {t("web.comments.add")}
                </Button>
                <Button type="button" size="sm" variant="ghost" onClick={() => setDraft(undefined)}>
                  {t("web.common.cancel")}
                </Button>
                {add.error && <span className="text-sm text-destructive">{add.error.message}</span>}
              </div>
            </form>
          )}
          {here.length > 0 && (
            <div className="mt-4 flex max-w-xl flex-col gap-2">
              {here.map((c) => (
                <CommentThread
                  key={c.id}
                  projectKey={projectKey}
                  comment={c}
                  active={focused === c.id}
                  onFocus={() => setFocused(c.id)}
                />
              ))}
            </div>
          )}
        </section>
      ) : (
        <Empty text={t("web.screens.empty")} />
      )}
    </div>
  );
}

/** The changed files, each as a line diff. */
function Code({
  projectKey,
  before,
  after,
}: {
  projectKey: string;
  before: string;
  after: string;
}) {
  const t = useT();
  const diff = useDiff(projectKey, before, after);
  const [picked, setPicked] = useState<string>();
  const changes = diff.data?.changes ?? [];
  const current = changes.find((c) => c.path === picked) ?? changes[0];
  if (diff.error) return <Empty text={t("web.common.error", { message: diff.error.message })} />;
  if (!diff.data) return <Empty text={t("web.common.loading")} />;
  if (changes.length === 0) return <Empty text={t("web.branch.noChanges")} />;
  const mark = { added: "A", modified: "M", deleted: "D" } as const;
  return (
    <div className="flex min-h-0 flex-1">
      <ul className="w-80 shrink-0 overflow-y-auto border-r py-1">
        {changes.map((change) => (
          <li key={change.path}>
            <button
              type="button"
              onClick={() => setPicked(change.path)}
              className={cn(
                "flex w-full items-center gap-2 truncate px-3 py-1 text-left font-mono text-xs hover:bg-accent",
                change.path === current?.path && "bg-accent font-semibold",
              )}
              title={change.path}
            >
              <span
                className={cn(
                  "w-3 shrink-0",
                  change.kind === "added" && "text-success",
                  change.kind === "deleted" && "text-destructive",
                )}
              >
                {mark[change.kind]}
              </span>
              <span className="truncate">{change.path}</span>
            </button>
          </li>
        ))}
      </ul>
      {current && (
        <FileDiff projectKey={projectKey} before={before} after={after} path={current.path} />
      )}
    </div>
  );
}

function FileDiff({
  projectKey,
  before,
  after,
  path,
}: {
  projectKey: string;
  before: string;
  after: string;
  path: string;
}) {
  const t = useT();
  const old = useFileAt(projectKey, before, path);
  const now = useFileAt(projectKey, after, path);
  const rows = useMemo(() => {
    if (!old.data || !now.data) return undefined;
    const lines = diffLines(old.data.text ?? "", now.data.text ?? "");
    return lines ? fold(lines) : null;
  }, [old.data, now.data]);
  return (
    <section className="min-w-0 flex-1 overflow-auto">
      <div className="sticky top-0 border-b bg-background px-4 py-2 font-mono text-xs">{path}</div>
      {rows === undefined && <Empty text={t("web.common.loading")} />}
      {rows === null && <Empty text={t("web.branch.tooLarge")} />}
      {rows && (
        <table className="w-full border-collapse font-mono text-xs">
          <tbody>
            {rows.map((row, index) =>
              row.kind === "fold" ? (
                // biome-ignore lint/suspicious/noArrayIndexKey: rows are positional
                <tr key={index} className="bg-muted/50 text-muted-foreground">
                  <td colSpan={3} className="px-3 py-1">
                    {t("web.branch.folded", { count: row.count })}
                  </td>
                </tr>
              ) : (
                <tr
                  // biome-ignore lint/suspicious/noArrayIndexKey: rows are positional
                  key={index}
                  className={cn(
                    row.kind === "add" && "bg-success/10",
                    row.kind === "del" && "bg-destructive/10",
                  )}
                >
                  <td className="w-10 select-none px-2 text-right align-top text-muted-foreground">
                    {row.before ?? ""}
                  </td>
                  <td className="w-10 select-none px-2 text-right align-top text-muted-foreground">
                    {row.after ?? ""}
                  </td>
                  <td className="whitespace-pre px-3">
                    {row.kind === "add" ? "+ " : row.kind === "del" ? "- " : "  "}
                    {row.text || " "}
                  </td>
                </tr>
              ),
            )}
          </tbody>
        </table>
      )}
    </section>
  );
}
