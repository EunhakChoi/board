// Line diff for the code tab of a branch: what a file was where the branch started, against what
// it is now. Small files only (an LCS table); larger ones are shown whole.

export interface DiffLine {
  kind: "same" | "add" | "del";
  text: string;
  /** Line number before (same, del) and after (same, add). */
  before?: number;
  after?: number;
}

/** A run of lines to show, or a count of unchanged lines folded away. */
export type DiffRow = DiffLine | { kind: "fold"; count: number };

const MAX_CELLS = 4_000_000;

/** The lines of `before` and `after` in order, or `undefined` when the files are too large. */
export function diffLines(before: string, after: string): DiffLine[] | undefined {
  const a = before === "" ? [] : before.split("\n");
  const b = after === "" ? [] : after.split("\n");
  // Common head and tail first: most edits touch a few lines in the middle.
  let head = 0;
  while (head < a.length && head < b.length && a[head] === b[head]) head++;
  let tail = 0;
  while (
    tail < a.length - head &&
    tail < b.length - head &&
    a[a.length - 1 - tail] === b[b.length - 1 - tail]
  )
    tail++;
  const midA = a.slice(head, a.length - tail);
  const midB = b.slice(head, b.length - tail);
  if ((midA.length + 1) * (midB.length + 1) > MAX_CELLS) return undefined;
  const n = midA.length;
  const m = midB.length;
  // lcs[i][j] = longest common run of midA[i..] and midB[j..].
  const lcs = Array.from({ length: n + 1 }, () => new Uint32Array(m + 1));
  for (let i = n - 1; i >= 0; i--) {
    const row = lcs[i] as Uint32Array;
    const next = lcs[i + 1] as Uint32Array;
    for (let j = m - 1; j >= 0; j--) {
      row[j] =
        midA[i] === midB[j]
          ? (next[j + 1] as number) + 1
          : Math.max(next[j] as number, row[j + 1] as number);
    }
  }
  const out: DiffLine[] = [];
  for (let k = 0; k < head; k++)
    out.push({ kind: "same", text: a[k] as string, before: k + 1, after: k + 1 });
  let i = 0;
  let j = 0;
  while (i < n || j < m) {
    if (i < n && j < m && midA[i] === midB[j]) {
      out.push({
        kind: "same",
        text: midA[i] as string,
        before: head + i + 1,
        after: head + j + 1,
      });
      i++;
      j++;
    } else if (
      i < n &&
      (j === m ||
        ((lcs[i + 1] as Uint32Array)[j] as number) >= ((lcs[i] as Uint32Array)[j + 1] as number))
    ) {
      out.push({ kind: "del", text: midA[i] as string, before: head + i + 1 });
      i++;
    } else {
      out.push({ kind: "add", text: midB[j] as string, after: head + j + 1 });
      j++;
    }
  }
  for (let k = 0; k < tail; k++) {
    out.push({
      kind: "same",
      text: a[a.length - tail + k] as string,
      before: a.length - tail + k + 1,
      after: b.length - tail + k + 1,
    });
  }
  return out;
}

/** Keeps `context` unchanged lines around each change and folds the rest. */
export function fold(lines: DiffLine[], context = 3): DiffRow[] {
  const near = new Uint8Array(lines.length);
  lines.forEach((line, index) => {
    if (line.kind === "same") return;
    for (
      let k = Math.max(0, index - context);
      k <= Math.min(lines.length - 1, index + context);
      k++
    )
      near[k] = 1;
  });
  const rows: DiffRow[] = [];
  let hidden = 0;
  lines.forEach((line, index) => {
    if (near[index]) {
      if (hidden) rows.push({ kind: "fold", count: hidden });
      hidden = 0;
      rows.push(line);
    } else hidden++;
  });
  if (hidden) rows.push({ kind: "fold", count: hidden });
  return rows;
}
