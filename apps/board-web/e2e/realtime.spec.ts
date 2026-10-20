import { expect, type Page, test } from "@playwright/test";
import { adminToken, base, enter } from "./people.ts";

// Two people on one board of the public server: each sees the other and their comments live.

test("people on the same board see each other and new comments without reloading", async ({
  browser,
}) => {
  const boss = await (await browser.newContext()).newPage();
  await enter(boss, adminToken());
  await boss.goto(`${base()}/p/demo/screens`);

  // Someone without a token: a visitor under a made-up name.
  const visitor = await (await browser.newContext()).newPage();
  await visitor.goto(`${base()}/p/demo/screens`);
  await expect(visitor.getByRole("button", { name: "Enter token" })).toBeVisible();

  const here = (page: Page) => page.getByRole("list", { name: "Here now" });
  await expect(here(boss).getByTitle(/^[A-Z][a-z]+ [A-Z][a-z]+ \(visitor\)$/)).toBeVisible();
  await expect(here(visitor).getByTitle("Admin")).toBeVisible();

  await visitor.goto(`${base()}/p/demo/t/screen/tasks-list`);
  const added = await boss.request.post(`${base()}/api/projects/demo/comments`, {
    headers: { "x-board-client": "e2e" },
    data: { kind: "screen", target: "tasks-list", at: { x: 10, y: 10 }, body: "Live from Boss" },
  });
  expect(added.status()).toBe(201);
  await expect(visitor.getByText("Live from Boss")).toBeVisible();

  // Following: the visitor's view goes wherever the admin goes.
  await visitor.getByRole("button", { name: "Follow Admin" }).click();
  await boss.getByRole("link", { name: "Flows" }).click();
  await expect(visitor).toHaveURL(/\/p\/demo\/flows/);
  await expect(visitor.getByRole("status")).toContainText("Following Admin");
});
