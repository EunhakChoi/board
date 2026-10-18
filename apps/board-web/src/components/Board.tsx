import { Maximize, Minus, Plus } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { useT } from "@/lib/i18n";
import { type Box, fit, type View, zoomAt } from "@/lib/layout";

/**
 * A pan and zoom surface. Children are laid out in board coordinates; `content` is their bounds,
 * used to fit the view. Scroll pans, Ctrl/⌘ + scroll (or a pinch) zooms, dragging the background pans.
 */
export function Board({ content, children }: { content: Box; children: ReactNode }) {
  const t = useT();
  const viewport = useRef<HTMLDivElement>(null);
  const [view, setView] = useState<View>({ x: 0, y: 0, k: 1 });
  // Keeps fitting as the content grows (captures, manifest) until the user moves the view.
  const moved = useRef(false);
  const drag = useRef<{ x: number; y: number; view: View } | null>(null);

  const fitView = useCallback(() => {
    const el = viewport.current;
    if (!el) return;
    setView(fit(content, { w: el.clientWidth, h: el.clientHeight }));
  }, [content]);

  useLayoutEffect(() => {
    if (!moved.current && content.w > 0) fitView();
  }, [content, fitView]);

  // Wheel needs a non-passive listener to stop the page from zooming.
  useEffect(() => {
    const el = viewport.current;
    if (!el) return;
    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      moved.current = true;
      const rect = el.getBoundingClientRect();
      if (event.ctrlKey || event.metaKey) {
        const factor = Math.exp(-event.deltaY * 0.0015);
        setView((v) => zoomAt(v, factor, event.clientX - rect.left, event.clientY - rect.top));
      } else {
        setView((v) => ({ ...v, x: v.x - event.deltaX, y: v.y - event.deltaY }));
      }
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, []);

  const zoomBy = (factor: number) => {
    const el = viewport.current;
    if (!el) return;
    moved.current = true;
    setView((v) => zoomAt(v, factor, el.clientWidth / 2, el.clientHeight / 2));
  };

  return (
    <div className="relative flex-1 overflow-hidden bg-canvas">
      <div
        ref={viewport}
        data-testid="board"
        className="absolute inset-0 cursor-grab touch-none select-none active:cursor-grabbing"
        onPointerDown={(event) => {
          // Only the background drags; cards handle their own clicks.
          if (
            event.target !== event.currentTarget &&
            !(event.target as HTMLElement).dataset.boardLayer
          )
            return;
          moved.current = true;
          drag.current = { x: event.clientX, y: event.clientY, view };
          event.currentTarget.setPointerCapture(event.pointerId);
        }}
        onPointerMove={(event) => {
          const start = drag.current;
          if (!start) return;
          setView({
            ...start.view,
            x: start.view.x + event.clientX - start.x,
            y: start.view.y + event.clientY - start.y,
          });
        }}
        onPointerUp={() => {
          drag.current = null;
        }}
      >
        <div
          data-board-layer="true"
          className="absolute top-0 left-0 origin-top-left"
          style={{ transform: `translate(${view.x}px, ${view.y}px) scale(${view.k})` }}
        >
          {children}
        </div>
      </div>
      <div className="absolute right-3 bottom-3 flex items-center gap-1 rounded-lg border bg-background p-1 shadow-sm">
        <Button
          variant="ghost"
          size="icon-sm"
          onClick={() => zoomBy(1 / 1.25)}
          aria-label={t("web.board.zoomOut")}
        >
          <Minus />
        </Button>
        <span className="w-12 text-center text-xs tabular-nums text-muted-foreground">
          {Math.round(view.k * 100)}%
        </span>
        <Button
          variant="ghost"
          size="icon-sm"
          onClick={() => zoomBy(1.25)}
          aria-label={t("web.board.zoomIn")}
        >
          <Plus />
        </Button>
        <Button variant="ghost" size="icon-sm" onClick={fitView} aria-label={t("web.board.fit")}>
          <Maximize />
        </Button>
      </div>
      <p className="pointer-events-none absolute bottom-4 left-4 text-xs text-muted-foreground">
        {t("web.board.hint")}
      </p>
    </div>
  );
}
