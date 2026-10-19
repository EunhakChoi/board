import { Check, Copy } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useT } from "@/lib/i18n";

/** A secret or one-time link right after it was made; it is not shown again. */
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
          aria-label={t("web.links.copy")}
          onClick={() => {
            void navigator.clipboard?.writeText(value);
            setCopied(true);
          }}
        >
          {copied ? <Check /> : <Copy />}
        </Button>
      </div>
    </div>
  );
}
