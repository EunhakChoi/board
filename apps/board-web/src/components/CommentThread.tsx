import { Check, RotateCcw, Unlink } from "lucide-react";
import { useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { type Comment, HttpError, snapshotImage } from "@/lib/api";
import { useLocale, useT } from "@/lib/i18n";
import { useAnswer } from "@/lib/queries";
import { isGuest, useCan } from "@/lib/role";
import { cn } from "@/lib/utils";

/** One comment with its replies, a reply box, and resolve / reopen. */
export function CommentThread({
  projectKey,
  comment,
  active,
  onFocus,
}: {
  projectKey: string;
  comment: Comment;
  active: boolean;
  onFocus: () => void;
}) {
  const t = useT();
  const { locale } = useLocale();
  const [reply, setReply] = useState("");
  const answer = useAnswer(projectKey);
  const canComment = useCan("commenter");
  const changed = answer.error instanceof HttpError && answer.error.code === "changed";
  const when = (iso: string) =>
    new Date(iso).toLocaleString(locale === "ko" ? "ko-KR" : "en-US", {
      dateStyle: "medium",
      timeStyle: "short",
    });
  const act = (action: "replies" | "resolve" | "reopen", body: string) =>
    answer.mutate({ comment, action, body }, { onSuccess: () => setReply("") });
  return (
    <article
      data-testid={`comment-${comment.id}`}
      onClick={onFocus}
      onKeyDown={(event) => {
        if (event.key === "Enter") onFocus();
      }}
      className={cn(
        "flex flex-col gap-2 rounded-lg border p-3 text-sm transition-colors",
        active && "border-ring bg-accent/50",
        comment.status === "resolved" && "opacity-70",
      )}
    >
      <header className="flex items-center gap-2">
        <span className="font-medium">{comment.author.name}</span>
        {isGuest(comment.author) && <Badge variant="outline">{t("web.links.guest")}</Badge>}
        <span className="text-xs text-muted-foreground">{when(comment.createdAt)}</span>
        <span className="flex-1" />
        <Badge variant={comment.status === "open" ? "info" : "secondary"}>
          {t(comment.status === "open" ? "web.comments.open" : "web.comments.resolved")}
        </Badge>
      </header>
      <p className="whitespace-pre-wrap">{comment.body}</p>
      {comment.anchor.detached && (
        <div className="flex flex-col gap-1">
          <p className="flex items-center gap-1 text-xs text-warning">
            <Unlink className="size-3" />
            {t("web.comments.detached")}
          </p>
          <img
            src={snapshotImage(projectKey, comment)}
            alt=""
            className="rounded border"
            loading="lazy"
          />
        </div>
      )}
      {comment.replies.map((r) => (
        <div key={r.id} className="border-l-2 pl-3">
          <p className="text-xs text-muted-foreground">
            <span className="font-medium text-foreground">{r.author.name}</span>
            {isGuest(r.author) && ` (${t("web.links.guest")})`}{" "}
            {r.kind === "resolve" && t("web.comments.resolvedBy")}
            {r.kind === "reopen" && t("web.comments.reopenedBy")} · {when(r.createdAt)}
          </p>
          {r.body && <p className="whitespace-pre-wrap">{r.body}</p>}
        </div>
      ))}
      {changed && <p className="text-xs text-warning">{t("web.comments.changed")}</p>}
      {answer.error && !changed && (
        <p className="text-xs text-destructive">{answer.error.message}</p>
      )}
      {canComment && (
        <>
          <Textarea
            value={reply}
            onChange={(event) => setReply(event.target.value)}
            placeholder={t("web.comments.replyPlaceholder")}
            className="min-h-9"
            onClick={(event) => event.stopPropagation()}
          />
          <div className="flex justify-end gap-2">
            {comment.status === "open" ? (
              <Button
                size="sm"
                variant="outline"
                disabled={answer.isPending}
                onClick={() => act("resolve", reply)}
              >
                <Check />
                {t("web.comments.resolve")}
              </Button>
            ) : (
              <Button
                size="sm"
                variant="outline"
                disabled={answer.isPending}
                onClick={() => act("reopen", reply)}
              >
                <RotateCcw />
                {t("web.comments.reopen")}
              </Button>
            )}
            <Button
              size="sm"
              disabled={!reply.trim() || answer.isPending}
              onClick={() => act("replies", reply)}
            >
              {t("web.comments.reply")}
            </Button>
          </div>
        </>
      )}
    </article>
  );
}
