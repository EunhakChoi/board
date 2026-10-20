import type { Key } from "@charet/i18n";
import { useCallback } from "react";
import { useT } from "./i18n.tsx";

/** As many as `web.visitor.adjective.*` and `web.visitor.animal.*` (and the server's lists). */
const WORDS = 16;

/**
 * The made-up name of a visitor in the page's language. The server picks the same words from the
 * id (`visitor_name`), so everyone sees the same visitor under the same name.
 */
export function useVisitorName() {
  const t = useT();
  return useCallback(
    (id: string) => {
      const pick = (from: number) => {
        const value = Number.parseInt(id.slice(from, from + 4), 16);
        return Number.isNaN(value) ? 0 : value % WORDS;
      };
      return t("web.visitor.name", {
        adjective: t(`web.visitor.adjective.${pick(0)}` as Key),
        animal: t(`web.visitor.animal.${pick(4)}` as Key),
      });
    },
    [t],
  );
}
