// A change from branch to version: proposed (as an agent's push would), compared, commented on,
// merged in the web tool, then restored. Last in the run, since it moves main.

import { expect, test } from "@playwright/test";
import { asOwner } from "./people.ts";

const base = () => process.env.CHARET_E2E_URL ?? "";
const FILE = "templates/recommended/src/pages/tasks.tsx";

test.describe.configure({ mode: "serial" });
// The owner, as the admin link or the CLI would sign them in.
test.use({ extraHTTPHeaders: asOwner() });

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

  // A comment on a spot of the new screen stays with the branch, not main.
  await page.getByRole("button", { name: "Comment" }).click();
  await page
    .locator("figure")
    .nth(1)
    .locator("iframe")
    .locator("..")
    .locator("div.absolute")
    .click({
      position: { x: 40, y: 30 },
    });
  await page.getByPlaceholder(/Comment on this spot/).fill("Is today the right word?");
  await page.getByRole("button", { name: "Comment", exact: true }).last().click();
  await expect(page.getByText("Is today the right word?")).toBeVisible();
  await expect(page.locator('[data-testid^="pin-"]')).toHaveCount(1);
  const onBranch = await (
    await request.get(`${base()}/api/projects/demo/comments?branch=${number}`)
  ).json();
  expect(onBranch.comments.map((c: { body: string }) => c.body)).toContain(
    "Is today the right word?",
  );
  const onMain = await (
    await request.get(`${base()}/api/projects/demo/comments?status=all`)
  ).json();
  expect(onMain.comments.map((c: { body: string }) => c.body)).not.toContain(
    "Is today the right word?",
  );

  await page.getByRole("radio", { name: "Code" }).click();
  await expect(page.getByText(FILE).last()).toBeVisible();
  await expect(
    page.getByText("+ ", { exact: false }).filter({ hasText: "Tasks for today" }),
  ).toBeVisible();

  await page.getByRole("button", { name: "Request review" }).click();
  await expect(page.getByText("In review", { exact: true })).toBeVisible();
  // The merge first builds what it gives and opens the screens it reaches.
  await page.getByRole("button", { name: "Merge" }).click();
  await expect(page.getByText(/Merged as v\d+/)).toBeVisible({ timeout: 180_000 });

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

  // An admin hands the merge off to the product repo: Versions shows where it is.
  const merge = (
    await (await request.get(`${base()}/api/projects/demo/versions`)).json()
  ).versions.find((v: { kind: string }) => v.kind === "merge");
  const marked = await request.post(
    `${base()}/api/projects/demo/versions/${merge.number}/handoff`,
    {
      data: { commit: "1234567abcdef" },
    },
  );
  expect(marked.status()).toBe(200);
  await page.reload();
  await expect(page.getByText("In the repo · 1234567")).toBeVisible();
});
