import { Laptop } from "lucide-react";
import { useT } from "@/lib/i18n";
import { colorOf, type Peer, usePeople, useRoom } from "@/lib/realtime";

function initials(name: string): string {
  const parts = name.trim().split(/\s+/);
  return ((parts[0]?.[0] ?? "") + (parts[1]?.[0] ?? "")).toUpperCase() || "?";
}

export function Avatar({ peer, size = 7 }: { peer: Pick<Peer, "person" | "name">; size?: 6 | 7 }) {
  const box = size === 6 ? "size-6 text-xs" : "size-7 text-xs";
  return (
    <span
      className={`${box} flex items-center justify-center rounded-full font-medium text-white ring-2 ring-background`}
      style={{ background: colorOf(peer.person) }}
    >
      {initials(peer.name)}
    </span>
  );
}

/** Who else is in this project now, and who works from their own checkout. */
export function Avatars() {
  const t = useT();
  const people = usePeople();
  const { working, me, following, follow } = useRoom();
  const others = working.filter((w) => w.person !== me?.person);
  return (
    <div className="flex items-center gap-2">
      {others.length > 0 && (
        <span
          className="flex items-center gap-1 text-xs text-muted-foreground"
          title={others.map((w) => w.name).join(", ")}
        >
          <Laptop className="size-3.5" />
          {t("web.presence.working", {
            names: others.map((w) => w.name).join(", "),
          })}
        </span>
      )}
      {people.length > 0 && (
        <ul aria-label={t("web.presence.here")} className="flex -space-x-2">
          {people.slice(0, 5).map(({ peer }) => (
            <li
              key={peer.person}
              title={
                peer.kind === "visitor" ? `${peer.name} (${t("web.auth.visitor")})` : peer.name
              }
            >
              <button
                type="button"
                className={`rounded-full ${following === peer.person ? "ring-2 ring-primary" : ""}`}
                aria-label={t(
                  following === peer.person ? "web.presence.following" : "web.presence.follow",
                  { name: peer.name },
                )}
                aria-pressed={following === peer.person}
                onClick={() => follow(following === peer.person ? null : peer.person)}
              >
                <Avatar peer={peer} />
              </button>
            </li>
          ))}
          {people.length > 5 && (
            <li className="flex size-7 items-center justify-center rounded-full bg-muted text-xs ring-2 ring-background">
              +{people.length - 5}
            </li>
          )}
        </ul>
      )}
    </div>
  );
}
