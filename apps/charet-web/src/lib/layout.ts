// Pure layout math for the board: where screens sit, how they connect, how the view fits.

export interface Size {
  w: number;
  h: number;
}

export interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Placed<T> extends Box {
  item: T;
}

/** Thumbnail height on the board; widths follow each screen's aspect ratio. */
export const CARD_HEIGHT = 220;
export const CARD_GAP = 56;
export const ROW_GAP = 96;
export const LABEL_WIDTH = 180;
/** Space under a card for its title. */
export const CAPTION = 44;

export function cardSize(size: Size, height = CARD_HEIGHT): Size {
  return { w: Math.round((height * size.w) / Math.max(1, size.h)), h: height };
}

/** Rows of cards, one row per group, in the given order. */
export function layoutRows<T>(
  rows: { items: T[] }[],
  sizeOf: (item: T) => Size,
): { cards: Placed<T>[]; rowY: number[] } {
  const cards: Placed<T>[] = [];
  const rowY: number[] = [];
  let y = 0;
  for (const row of rows) {
    rowY.push(y);
    let x = LABEL_WIDTH;
    let tallest = 0;
    for (const item of row.items) {
      const size = cardSize(sizeOf(item));
      cards.push({ item, x, y, ...size });
      x += size.w + CARD_GAP;
      tallest = Math.max(tallest, size.h);
    }
    y += (tallest || CARD_HEIGHT) + CAPTION + ROW_GAP;
  }
  return { cards, rowY };
}

/** An SVG path from the right edge of `from` to the left edge of `to`, curving around when needed. */
export function connectionPath(from: Box, to: Box): string {
  const start = { x: from.x + from.w, y: from.y + from.h / 2 };
  const end = { x: to.x, y: to.y + to.h / 2 };
  const reach = Math.max(48, Math.abs(end.x - start.x) / 2);
  return `M ${start.x} ${start.y} C ${start.x + reach} ${start.y}, ${end.x - reach} ${end.y}, ${end.x} ${end.y}`;
}

export interface View {
  x: number;
  y: number;
  k: number;
}

export const MIN_ZOOM = 0.1;
export const MAX_ZOOM = 4;

export function clampZoom(k: number): number {
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, k));
}

/** The view that shows `content` inside a viewport, with padding, never zooming past 1. */
export function fit(content: Box, viewport: Size, padding = 48): View {
  if (content.w <= 0 || content.h <= 0) return { x: padding, y: padding, k: 1 };
  const k = clampZoom(
    Math.min(1, (viewport.w - padding * 2) / content.w, (viewport.h - padding * 2) / content.h),
  );
  return {
    k,
    x: (viewport.w - content.w * k) / 2 - content.x * k,
    y: Math.max(padding, (viewport.h - content.h * k) / 2) - content.y * k,
  };
}

/** Zooms by `factor` keeping the point (`px`, `py`, in viewport pixels) still. */
export function zoomAt(view: View, factor: number, px: number, py: number): View {
  const k = clampZoom(view.k * factor);
  const ratio = k / view.k;
  return { k, x: px - (px - view.x) * ratio, y: py - (py - view.y) * ratio };
}

export function bounds(boxes: Box[]): Box {
  if (boxes.length === 0) return { x: 0, y: 0, w: 0, h: 0 };
  const x = Math.min(...boxes.map((b) => b.x));
  const y = Math.min(...boxes.map((b) => b.y));
  const right = Math.max(...boxes.map((b) => b.x + b.w));
  const bottom = Math.max(...boxes.map((b) => b.y + b.h));
  return { x, y, w: right - x, h: bottom - y };
}

/** The smallest element containing the point (the first on ties). */
export function elementAt<T extends Box>(elements: T[], x: number, y: number): T | undefined {
  let best: T | undefined;
  for (const e of elements) {
    if (x < e.x || x > e.x + e.w || y < e.y || y > e.y + e.h) continue;
    if (!best || e.w * e.h < best.w * best.h) best = e;
  }
  return best;
}

/** `path:line:col` → its parts. */
export function parseSource(
  src: string,
): { path: string; line: number; column: number } | undefined {
  const match = /^(.*):(\d+):(\d+)$/.exec(src);
  if (!match?.[1]) return undefined;
  return { path: match[1], line: Number(match[2]), column: Number(match[3]) };
}

/** A repository path from an app-relative source path (`charet.json` `app`). */
export function repoPath(app: string, path: string): string {
  const prefix = app.replace(/^\.?\/?/, "").replace(/\/$/, "");
  return prefix ? `${prefix}/${path}` : path;
}
