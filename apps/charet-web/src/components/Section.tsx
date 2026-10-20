import { Badge } from "@/components/ui/badge";

/** A titled block of the settings page. */
export function Section({
  title,
  count,
  lead,
  children,
}: {
  title: string;
  count?: number | undefined;
  lead?: string;
  children: React.ReactNode;
}) {
  return (
    <section aria-label={title}>
      <h2 className="mb-2 flex items-center gap-2 text-sm font-medium text-muted-foreground">
        {title}
        {count !== undefined && <Badge variant="secondary">{count}</Badge>}
      </h2>
      {lead && <p className="mb-2 text-sm text-muted-foreground">{lead}</p>}
      <div className="divide-y rounded-xl border">{children}</div>
    </section>
  );
}
