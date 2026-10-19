import { MousePointer2, Radio, X } from "lucide-react";
import { type CSSProperties, useEffect, useMemo, useRef, useState } from "react";
import { useLocation, useNavigate } from "react-router";
import { Button } from "@/components/ui/button";
import { useT } from "@/lib/i18n";
import { colorOf, type Presence, throttle, useFollowed, usePeople, useRoom } from "@/lib/realtime";

/** Peers' cursors (and chat bubbles) on this view, placed by `place` in the surface's units. */
export function Cursors({
  place,
  scale = 1,
}: {
  place: (point: { x: number; y: number }) => CSSProperties;
  scale?: number;
}) {
  const { pathname } = useLocation();
  const people = usePeople();
  return (
    <>
      {people
        .filter(({ presence }) => presence.view === pathname && presence.cursor)
        .map(({ peer, presence }) => {
          const color = colorOf(peer.person);
          const cursor = presence.cursor as { x: number; y: number };
          return (
            <div
              key={peer.person}
              data-testid={`cursor-${peer.name}`}
              className="pointer-events-none absolute z-40"
              style={place(cursor)}
            >
              <div className="origin-top-left" style={{ transform: `scale(${1 / scale})` }}>
                <MousePointer2 className="size-5 drop-shadow" style={{ color, fill: color }} />
                <div
                  className="ml-4 max-w-56 rounded-md rounded-tl-none px-2 py-0.5 text-xs font-medium whitespace-pre-wrap text-white shadow"
                  style={{ background: color }}
                >
                  {presence.chat ? presence.chat : peer.name}
                </div>
              </div>
            </div>
          );
        })}
    </>
  );
}

/** Outlines of what peers picked on this view (capture pixels → `place`). */
export function Selections({
  place,
}: {
  place: (box: { x: number; y: number; w: number; h: number }) => CSSProperties;
}) {
  const { pathname } = useLocation();
  const people = usePeople();
  return (
    <>
      {people
        .filter(({ presence }) => presence.view === pathname && presence.selection)
        .map(({ peer, presence }) => (
          <div
            key={peer.person}
            data-testid={`selection-${peer.name}`}
            className="pointer-events-none absolute border-2"
            style={{
              ...place(presence.selection as { x: number; y: number; w: number; h: number }),
              borderColor: colorOf(peer.person),
            }}
          >
            <span
              className="absolute -top-5 left-0 rounded px-1 text-xs text-white"
              style={{ background: colorOf(peer.person) }}
            >
              {peer.name}
            </span>
          </div>
        ))}
    </>
  );
}

/** Shares where this tab is, and goes where the followed person goes. */
export function PresenceSync() {
  const { pathname, search } = useLocation();
  const navigate = useNavigate();
  const { share, connected } = useRoom();
  const followed = useFollowed();
  // biome-ignore lint/correctness/useExhaustiveDependencies: share again after reconnecting
  useEffect(() => {
    share({ view: pathname, cursor: null, selection: null });
  }, [pathname, share, connected]);
  const view = followed?.presence.view;
  useEffect(() => {
    if (view && view !== pathname) navigate(`${view}${search}`);
  }, [view, pathname, search, navigate]);
  return null;
}

/** "Following Ann · Stop", and the spotlight switch. */
export function FollowBar() {
  const t = useT();
  const { follow, share } = useRoom();
  const followed = useFollowed();
  const [spotlight, setSpotlight] = useState(false);
  const toggle = () => {
    setSpotlight(!spotlight);
    share({ spotlight: !spotlight });
  };
  return (
    <>
      <Button
        size="sm"
        variant={spotlight ? "default" : "ghost"}
        onClick={toggle}
        aria-pressed={spotlight}
        title={t("web.presence.spotlightHint")}
      >
        <Radio />
        {t(spotlight ? "web.presence.spotlightOn" : "web.presence.spotlight")}
      </Button>
      {followed && (
        <div
          role="status"
          className="fixed bottom-4 left-1/2 z-50 flex -translate-x-1/2 items-center gap-2 rounded-full border-2 bg-background py-1 pr-1 pl-3 text-sm shadow-lg"
          style={{ borderColor: colorOf(followed.peer.person) }}
        >
          {t(followed.presence.spotlight ? "web.presence.presenting" : "web.presence.following", {
            name: followed.peer.name,
          })}
          <Button
            size="icon-sm"
            variant="ghost"
            onClick={() => follow(null)}
            aria-label={t("web.presence.stop")}
          >
            <X />
          </Button>
        </div>
      )}
    </>
  );
}

/** Press `/` to say something short next to your cursor; it fades after a few seconds. */
export function CursorChat() {
  const t = useT();
  const { share } = useRoom();
  const [open, setOpen] = useState<{ x: number; y: number } | null>(null);
  const [text, setText] = useState("");
  const mouse = useRef({ x: 0, y: 0 });
  const clear = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => {
    const move = (event: MouseEvent) => {
      mouse.current = { x: event.clientX, y: event.clientY };
    };
    const key = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement;
      if (target.closest("input, textarea, [contenteditable]")) return;
      if (event.key === "/") {
        event.preventDefault();
        setOpen({ ...mouse.current });
      }
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("keydown", key);
    return () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("keydown", key);
    };
  }, []);
  if (!open) return null;
  return (
    <input
      // biome-ignore lint/a11y/noAutofocus: the person just pressed / to type here
      autoFocus
      aria-label={t("web.presence.chat")}
      placeholder={t("web.presence.chat")}
      maxLength={120}
      value={text}
      onChange={(event) => setText(event.target.value)}
      onBlur={() => setOpen(null)}
      onKeyDown={(event) => {
        if (event.key === "Escape") setOpen(null);
        if (event.key === "Enter" && text.trim()) {
          share({ chat: text.trim() });
          if (clear.current) clearTimeout(clear.current);
          clear.current = setTimeout(() => share({ chat: null }), 6000);
          setText("");
          setOpen(null);
        }
      }}
      className="fixed z-50 w-56 rounded-md border bg-background px-2 py-1 text-sm shadow-lg outline-none"
      style={{ left: open.x + 16, top: open.y + 8 }}
    />
  );
}

/** Shares the pointer over a surface, in its own units (`toLocal` maps client pixels). */
export function usePointerShare(
  toLocal: (clientX: number, clientY: number) => { x: number; y: number },
) {
  const { share } = useRoom();
  const send = useMemo(() => throttle((presence: Presence) => share(presence), 50), [share]);
  return {
    onPointerMove: (event: React.PointerEvent) => {
      const { x, y } = toLocal(event.clientX, event.clientY);
      send({ cursor: { x: Math.round(x), y: Math.round(y) } });
    },
    onPointerLeave: () => send({ cursor: null }),
  };
}
