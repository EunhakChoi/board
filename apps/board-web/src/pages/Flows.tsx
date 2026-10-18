import { ChevronLeft, ChevronRight, Play, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router";
import { Thumb } from "@/components/Thumb";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { captureImage, type Manifest } from "@/lib/api";
import { useT } from "@/lib/i18n";
import { cardSize } from "@/lib/layout";
import { useProjectContext } from "@/lib/project";
import { useScreenTheme } from "@/lib/theme";
import { Empty } from "./Screens";

type Flow = Manifest["flows"][number];

export function Flows() {
  const t = useT();
  const { key, manifest, captureOf, commentsOn } = useProjectContext();
  const [theme] = useScreenTheme();
  const [playing, setPlaying] = useState<Flow | undefined>();
  if (!manifest) return <Empty text={t("web.common.loading")} />;
  if (manifest.flows.length === 0) return <Empty text={t("web.flows.empty")} />;
  const sizeOf = (id: string) =>
    manifest.screens.find((s) => s.id === id)?.size ?? { w: 1440, h: 900 };
  return (
    <div className="flex-1 overflow-auto bg-canvas p-6">
      <div className="flex flex-col gap-10">
        {manifest.flows.map((flow) => (
          <section key={flow.id} data-testid={`flow-${flow.id}`} className="flex flex-col gap-3">
            <div className="flex items-center gap-3">
              <h2 className="text-lg font-semibold">{flow.title}</h2>
              <Button size="sm" variant="outline" onClick={() => setPlaying(flow)}>
                <Play />
                {t("web.flows.play")}
              </Button>
            </div>
            <ol className="flex items-start gap-4 overflow-x-auto pb-2">
              {flow.steps.map((step, index) => {
                const size = cardSize(sizeOf(step.id), 160);
                return (
                  // biome-ignore lint/suspicious/noArrayIndexKey: a flow may visit the same screen twice
                  <li key={`${step.id}-${index}`} className="flex shrink-0 items-center gap-4">
                    {index > 0 && <ChevronRight className="size-5 text-muted-foreground" />}
                    <div className="flex flex-col gap-2" style={{ width: size.w }}>
                      {step.planned ? (
                        <div
                          className="flex flex-col items-center justify-center gap-1 rounded-lg border-2 border-dashed p-3 text-center text-xs text-muted-foreground"
                          style={{ height: size.h }}
                        >
                          <Badge variant="outline">{t("web.flows.planned")}</Badge>
                          {step.note}
                        </div>
                      ) : (
                        <Link
                          to={`/p/${key}/t/screen/${step.id}${theme === "dark" ? "?theme=dark" : ""}`}
                          style={{ height: size.h }}
                        >
                          <Thumb
                            projectKey={key}
                            capture={captureOf("screen", step.id, theme)}
                            title={step.title}
                            openComments={
                              commentsOn("screen", step.id).filter((c) => c.status === "open")
                                .length
                            }
                            className="size-full"
                          />
                        </Link>
                      )}
                      <span className="text-sm">
                        <span className="text-muted-foreground">{index + 1}. </span>
                        {step.title}
                      </span>
                    </div>
                  </li>
                );
              })}
            </ol>
          </section>
        ))}
      </div>
      {playing && <Player flow={playing} onClose={() => setPlaying(undefined)} />}
    </div>
  );
}

/** Steps one at a time, full screen; arrow keys move, Escape closes. */
function Player({ flow, onClose }: { flow: Flow; onClose: () => void }) {
  const t = useT();
  const { key, captureOf } = useProjectContext();
  const [theme] = useScreenTheme();
  const [index, setIndex] = useState(0);
  const step = flow.steps[index];
  const move = useCallback(
    (by: number) => setIndex((i) => Math.min(flow.steps.length - 1, Math.max(0, i + by))),
    [flow.steps.length],
  );
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "ArrowRight") move(1);
      if (event.key === "ArrowLeft") move(-1);
      if (event.key === "Escape") onClose();
    };
    addEventListener("keydown", onKey);
    return () => removeEventListener("keydown", onKey);
  }, [move, onClose]);
  if (!step) return null;
  const capture = step.planned ? undefined : captureOf("screen", step.id, theme);
  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label={flow.title}
      className="fixed inset-0 z-50 flex flex-col bg-background/95"
    >
      <div className="flex items-center gap-3 border-b px-4 py-2">
        <span className="font-semibold">{flow.title}</span>
        <span className="text-sm text-muted-foreground">
          {t("web.flows.step", { n: index + 1, total: flow.steps.length })} · {step.title}
        </span>
        <span className="flex-1" />
        <Button size="icon-sm" variant="ghost" onClick={onClose} aria-label={t("web.common.close")}>
          <X />
        </Button>
      </div>
      <div className="flex min-h-0 flex-1 items-center justify-center gap-4 p-6">
        <Button
          size="icon"
          variant="outline"
          onClick={() => move(-1)}
          disabled={index === 0}
          aria-label={t("web.flows.prev")}
        >
          <ChevronLeft />
        </Button>
        <div className="flex h-full min-w-0 flex-1 items-center justify-center">
          {capture ? (
            <img
              src={captureImage(key, capture)}
              alt={step.title}
              className="max-h-full max-w-full rounded-lg border shadow"
            />
          ) : (
            <div className="flex flex-col items-center gap-2 rounded-xl border-2 border-dashed p-12 text-muted-foreground">
              <Badge variant="outline">
                {step.planned ? t("web.flows.planned") : t("web.capture.none")}
              </Badge>
              {step.note}
            </div>
          )}
        </div>
        <Button
          size="icon"
          variant="outline"
          onClick={() => move(1)}
          disabled={index === flow.steps.length - 1}
          aria-label={t("web.flows.next")}
        >
          <ChevronRight />
        </Button>
      </div>
    </div>
  );
}
