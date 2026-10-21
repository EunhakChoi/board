// The little Markdown an agent's answer uses: paragraphs, headings, `-` / `1.` lists, **bold**
// and `code`. Parsed into plain data (never HTML), so a reply cannot inject markup.

export type Inline = { kind: "text" | "bold" | "code"; text: string };
export type Block =
  | { kind: "p"; lines: Inline[][] }
  | { kind: "h"; text: Inline[] }
  | { kind: "ul" | "ol"; items: Inline[][] }
  | { kind: "pre"; text: string };

export function inline(text: string): Inline[] {
  const out: Inline[] = [];
  const pattern = /`([^`]+)`|\*\*([^*]+)\*\*/g;
  let at = 0;
  for (const match of text.matchAll(pattern)) {
    if (match.index > at) out.push({ kind: "text", text: text.slice(at, match.index) });
    if (match[1] !== undefined) out.push({ kind: "code", text: match[1] });
    else out.push({ kind: "bold", text: match[2] ?? "" });
    at = match.index + match[0].length;
  }
  if (at < text.length) out.push({ kind: "text", text: text.slice(at) });
  return out;
}

export function blocks(source: string): Block[] {
  const out: Block[] = [];
  const lines = source.replace(/\r\n/g, "\n").split("\n");
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i] ?? "";
    if (line.trim().startsWith("```")) {
      const body: string[] = [];
      for (i++; i < lines.length && !(lines[i] ?? "").trim().startsWith("```"); i++) {
        body.push(lines[i] ?? "");
      }
      out.push({ kind: "pre", text: body.join("\n") });
      continue;
    }
    if (!line.trim()) continue;
    const heading = /^#{1,6}\s+(.*)$/.exec(line) ?? /^\*\*([^*]+)\*\*:?\s*$/.exec(line.trim());
    if (heading) {
      out.push({ kind: "h", text: inline(heading[1] ?? "") });
      continue;
    }
    const item = /^\s*(?:([-*•])|(\d+)[.)])\s+(.*)$/.exec(line);
    if (item) {
      const kind = item[1] ? "ul" : "ol";
      const last = out.at(-1);
      const text = inline(item[3] ?? "");
      if (last && last.kind === kind) last.items.push(text);
      else out.push({ kind, items: [text] });
      continue;
    }
    const last = out.at(-1);
    if (last && last.kind === "p" && lines[i - 1]?.trim()) last.lines.push(inline(line));
    else out.push({ kind: "p", lines: [inline(line)] });
  }
  return out;
}
