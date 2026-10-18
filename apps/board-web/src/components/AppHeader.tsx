import { Moon, Sun } from "lucide-react";
import type { ReactNode } from "react";
import { Link } from "react-router";
import { Button } from "@/components/ui/button";
import { Segmented } from "@/components/ui/segmented";
import { useLocale, useT } from "@/lib/i18n";
import { useUiTheme } from "@/lib/theme";

export function AppHeader({ children, actions }: { children?: ReactNode; actions?: ReactNode }) {
  const t = useT();
  const { locale, setLocale } = useLocale();
  const { dark, toggle } = useUiTheme();
  return (
    <header className="flex h-12 shrink-0 items-center gap-3 border-b bg-background px-3">
      <Link
        to="/"
        className="flex items-center gap-2 font-semibold"
        aria-label={t("web.nav.projects")}
      >
        <img src="/favicon.svg" alt="" className="size-6" />
        <span>board</span>
      </Link>
      <div className="flex min-w-0 flex-1 items-center gap-3">{children}</div>
      <div className="flex items-center gap-2">
        {actions}
        <Segmented
          label={t("web.ui.language")}
          value={locale}
          onChange={setLocale}
          options={[
            { value: "en", label: "EN" },
            { value: "ko", label: "한" },
          ]}
        />
        <Button
          variant="ghost"
          size="icon-sm"
          onClick={toggle}
          aria-label={t("web.ui.dark")}
          aria-pressed={dark}
        >
          {dark ? <Sun /> : <Moon />}
        </Button>
      </div>
    </header>
  );
}
