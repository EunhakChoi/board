import { FileText } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useParams, useSearchParams } from "react-router";
import { Input } from "@/components/ui/input";
import { useT } from "@/lib/i18n";
import { useFile, useFiles } from "@/lib/queries";
import { cn } from "@/lib/utils";
import { Empty } from "./Screens";

export function Code() {
  const t = useT();
  const key = useParams().key ?? "";
  const [params, setParams] = useSearchParams();
  const selected = params.get("path") ?? undefined;
  const line = Number(params.get("line") ?? "0");
  const files = useFiles(key);
  const file = useFile(key, selected);
  const [filter, setFilter] = useState("");
  const paths = useMemo(
    () => (files.data?.paths ?? []).filter((p) => p.toLowerCase().includes(filter.toLowerCase())),
    [files.data, filter],
  );
  const highlighted = useRef<HTMLTableRowElement>(null);
  useEffect(() => {
    if (file.data) highlighted.current?.scrollIntoView({ block: "center" });
  }, [file.data]);

  return (
    <div className="flex min-h-0 flex-1">
      <aside className="flex w-80 shrink-0 flex-col border-r">
        <div className="border-b p-2">
          <Input
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder={t("web.code.filter")}
          />
        </div>
        <ul className="flex-1 overflow-y-auto py-1 text-sm">
          {paths.map((path) => (
            <li key={path}>
              <button
                type="button"
                onClick={() => setParams({ path })}
                className={cn(
                  "flex w-full items-center gap-2 truncate px-3 py-1 text-left font-mono text-xs hover:bg-accent",
                  path === selected && "bg-accent font-semibold",
                )}
                title={path}
              >
                <FileText className="size-3.5 shrink-0 text-muted-foreground" />
                <span className="truncate">{path}</span>
              </button>
            </li>
          ))}
        </ul>
      </aside>
      <section className="min-w-0 flex-1 overflow-auto">
        {!selected && <Empty text={t("web.code.pick")} />}
        {selected && file.isLoading && <Empty text={t("web.common.loading")} />}
        {selected && file.error && (
          <Empty text={t("web.common.error", { message: file.error.message })} />
        )}
        {selected && file.data && file.data.text === undefined && (
          <Empty text={t("web.code.binary")} />
        )}
        {file.data?.text !== undefined && (
          <div>
            <div className="sticky top-0 border-b bg-background px-4 py-2 font-mono text-xs">
              {selected}
            </div>
            <table className="w-full border-collapse font-mono text-xs">
              <tbody>
                {file.data.text.split("\n").map((text, index) => {
                  const number = index + 1;
                  return (
                    <tr
                      // biome-ignore lint/suspicious/noArrayIndexKey: lines are positional
                      key={index}
                      ref={number === line ? highlighted : undefined}
                      className={cn(number === line && "bg-warning/15")}
                    >
                      <td className="w-12 select-none px-3 text-right align-top text-muted-foreground">
                        {number}
                      </td>
                      <td className="whitespace-pre px-3">{text || " "}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </section>
    </div>
  );
}
