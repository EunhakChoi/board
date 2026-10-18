import { useMemo } from "react";
import { Link } from "react-router";
import { Board } from "@/components/Board";
import { Thumb } from "@/components/Thumb";
import type { Manifest } from "@/lib/api";
import { useT } from "@/lib/i18n";
import { bounds, CAPTION, connectionPath, LABEL_WIDTH, layoutRows } from "@/lib/layout";
import { useProjectContext } from "@/lib/project";
import { useScreenTheme } from "@/lib/theme";

type Screen = Manifest["screens"][number];

export function Screens() {
  const t = useT();
  const { key, manifest, captureOf, commentsOn } = useProjectContext();
  const [theme] = useScreenTheme();
  const layout = useMemo(() => {
    if (!manifest) return undefined;
    const rows = manifest.pages
      .map((page) => ({ page, items: manifest.screens.filter((s) => s.page === page.id) }))
      .filter((row) => row.items.length > 0);
    const { cards, rowY } = layoutRows(rows, (screen: Screen) => screen.size);
    const byId = new Map(cards.map((card) => [card.item.id, card]));
    const content = bounds([
      { x: 0, y: 0, w: LABEL_WIDTH, h: 0 },
      ...cards.map((c) => ({ ...c, h: c.h + CAPTION })),
    ]);
    return { rows, rowY, cards, byId, content };
  }, [manifest]);

  if (!manifest || !layout) return <Empty text={t("web.common.loading")} />;
  if (manifest.screens.length === 0) return <Empty text={t("web.screens.empty")} />;
  const query = theme === "dark" ? "?theme=dark" : "";
  return (
    <Board content={layout.content}>
      <svg
        className="pointer-events-none absolute top-0 left-0 overflow-visible"
        width={1}
        height={1}
        aria-hidden="true"
      >
        <defs>
          <marker
            id="arrow"
            viewBox="0 0 10 10"
            refX="9"
            refY="5"
            markerWidth="7"
            markerHeight="7"
            orient="auto"
          >
            <path d="M 0 0 L 10 5 L 0 10 z" className="fill-muted-foreground" />
          </marker>
        </defs>
        {manifest.connections.map((connection) => {
          const from = layout.byId.get(connection.from);
          const to = layout.byId.get(connection.to);
          if (!from || !to) return null;
          return (
            <path
              key={`${connection.from}-${connection.to}`}
              d={connectionPath(from, to)}
              className={
                connection.kind === "error" ? "stroke-destructive" : "stroke-muted-foreground"
              }
              fill="none"
              strokeWidth={2}
              strokeDasharray={connection.kind === "auto" ? "6 6" : undefined}
              markerEnd="url(#arrow)"
            />
          );
        })}
      </svg>
      {layout.rows.map((row, index) => (
        <div
          key={row.page.id}
          className="absolute left-0 flex flex-col gap-1 pr-6"
          style={{ top: layout.rowY[index], width: LABEL_WIDTH }}
        >
          <span className="text-sm font-semibold">{row.page.title}</span>
          <span className="truncate font-mono text-xs text-muted-foreground">{row.page.route}</span>
        </div>
      ))}
      {layout.cards.map((card) => {
        const screen = card.item;
        const open = commentsOn("screen", screen.id).filter((c) => c.status === "open").length;
        return (
          <Link
            key={screen.id}
            to={`/p/${key}/t/screen/${screen.id}${query}`}
            data-testid={`screen-${screen.id}`}
            className="group absolute flex flex-col gap-2 outline-none"
            style={{ left: card.x, top: card.y, width: card.w }}
          >
            <div style={{ height: card.h }}>
              <Thumb
                projectKey={key}
                capture={captureOf("screen", screen.id, theme)}
                title={screen.title}
                openComments={open}
                className="size-full transition-shadow group-hover:ring-2 group-hover:ring-ring group-focus-visible:ring-2 group-focus-visible:ring-ring"
              />
            </div>
            <span className="truncate text-sm font-medium">{screen.title}</span>
          </Link>
        );
      })}
    </Board>
  );
}

export function Empty({ text }: { text: string }) {
  return (
    <div className="flex flex-1 items-center justify-center p-8 text-sm text-muted-foreground">
      {text}
    </div>
  );
}
