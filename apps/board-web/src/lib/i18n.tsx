import { type Key, type Locale, t as translate } from "@board/i18n";
import { createContext, type ReactNode, useCallback, useContext, useMemo, useState } from "react";

const STORAGE = "board.locale";

function initialLocale(): Locale {
  try {
    const saved = localStorage.getItem(STORAGE);
    if (saved === "en" || saved === "ko") return saved;
  } catch {}
  return navigator.language.toLowerCase().startsWith("ko") ? "ko" : "en";
}

const LocaleContext = createContext<{ locale: Locale; setLocale: (locale: Locale) => void }>({
  locale: "en",
  setLocale: () => {},
});

export function LocaleProvider({ children }: { children: ReactNode }) {
  const [locale, set] = useState<Locale>(initialLocale);
  const setLocale = useCallback((next: Locale) => {
    set(next);
    document.documentElement.lang = next;
    try {
      localStorage.setItem(STORAGE, next);
    } catch {}
  }, []);
  const value = useMemo(() => ({ locale, setLocale }), [locale, setLocale]);
  return <LocaleContext value={value}>{children}</LocaleContext>;
}

export function useLocale() {
  return useContext(LocaleContext);
}

/** `t(key, params)` in the current locale. */
export function useT() {
  const { locale } = useContext(LocaleContext);
  return useCallback(
    (key: Key, params: Record<string, string | number> = {}) => {
      const strings = Object.fromEntries(Object.entries(params).map(([k, v]) => [k, String(v)]));
      return translate(key, strings, locale);
    },
    [locale],
  );
}
