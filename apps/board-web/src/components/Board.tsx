import { Maximize, Minus, Plus } from "lucide-react";
import {
  type ReactNode,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { Cursors, usePointerShare } from "@/components/Presence";
import { Button } from "@/components/ui/button";
import { useT } from "@/lib/i18n";
import { type Box, fit, type View, zoomAt } from "@/lib/layout";
import { throttle, useFollowed, useRoom } from "@/lib/realtime";

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

  // Presence: share the pointer and the view's center in board units, and follow someone's view.
  const { share } = useRoom();
  const followed = useFollowed();
  const pointer = usePointerShare((clientX, clientY) => {
    const rect = viewport.current?.getBoundingClientRect();
    return {
      x: (clientX - (rect?.left ?? 0) - view.x) / view.k,
      y: (clientY - (rect?.top ?? 0) - view.y) / view.k,
    };
  });
  const shareViewport = useMemo(
    () =>
      throttle((v: View) => {
        const el = viewport.current;
        if (!el) return;
        share({
          viewport: {
            x: (el.clientWidth / 2 - v.x) / v.k,
            y: (el.clientHeight / 2 - v.y) / v.k,
            zoom: v.k,
          },
        });
      }, 100),
    [share],
  );
  useEffect(() => shareViewport(view), [view, shareViewport]);
  const target = followed?.presence.viewport;
  useEffect(() => {
    const el = viewport.current;
    if (!target || !el) return;
    moved.current = true;
    setView({
      k: target.zoom,
      x: el.clientWidth / 2 - target.x * target.zoom,
      y: el.clientHeight / 2 - target.y * target.zoom,
    });
  }, [target]);

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
        onPointerLeave={pointer.onPointerLeave}
        onPointerMove={(event) => {
          pointer.onPointerMove(event);
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
          <Cursors place={(c) => ({ left: c.x, top: c.y })} scale={view.k} />
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
