import { ArrowDown, ArrowUp, Bot, LoaderCircle, Redo2, Undo2, X } from "lucide-react";
import { useMemo, useState } from "react";
import { Link } from "react-router";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Segmented } from "@/components/ui/segmented";
import { type Branch, type Element, HttpError } from "@/lib/api";
import { useT } from "@/lib/i18n";
import { type HandEditInput, useBranchActions, useDesign, useDesignActions } from "@/lib/queries";
import { FAMILIES, type Family, PADDING, SCALE, spacingOf, withSpacing } from "@/lib/spacing";

type Edit = HandEditInput["edit"];
/** One saved edit, for undo and redo: the file before and after it. */
type Step = { branch: number; path: string; before: string; after: string };
type Status =
  | { kind: "checking" }
  | { kind: "passed"; branch: number }
  | { kind: "failed"; issues: string[]; reverted: boolean }
  | { kind: "error"; message: string };

/** Where the picked element is in the code: `src` (as written) and `use` (the instance). */
export type Located = { src?: string | undefined; use?: string | undefined };

/**
 * Hand edits of the picked element (story 05): text, `cva` variants, spacing and order, on the
 * person's draft. Each change is checked; one that breaks a screen is taken back.
 */
export function DesignPanel({
  projectKey,
  screen,
  app,
  drafts,
  draft,
  onDraft,
  pick,
  onEdited,
  onAskAgent,
}: {
  projectKey: string;
  screen: string;
  /** The app directory (`charet.json` `app`), to turn project paths back into element paths. */
  app: string;
  drafts: Branch[];
  draft: number | "new";
  onDraft: (draft: number | "new") => void;
  pick: Element | undefined;
  /** Where the element is after an edit (it may have moved), to pick it again on the new picture. */
  onEdited: (at: Located) => void;
  onAskAgent: () => void;
}) {
  const t = useT();
  const branch = draft === "new" ? undefined : drafts.find((d) => d.number === draft);
  const rev = branch?.head;
  const design = useDesign(projectKey, rev, pick?.src, pick?.use);
  const actions = useDesignActions(projectKey);
  const review = useBranchActions(projectKey).review;
  const [scope, setScope] = useState<"instance" | "original">("instance");
  const [status, setStatus] = useState<Status | undefined>();
  const [undo, setUndo] = useState<Step[]>([]);
  const [redo, setRedo] = useState<Step[]>([]);

  const info = design.data;
  const target =
    (scope === "instance" ? info?.instance : info?.original) ?? info?.instance ?? info?.original;
  const busy = actions.edit.isPending || actions.check.isPending || actions.restore.isPending;
  const local = (at: string) => (app && at.startsWith(`${app}/`) ? at.slice(app.length + 1) : at);

  const apply = (edit: Edit) => {
    if (!target) return;
    setStatus(undefined);
    actions.edit.mutate(
      {
        ...(branch ? { branch: branch.number } : {}),
        title: t("web.design.draftTitle", { screen }),
        at: target.at,
        edit,
      },
      {
        onSuccess: (edited) => {
          onDraft(edited.branch.number);
          const moved = local(edited.at);
          onEdited(target === info?.instance ? { use: moved } : { src: moved });
          setStatus({ kind: "checking" });
          actions.check.mutate(
            {
              branch: edited.branch.number,
              path: edited.path,
              before: edited.before,
              commit: edited.commit,
            },
            {
              onSuccess: (check) => {
                if (check.state === "passed") {
                  setStatus({ kind: "passed", branch: edited.branch.number });
                  setUndo((steps) => [
                    ...steps,
                    {
                      branch: edited.branch.number,
                      path: edited.path,
                      before: edited.before,
                      after: edited.commit,
                    },
                  ]);
                  setRedo([]);
                } else {
                  setStatus({ kind: "failed", issues: check.issues, reverted: check.reverted });
                }
              },
              onError: (error) => setStatus({ kind: "error", message: error.message }),
            },
          );
        },
        onError: (error) => setStatus({ kind: "error", message: error.message }),
      },
    );
  };

  const step = (from: Step[], back: boolean) => {
    const last = from.at(-1);
    if (!last) return;
    actions.restore.mutate(
      {
        branch: last.branch,
        path: last.path,
        to: back ? last.before : last.after,
        expect: back ? last.after : last.before,
      },
      {
        onSuccess: (restored) => {
          const moved = { ...last, [back ? "before" : "after"]: restored.commit };
          if (back) {
            setUndo((s) => s.slice(0, -1));
            setRedo((s) => [...s, moved]);
          } else {
            setRedo((s) => s.slice(0, -1));
            setUndo((s) => [...s, moved]);
          }
          setStatus({ kind: "passed", branch: last.branch });
        },
        onError: (error) =>
          setStatus({
            kind: "error",
            message:
              error instanceof HttpError && error.status === 409
                ? t("web.design.changedSince")
                : error.message,
          }),
      },
    );
  };
  // After an undo the file is as it was before: redo puts the edit back, and the next undo
  // starts from the commit the redo made.
  const undoLast = () => step(undo, true);
  const redoLast = () => step(redo, false);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex flex-col gap-2 border-b p-3">
        <label htmlFor="design-branch" className="text-xs text-muted-foreground">
          {t("web.agent.where")}
        </label>
        <div className="flex items-center gap-1">
          <select
            id="design-branch"
            className="h-8 min-w-0 flex-1 rounded-md border bg-background px-2 text-sm"
            value={String(draft)}
            onChange={(event) => {
              // Another draft has its own history.
              setUndo([]);
              setRedo([]);
              setStatus(undefined);
              onDraft(event.target.value === "new" ? "new" : Number(event.target.value));
            }}
          >
            <option value="new">{t("web.agent.newDraft")}</option>
            {drafts.map((d) => (
              <option key={d.number} value={d.number}>
                #{d.number} {d.title}
              </option>
            ))}
          </select>
          <Button
            size="icon-sm"
            variant="ghost"
            aria-label={t("web.design.undo")}
            title={t("web.design.undo")}
            disabled={busy || undo.length === 0}
            onClick={undoLast}
          >
            <Undo2 />
          </Button>
          <Button
            size="icon-sm"
            variant="ghost"
            aria-label={t("web.design.redo")}
            title={t("web.design.redo")}
            disabled={busy || redo.length === 0}
            onClick={redoLast}
          >
            <Redo2 />
          </Button>
        </div>
        {branch && (
          <div className="flex items-center gap-2 text-sm">
            <Link
              className="text-info hover:underline"
              to={`/p/${projectKey}/branches/${branch.number}`}
            >
              {t("web.agent.openBranch", { number: branch.number })}
            </Link>
            <span className="flex-1" />
            {branch.status === "open" && (
              <Button
                size="sm"
                variant="outline"
                disabled={review.isPending}
                onClick={() => review.mutate(branch.number)}
              >
                {t("web.branch.review")}
              </Button>
            )}
          </div>
        )}
      </div>
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-3 text-sm">
        {!pick && <p className="text-muted-foreground">{t("web.design.lead")}</p>}
        {pick && !pick.src && !pick.use && (
          <p className="text-muted-foreground">{t("web.design.noSource")}</p>
        )}
        {design.error && <p className="text-destructive">{design.error.message}</p>}
        {pick && design.isLoading && (
          <p className="text-muted-foreground">{t("web.common.loading")}</p>
        )}
        {info?.instance && info.original && (
          <Segmented
            label={t("web.design.scope")}
            value={target === info.instance ? "instance" : "original"}
            onChange={setScope}
            options={[
              { value: "instance", label: t("web.design.scopeInstance") },
              {
                value: "original",
                label: t("web.design.scopeOriginal", { count: info.original.screens }),
              },
            ]}
          />
        )}
        {target && (
          <Editor
            key={`${target.at}:${rev ?? "main"}`}
            target={target}
            busy={busy}
            onApply={apply}
            onAskAgent={onAskAgent}
          />
        )}
        {status && <StatusLine status={status} onClose={() => setStatus(undefined)} />}
      </div>
    </div>
  );
}

type Target = NonNullable<ReturnType<typeof useDesign>["data"]>["original"] & {};

function Editor({
  target,
  busy,
  onApply,
  onAskAgent,
}: {
  target: Target;
  busy: boolean;
  onApply: (edit: Edit) => void;
  onAskAgent: () => void;
}) {
  const t = useT();
  const { element, variants } = target;
  const [text, setText] = useState(
    element.text.state === "literal" ? (element.text.value ?? "") : "",
  );
  const classes = element.className.state === "literal" ? (element.className.value ?? "") : "";
  const [raw, setRaw] = useState(classes);
  const spacing = useMemo(() => spacingOf(classes), [classes]);
  const hidePadding = "size" in variants;
  const shown = [...spacing.keys()].filter((f) => !(hidePadding && PADDING.includes(f)));
  const addable = FAMILIES.filter((f) => !spacing.has(f) && !(hidePadding && PADDING.includes(f)));
  const computed = (
    <span className="flex items-center gap-2 text-muted-foreground">
      {t("web.design.computed")}
      <Button size="sm" variant="ghost" onClick={onAskAgent}>
        <Bot />
        {t("web.design.askAgent")}
      </Button>
    </span>
  );
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center gap-2">
        <Badge variant="outline">{element.component ? element.name : `<${element.name}>`}</Badge>
        <span className="truncate font-mono text-xs text-muted-foreground">{target.at}</span>
      </div>
      {element.inList && <p className="text-warning">{t("web.design.inList")}</p>}

      <Group title={t("web.design.text")}>
        {element.text.state === "literal" ? (
          <form
            className="flex gap-2"
            onSubmit={(event) => {
              event.preventDefault();
              if (text !== element.text.value) onApply({ kind: "text", text });
            }}
          >
            <Input
              value={text}
              onChange={(event) => setText(event.target.value)}
              aria-label={t("web.design.text")}
            />
            <Button type="submit" size="sm" disabled={busy || text === element.text.value}>
              {t("web.design.apply")}
            </Button>
          </form>
        ) : element.text.state === "computed" ? (
          computed
        ) : (
          <span className="text-muted-foreground">{t("web.design.noText")}</span>
        )}
      </Group>

      {Object.entries(variants).map(([name, variant]) => {
        const prop = element.props[name];
        if (prop?.state === "computed") {
          return (
            <Group key={name} title={name}>
              {computed}
            </Group>
          );
        }
        const current = prop?.state === "literal" ? prop.value : variant.default;
        return (
          <Group key={name} title={name}>
            <select
              aria-label={name}
              className="h-8 rounded-md border bg-background px-2"
              value={current ?? ""}
              disabled={busy}
              onChange={(event) => {
                const value = event.target.value;
                onApply({
                  kind: "prop",
                  name,
                  ...(value && value !== variant.default ? { value } : {}),
                });
              }}
            >
              {!current && <option value="">{t("web.design.default")}</option>}
              {variant.options.map((option) => (
                <option key={option} value={option}>
                  {option}
                  {option === variant.default ? ` (${t("web.design.default")})` : ""}
                </option>
              ))}
            </select>
          </Group>
        );
      })}

      <Group title={t("web.design.spacing")}>
        {element.className.state === "computed" ? (
          computed
        ) : (
          <div className="flex flex-col gap-2">
            {hidePadding && (
              <p className="text-xs text-muted-foreground">{t("web.design.sizeNote")}</p>
            )}
            {shown.map((family) => (
              <div key={family} className="flex items-center gap-2">
                <span className="w-16 font-mono text-xs">{family}</span>
                <select
                  aria-label={t("web.design.spacingOf", { family })}
                  className="h-8 rounded-md border bg-background px-2"
                  value={spacing.get(family)}
                  disabled={busy}
                  onChange={(event) =>
                    onApply({
                      kind: "className",
                      value: withSpacing(classes, family, event.target.value),
                    })
                  }
                >
                  {[...new Set([spacing.get(family) ?? "", ...SCALE])].map((v) => (
                    <option key={v} value={v}>
                      {v}
                    </option>
                  ))}
                </select>
                <Button
                  size="icon-sm"
                  variant="ghost"
                  aria-label={t("web.design.removeSpacing", { family })}
                  disabled={busy}
                  onClick={() =>
                    onApply({ kind: "className", value: withSpacing(classes, family, undefined) })
                  }
                >
                  <X />
                </Button>
              </div>
            ))}
            <select
              aria-label={t("web.design.addSpacing")}
              className="h-8 rounded-md border bg-background px-2 text-muted-foreground"
              value=""
              disabled={busy}
              onChange={(event) => {
                const family = event.target.value as Family;
                if (family)
                  onApply({ kind: "className", value: withSpacing(classes, family, "4") });
              }}
            >
              <option value="">{t("web.design.addSpacing")}</option>
              {addable.map((family) => (
                <option key={family} value={family}>
                  {family}
                </option>
              ))}
            </select>
            <form
              className="flex gap-2"
              onSubmit={(event) => {
                event.preventDefault();
                if (raw !== classes) onApply({ kind: "className", value: raw });
              }}
            >
              <Input
                value={raw}
                onChange={(event) => setRaw(event.target.value)}
                aria-label={t("web.design.classes")}
                placeholder={t("web.design.classes")}
              />
              <Button type="submit" size="sm" variant="outline" disabled={busy || raw === classes}>
                {t("web.design.apply")}
              </Button>
            </form>
          </div>
        )}
      </Group>

      <Group title={t("web.design.order")}>
        {element.moves.up || element.moves.down ? (
          <div className="flex gap-2">
            <Button
              size="sm"
              variant="outline"
              disabled={busy || !element.moves.up}
              onClick={() => onApply({ kind: "move", by: -1 })}
            >
              <ArrowUp />
              {t("web.design.up")}
            </Button>
            <Button
              size="sm"
              variant="outline"
              disabled={busy || !element.moves.down}
              onClick={() => onApply({ kind: "move", by: 1 })}
            >
              <ArrowDown />
              {t("web.design.down")}
            </Button>
          </div>
        ) : (
          <span className="text-muted-foreground">{t("web.design.noOrder")}</span>
        )}
      </Group>
    </div>
  );
}

function Group({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="flex flex-col gap-1.5">
      <h3 className="text-xs font-medium text-muted-foreground">{title}</h3>
      {children}
    </section>
  );
}

function StatusLine({ status, onClose }: { status: Status; onClose: () => void }) {
  const t = useT();
  const text =
    status.kind === "checking"
      ? t("web.design.checking")
      : status.kind === "passed"
        ? t("web.design.passed", { number: status.branch })
        : status.kind === "failed"
          ? t(status.reverted ? "web.design.reverted" : "web.design.failed", {
              issues: status.issues.join("; "),
            })
          : status.message;
  return (
    <div
      role="status"
      className={
        status.kind === "passed"
          ? "flex items-start gap-2 rounded-md bg-muted p-2"
          : status.kind === "checking"
            ? "flex items-start gap-2 rounded-md bg-muted p-2 text-muted-foreground"
            : "flex items-start gap-2 rounded-md bg-destructive/10 p-2 text-destructive"
      }
    >
      {status.kind === "checking" && <LoaderCircle className="size-4 shrink-0 animate-spin" />}
      <span className="flex-1">{text}</span>
      {status.kind !== "checking" && (
        <button type="button" aria-label={t("web.common.close")} onClick={onClose}>
          <X className="size-4" />
        </button>
      )}
    </div>
  );
}
