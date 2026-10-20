// A change from branch to version: proposed (as an agent's push would), compared, merged in the
// web tool, then restored. Last in the run, since it moves main.

import { expect, test } from "@playwright/test";

const base = () => process.env.CHARET_E2E_URL ?? "";
const FILE = "templates/recommended/src/pages/tasks.tsx";

test.describe.configure({ mode: "serial" });

test("a branch is compared, merged and restored", async ({ page, request }) => {
  test.setTimeout(240_000);
  const versions = await (await request.get(`${base()}/api/projects/demo/versions`)).json();
  const head = versions.versions[0].commit as string;
  const source = await (
    await request.get(`${base()}/api/projects/demo/file?path=${encodeURIComponent(FILE)}`)
  ).text();
  const changed = source.replace(">Tasks</h1>", ">Tasks for today</h1>");
  expect(changed).not.toBe(source);
  const proposed = await request.post(`${base()}/api/projects/demo/branches`, {
    data: {
      title: "Say which day",
      from: head,
      files: [{ path: FILE, content: Buffer.from(changed).toString("base64") }],
    },
  });
  expect(proposed.status()).toBe(201);
  const { number } = await proposed.json();

  await page.goto(`${base()}/p/demo/branches`);
  await page.getByRole("link", { name: /Say which day/ }).click();
  await expect(page).toHaveURL(new RegExp(`/p/demo/branches/${number}$`));
  await expect(page.getByText("Draft", { exact: true })).toBeVisible();

  // Before and after, each a live preview of its own commit.
  await expect(page.getByText("Before", { exact: true })).toBeVisible();
  const frames = page.locator("figure iframe");
  await expect(frames).toHaveCount(2, { timeout: 180_000 });
  await expect(page.frameLocator("figure iframe").nth(1).getByText("Tasks for today")).toBeVisible({
    timeout: 60_000,
  });
  await expect(
    page.frameLocator("figure iframe").nth(0).getByText("Tasks", { exact: true }),
  ).toBeVisible();

  await page.getByRole("radio", { name: "Code" }).click();
  await expect(page.getByText(FILE).last()).toBeVisible();
  await expect(
    page.getByText("+ ", { exact: false }).filter({ hasText: "Tasks for today" }),
  ).toBeVisible();

  await page.getByRole("button", { name: "Request review" }).click();
  await expect(page.getByText("In review", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Merge" }).click();
  await expect(page.getByText(/Merged as v\d+/)).toBeVisible();

  await page.goto(`${base()}/p/demo/versions`);
  const merged = page.locator("li").filter({ hasText: "Say which day" });
  await expect(merged.getByText("Merged")).toBeVisible();
  await expect(merged.getByRole("link", { name: `#${number}` })).toBeVisible();
  await merged.getByRole("button", { name: /Restore v\d+/ }).click();
  await merged.getByRole("button", { name: /Undo v\d+\?/ }).click();
  await expect(
    page
      .locator("li")
      .filter({ hasText: /Restore v\d+/ })
      .getByText("Restored"),
  ).toBeVisible();
  const restored = await (
    await request.get(`${base()}/api/projects/demo/file?path=${encodeURIComponent(FILE)}`)
  ).text();
  expect(restored).toBe(source);
});
