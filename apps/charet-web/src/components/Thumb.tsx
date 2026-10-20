import { CircleAlert, Clock, MessageSquare } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { type Capture, captureImage } from "@/lib/api";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/** A capture picture with its state: missing, out of date, problems, open comments. */
export function Thumb({
  projectKey,
  capture,
  title,
  openComments,
  className,
}: {
  projectKey: string;
  capture: Capture | undefined;
  title: string;
  openComments: number;
  className?: string;
}) {
  const t = useT();
  return (
    <div
      className={cn(
        "relative overflow-hidden rounded-lg border bg-background shadow-sm",
        className,
      )}
    >
      {capture ? (
        <img
          src={captureImage(projectKey, capture)}
          alt={title}
          draggable={false}
          className="size-full object-cover object-top"
          loading="lazy"
        />
      ) : (
        <div className="flex size-full items-center justify-center text-xs text-muted-foreground">
          {t("web.capture.none")}
        </div>
      )}
      <div className="absolute top-2 right-2 flex gap-1">
        {capture?.stale && (
          <Badge variant="warning" title={t("web.capture.stale", { count: 1 })}>
            <Clock />
          </Badge>
        )}
        {capture && capture.issues.length > 0 && (
          <Badge variant="destructive" title={capture.issues.join("\n")}>
            <CircleAlert />
            {capture.issues.length}
          </Badge>
        )}
        {openComments > 0 && (
          <Badge variant="info">
            <MessageSquare />
            {openComments}
          </Badge>
        )}
      </div>
    </div>
  );
}
