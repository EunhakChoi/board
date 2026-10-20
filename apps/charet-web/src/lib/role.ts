import { useParams } from "react-router";
import type { Role } from "./api.ts";
import { useMe, useProject } from "./queries.ts";

const RANK: Record<Role, number> = { viewer: 0, commenter: 1, editor: 2, admin: 3 };

/** Whether the person looking has at least `role` (hides controls the server would refuse). */
export function useCan(role: Role): boolean {
  const me = useMe();
  const key = useParams().key ?? "";
  const project = useProject(key, me.data?.kind === "visitor" && key !== "");
  if (me.data?.kind === "visitor") {
    // Visitors do what this project lets them: look, or also comment.
    if (role === "viewer") return true;
    return role === "commenter" && project.data?.visitorAccess === "comment";
  }
  const mine = me.data?.role;
  return mine !== undefined && RANK[mine] >= RANK[role];
}
