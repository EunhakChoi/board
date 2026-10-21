// Hand edits (story 05): pick an element on a screen, change its text, spacing, a variant and its
// order from the Design panel. Each change lands on the person's draft, is checked, and shows on
// the draft's picture; undo puts the file back. Nothing is merged here.

import { expect, type Page, test } from "@playwright/test";
import { asOwner } from "./people.ts";

const base = () => process.env.CHARET_E2E_URL ?? "";
const FILE = "templates/recommended/src/pages/tasks.tsx";

test.describe.configure({ mode: "serial" });
test.use({ extraHTTPHeaders: asOwner() });

type Box = { x: number; y: number; w: number; h: number; tag: string; text: string };

/** Clicks the element of the picture shown, by its box in screen pixels. */
async function pickOn(page: Page, size: { w: number; h: number }, element: Box) {
  const capture = page.getByTestId("capture");
  await expect(capture.locator("img")).toBeVisible();
  const box = await capture.boundingBox();
  if (!box) throw new Error("no picture");
  await page.mouse.click(
    box.x + ((element.x + element.w / 2) / size.w) * box.width,
    box.y + ((element.y + element.h / 2) / size.h) * box.height,
  );
}

test("hand edits change a draft and are checked, shown and undone", async ({ page, request }) => {
  test.setTimeout(420_000);
  const manifest = await (await request.get(`${base()}/api/projects/demo/manifest`)).json();
  const size = manifest.manifest.screens.find((s: { id: string }) => s.id === "tasks-list").size;
  await request.post(`${base()}/api/projects/demo/captures`, {
    data: { targets: ["tasks-list"], themes: ["light"], force: true },
  });
  const onMain: Box[] = await (
    await request.get(`${base()}/api/projects/demo/captures/screen/tasks-list/light/elements`)
  ).json();
  const heading = onMain.find((e) => e.tag === "h1");
  if (!heading) throw new Error("no heading in the element map");

  await page.goto(`${base()}/p/demo/t/screen/tasks-list`);
  await page
    .getByRole("radiogroup", { name: "Side panel" })
    .getByRole("radio", { name: "Design" })
    .click();
  await expect(page.getByText(/change its text, variant, spacing or order/)).toBeVisible();

  // Text: a new draft starts with the first edit.
  await pickOn(page, size, heading);
  const text = page.getByRole("textbox", { name: "Text", exact: true });
  await expect(text).toHaveValue("Tasks");
  await text.fill("Today");
  await page.getByRole("button", { name: "Apply" }).first().click();
  await expect(page.getByText(/Saved on draft #\d+\./)).toBeVisible({ timeout: 240_000 });
  const number = Number(
    /#(\d+)/.exec((await page.getByText(/Saved on draft #\d+\./).textContent()) ?? "")?.[1],
  );
  // The draft's picture replaces main's, and the heading is picked again on it.
  await expect(page.getByRole("textbox", { name: "Text", exact: true })).toHaveValue("Today", {
    timeout: 120_000,
  });

  // Spacing on the heading.
  await page.getByRole("combobox", { name: "Add spacing…" }).selectOption("mt");
  await expect(page.getByRole("combobox", { name: "Spacing mt" })).toHaveValue("4", {
    timeout: 240_000,
  });

  // The "Open" button: an instance of the shared Button, with its size variant.
  // The draft as rendered now (asking again while it builds).
  let looked: { results?: { elements: Box[] }[] } = {};
  await expect
    .poll(
      async () => {
        looked = await (
          await request.post(`${base()}/api/projects/demo/check`, {
            data: { rev: `b${number}`, targets: ["tasks-list"], image: true },
          })
        ).json();
        return !!looked.results;
      },
      { timeout: 240_000, intervals: [2000] },
    )
    .toBe(true);
  const open = (looked.results?.[0]?.elements ?? []).find(
    (e) => e.tag === "button" && e.text === "Open",
  );
  if (!open) throw new Error("no Open button on the draft");
  await pickOn(page, size, open);
  // shadcn's Button renders `<Comp>` (a variable), so only this instance is editable.
  await expect(page.getByText(/^Button$/)).toBeVisible();
  await expect(page.getByRole("textbox", { name: "Text", exact: true })).toHaveValue("Open");
  // `variant` is computed from state here: the agent takes that one.
  await expect(page.getByRole("button", { name: "Ask the agent" })).toBeVisible();
  await page.getByRole("combobox", { name: "size" }).selectOption("sm");
  await expect(page.getByText(/Saved on draft #\d+\./)).toBeVisible({ timeout: 240_000 });
  await expect(page.getByRole("combobox", { name: "size" })).toHaveValue("sm", {
    timeout: 120_000,
  });

  // Order: Open before All.
  await page.getByRole("button", { name: "Move up" }).click();
  await expect(page.getByRole("button", { name: "Move down" })).toBeEnabled({ timeout: 240_000 });
  const fileAt = async () =>
    (
      await request.get(
        `${base()}/api/projects/demo/file?rev=b${number}&path=${encodeURIComponent(FILE)}`,
      )
    ).text();
  let source = await fileAt();
  expect(source).toContain(">Today</h1>");
  expect(source).toContain('className="text-2xl font-semibold mt-4"');
  expect(source).toContain('size="sm"');
  expect(source.indexOf("Open")).toBeLessThan(source.indexOf("All"));

  // Undo the move: the file is back as the size edit left it.
  await page.getByRole("button", { name: "Undo" }).click();
  await expect
    .poll(
      async () => {
        const now = await fileAt();
        return now.indexOf("All") < now.indexOf("Open");
      },
      { timeout: 60_000 },
    )
    .toBe(true);
  source = await fileAt();
  expect(source).toContain('size="sm"');

  // Main is untouched; the draft goes to review like any other.
  const main = await (
    await request.get(`${base()}/api/projects/demo/file?path=${encodeURIComponent(FILE)}`)
  ).text();
  expect(main).toContain(">Tasks</h1>");
  await page.getByRole("button", { name: "Request review" }).click();
  await expect(page.getByRole("button", { name: "Request review" })).toHaveCount(0);
  await page.getByRole("link", { name: `Open draft #${number}` }).click();
  await expect(page.getByText("In review", { exact: true })).toBeVisible();
  // Archived so later specs start from main as it was.
  await page.getByRole("button", { name: "Archive" }).click();
});
