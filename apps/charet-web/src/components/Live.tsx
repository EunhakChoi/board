import { type ReactNode, useEffect, useRef, useState } from "react";
import { Badge } from "@/components/ui/badge";
import type { TargetKind } from "@/lib/api";
import { useT } from "@/lib/i18n";
import { usePreview } from "@/lib/queries";

/** The real page on the preview origin, scaled to fit. */
export function Live({
  projectKey,
  kind,
  id,
  theme,
  size,
  rev,
  overlay,
}: {
  projectKey: string;
  /** `main` when absent, else `v<n>`, `b<n>` or a commit. */
  rev?: string;
  kind: TargetKind;
  id: string;
  theme: "light" | "dark";
  size: { w: number; h: number };
  /** Drawn over the page (pins, a click layer), given the scale the page is shown at. */
  overlay?: (scale: number) => ReactNode;
}) {
  const t = useT();
  const preview = usePreview(projectKey, rev);
  const holder = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(size.w);
  useEffect(() => {
    const el = holder.current;
    if (!el) return;
    const observer = new ResizeObserver(() => setWidth(el.clientWidth));
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  const base = preview.data?.baseUrl;
  const scale = Math.min(1, width / size.w);
  return (
    <div ref={holder} className="mx-auto w-full" style={{ maxWidth: size.w }}>
      {base ? (
        <div
          className="relative overflow-hidden rounded-lg border bg-background shadow-sm"
          style={{ height: size.h * scale }}
        >
          <iframe
            title={id}
            // Previews on the tool's own address run in an opaque origin (the server says so
            // too): project code must not reach the tool or its session.
            sandbox={
              new URL(base, window.location.href).origin === window.location.origin
                ? "allow-scripts allow-forms allow-popups allow-modals allow-downloads allow-pointer-lock"
                : undefined
            }
            src={`${base}.charet/frame.html?${kind}=${encodeURIComponent(id)}&theme=${theme}`}
            className="origin-top-left border-0"
            style={{ width: size.w, height: size.h, transform: `scale(${scale})` }}
          />
          {overlay?.(scale)}
        </div>
      ) : (
        <Badge variant="secondary">{t("web.build.running")}</Badge>
      )}
    </div>
  );
}
