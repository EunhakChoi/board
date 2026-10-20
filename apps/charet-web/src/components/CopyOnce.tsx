import { Check, Copy } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useT } from "@/lib/i18n";

/**
 * Puts `text` on the clipboard. Pages on a plain-HTTP address (an IP on the LAN) have no
 * clipboard API, so a hidden text area and the older copy command stand in.
 */
export async function copyText(text: string): Promise<boolean> {
  if (navigator.clipboard && window.isSecureContext) {
    return navigator.clipboard.writeText(text).then(
      () => true,
      () => false,
    );
  }
  const area = document.createElement("textarea");
  area.value = text;
  area.style.position = "fixed";
  area.style.opacity = "0";
  document.body.append(area);
  area.select();
  const ok = document.execCommand("copy");
  area.remove();
  return ok;
}

/** A secret right after it was made; it is not shown again. */
export function CopyOnce({ value, label, note }: { value: string; label: string; note: string }) {
  const t = useT();
  const [copied, setCopied] = useState(false);
  return (
    <div className="flex flex-col gap-1 rounded-md bg-muted p-2 text-left">
      <p className="text-xs">{note}</p>
      <div className="flex gap-2">
        <Input readOnly value={value} aria-label={label} onFocus={(e) => e.target.select()} />
        <Button
          size="sm"
          variant="outline"
          aria-label={t("web.common.copy")}
          onClick={() => copyText(value).then(setCopied)}
        >
          {copied ? <Check /> : <Copy />}
        </Button>
      </div>
    </div>
  );
}
