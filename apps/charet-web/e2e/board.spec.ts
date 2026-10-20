import { expect, test } from "@playwright/test";

const base = () => process.env.CHARET_E2E_URL ?? "";

test.describe.configure({ mode: "serial" });

test("captures screens onto the charet", async ({ page }) => {
  await page.goto(`${base()}/`);
  await page.getByRole("link", { name: /demo/ }).click();
  await expect(page).toHaveURL(/\/p\/demo\/screens$/);
  await expect(page.getByTestId("screen-tasks-list")).toBeVisible();
  await page.getByRole("button", { name: "Capture", exact: true }).click();
  await expect(page.getByTestId("screen-tasks-list").locator("img")).toBeVisible({
    timeout: 45_000,
  });
  await expect(page.getByTestId("screen-tasks-error").locator("img")).toBeVisible();
});

test("comments on an element, replies and resolves", async ({ page }) => {
  await page.goto(`${base()}/p/demo/t/screen/tasks-list`);
  const capture = page.getByTestId("capture");
  await expect(capture.locator("img")).toBeVisible();
  const box = await capture.boundingBox();
  if (!box) throw new Error("no capture");
  await page.mouse.click(box.x + box.width * 0.4, box.y + box.height * 0.2);
  const composer = page.getByPlaceholder(/Leave a comment/);
  await composer.fill("The list needs more spacing");
  await composer.press("Control+Enter");

  const thread = page
    .locator("[data-testid^=comment-]")
    .filter({ hasText: "The list needs more spacing" });
  await expect(thread).toBeVisible();
  await expect(page.locator("[data-testid^=pin-]")).toHaveCount(1);
  await thread.getByPlaceholder(/Reply/).fill("Agreed");
  await thread.getByRole("button", { name: "Reply" }).click();
  await expect(thread.getByText("Agreed")).toBeVisible();
  await thread.getByRole("button", { name: "Resolve" }).click();
  await expect(thread.getByText("Resolved", { exact: true })).toBeVisible();

  // The charet counts only open comments.
  await page.goto(`${base()}/p/demo/screens`);
  await expect(page.getByTestId("screen-tasks-list")).toBeVisible();
});

test("plays a flow step by step", async ({ page }) => {
  await page.goto(`${base()}/p/demo/flows`);
  const flow = page.locator("[data-testid^=flow-]").first();
  await flow.getByRole("button", { name: "Play" }).click();
  const player = page.getByRole("dialog");
  await expect(player).toContainText("Step 1");
  await page.keyboard.press("ArrowRight");
  await expect(player).toContainText("Step 2");
  await page.keyboard.press("Escape");
  await expect(player).toBeHidden();
});

test("shows a source file at a line", async ({ page }) => {
  await page.goto(`${base()}/p/demo/code?path=templates/recommended/src/main.tsx&line=2`);
  await expect(page.getByText("templates/recommended/src/main.tsx").last()).toBeVisible();
  await expect(page.locator("tr.bg-warning\\/15")).toHaveCount(1);
});
