import assert from "node:assert/strict";
import { test } from "node:test";
import {
  bounds,
  CARD_HEIGHT,
  cardSize,
  connectionPath,
  elementAt,
  fit,
  LABEL_WIDTH,
  layoutRows,
  parseSource,
  repoPath,
  zoomAt,
} from "./layout.ts";

test("cards keep aspect ratio and rows stack", () => {
  assert.deepEqual(cardSize({ w: 1440, h: 900 }), { w: 352, h: CARD_HEIGHT });
  const { cards, rowY } = layoutRows(
    [
      {
        items: [
          { w: 1440, h: 900 },
          { w: 390, h: 844 },
        ],
      },
      { items: [{ w: 800, h: 800 }] },
    ],
    (s) => s,
  );
  assert.equal(cards[0]?.x, LABEL_WIDTH);
  assert.equal(cards[1]?.x, LABEL_WIDTH + 352 + 56);
  assert.equal(rowY[1], CARD_HEIGHT + 44 + 96);
  assert.equal(cards[2]?.y, rowY[1]);
});

test("fit centers and never zooms past 1", () => {
  const view = fit({ x: 0, y: 0, w: 400, h: 200 }, { w: 1000, h: 800 });
  assert.equal(view.k, 1);
  assert.equal(view.x, 300);
  const big = fit({ x: 0, y: 0, w: 4000, h: 1000 }, { w: 1000, h: 800 });
  assert.ok(big.k < 1);
});

test("zooming keeps the pointer still", () => {
  const before = { x: 10, y: 20, k: 1 };
  const after = zoomAt(before, 2, 110, 120);
  // The charet point under the pointer stays the same.
  assert.equal((110 - before.x) / before.k, (110 - after.x) / after.k);
  assert.equal((120 - before.y) / before.k, (120 - after.y) / after.k);
});

test("geometry helpers", () => {
  assert.deepEqual(
    bounds([
      { x: 10, y: 10, w: 5, h: 5 },
      { x: 0, y: 20, w: 5, h: 5 },
    ]),
    { x: 0, y: 10, w: 15, h: 15 },
  );
  assert.match(
    connectionPath({ x: 0, y: 0, w: 10, h: 10 }, { x: 100, y: 0, w: 10, h: 10 }),
    /^M 10 5 C/,
  );
  const els = [
    { x: 0, y: 0, w: 100, h: 100, id: "big" },
    { x: 10, y: 10, w: 10, h: 10, id: "small" },
  ];
  assert.equal(elementAt(els, 15, 15)?.id, "small");
  assert.equal(elementAt(els, 50, 50)?.id, "big");
  assert.equal(elementAt(els, 500, 500), undefined);
  assert.deepEqual(parseSource("src/a:b.tsx:12:3"), { path: "src/a:b.tsx", line: 12, column: 3 });
  assert.equal(repoPath("apps/web/", "src/a.tsx"), "apps/web/src/a.tsx");
  assert.equal(repoPath(".", "src/a.tsx"), "src/a.tsx");
});
