import { useCallback, useEffect, useState } from "react";
import { useSearchParams } from "react-router";
import type { Theme } from "./api.ts";

const STORAGE = "charet.ui-theme";

/** The tool's own light/dark mode (not the screens'), following the system until chosen. */
export function useUiTheme() {
  const [dark, setDark] = useState<boolean>(() => {
    try {
      const saved = localStorage.getItem(STORAGE);
      if (saved) return saved === "dark";
    } catch {}
    return matchMedia("(prefers-color-scheme: dark)").matches;
  });
  useEffect(() => {
    document.documentElement.classList.toggle("dark", dark);
  }, [dark]);
  const toggle = useCallback(() => {
    setDark((current) => {
      try {
        localStorage.setItem(STORAGE, current ? "light" : "dark");
      } catch {}
      return !current;
    });
  }, []);
  return { dark, toggle };
}

/** The theme screens are shown in, kept in the URL (`?theme=dark`) so links carry it. */
export function useScreenTheme(): [Theme, (theme: Theme) => void] {
  const [params, setParams] = useSearchParams();
  const theme: Theme = params.get("theme") === "dark" ? "dark" : "light";
  const set = useCallback(
    (next: Theme) =>
      setParams(
        (current) => {
          const copy = new URLSearchParams(current);
          if (next === "dark") copy.set("theme", "dark");
          else copy.delete("theme");
          return copy;
        },
        { replace: true },
      ),
    [setParams],
  );
  return [theme, set];
}
