import type * as React from "react";
import { cn } from "@/lib/utils";

/** A small set of mutually exclusive options (theme, view mode). */
function Segmented<T extends string>({
  value,
  options,
  onChange,
  className,
  label,
}: {
  value: T;
  options: { value: T; label: React.ReactNode }[];
  onChange: (value: T) => void;
  className?: string;
  label?: string;
}) {
  return (
    <div
      data-slot="segmented"
      role="radiogroup"
      aria-label={label}
      className={cn("inline-flex h-8 items-center rounded-md bg-muted p-0.5 text-sm", className)}
    >
      {options.map((option) => (
        // biome-ignore lint/a11y/useSemanticElements: a segmented control is a radio group of buttons
        <button
          key={option.value}
          type="button"
          role="radio"
          aria-checked={option.value === value}
          onClick={() => onChange(option.value)}
          className={cn(
            "inline-flex h-7 items-center gap-1.5 rounded-sm px-2.5 text-muted-foreground transition-colors [&_svg]:size-3.5",
            option.value === value && "bg-background text-foreground shadow-sm",
          )}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

export { Segmented };
