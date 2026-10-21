import { type Block, blocks, type Inline } from "@/lib/markdown";

function Line({ parts }: { parts: Inline[] }) {
  return parts.map((part, i) =>
    part.kind === "bold" ? (
      // biome-ignore lint/suspicious/noArrayIndexKey: parts never reorder
      <strong key={i}>{part.text}</strong>
    ) : part.kind === "code" ? (
      // biome-ignore lint/suspicious/noArrayIndexKey: parts never reorder
      <code key={i} className="rounded bg-background/60 px-1 font-mono text-xs">
        {part.text}
      </code>
    ) : (
      part.text
    ),
  );
}

function Piece({ block }: { block: Block }) {
  switch (block.kind) {
    case "h":
      return (
        <p className="font-semibold">
          <Line parts={block.text} />
        </p>
      );
    case "ul":
    case "ol": {
      const List = block.kind;
      return (
        <List className={`${block.kind === "ul" ? "list-disc" : "list-decimal"} space-y-0.5 pl-5`}>
          {block.items.map((item, i) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: items never reorder
            <li key={i}>
              <Line parts={item} />
            </li>
          ))}
        </List>
      );
    }
    case "pre":
      return (
        <pre className="overflow-x-auto rounded bg-background/60 p-2 font-mono text-xs">
          {block.text}
        </pre>
      );
    default:
      return (
        <p>
          {block.lines.map((line, i) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: lines never reorder
            <span key={i}>
              {i > 0 && <br />}
              <Line parts={line} />
            </span>
          ))}
        </p>
      );
  }
}

/** An agent's answer: the little Markdown it writes, as elements (never raw HTML). */
export function Markdown({ text, className }: { text: string; className?: string }) {
  return (
    <div className={`flex flex-col gap-2 ${className ?? ""}`}>
      {blocks(text).map((block, i) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: blocks never reorder
        <Piece key={i} block={block} />
      ))}
    </div>
  );
}
