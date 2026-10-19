import type { Role } from "./api.ts";
import { useMe } from "./queries.ts";

const RANK: Record<Role, number> = { viewer: 0, commenter: 1, editor: 2, admin: 3 };

/** Whether the person looking has at least `role` (hides controls the server would refuse). */
export function useCan(role: Role): boolean {
  const me = useMe();
  const mine = me.data?.role;
  return mine !== undefined && RANK[mine] >= RANK[role];
}

/** People who came in through a share link are marked next to their name. */
export function isGuest(person: { id: string }): boolean {
  return person.id.startsWith("guest:");
}
