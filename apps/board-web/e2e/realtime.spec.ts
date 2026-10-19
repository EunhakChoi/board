import { expect, type Page, test } from "@playwright/test";

// Two people on one board of the public server: each sees the other and their comments live.
const base = () => process.env.BOARD_E2E_PUBLIC_URL ?? "";

async function signIn(page: Page, who: string) {
  await page.goto(`${base()}/signin`);
  await page.getByRole("link", { name: "Sign in with your account" }).click();
  await page.getByRole("link", { name: `${who}@example.com` }).click();
}

test("people on the same board see each other and new comments without reloading", async ({
  browser,
}) => {
  const boss = await (await browser.newContext()).newPage();
  await signIn(boss, "boss");
  await boss.goto(`${base()}/p/demo/screens`);
  await boss.getByRole("button", { name: "Share" }).click();
  const panel = boss.getByRole("region", { name: "Share links" });
  await panel.getByRole("radio", { name: "Comment" }).click();
  await panel.getByRole("textbox", { name: "Label" }).fill("live");
  await panel.getByRole("button", { name: "Create link" }).click();
  const url = await panel.getByRole("textbox", { name: "Link address" }).inputValue();
  await boss.keyboard.press("Escape");

  const guest = await (await browser.newContext()).newPage();
  await guest.goto(url);
  await guest.getByRole("textbox", { name: "Your name" }).fill("Robin");
  await guest.getByRole("button", { name: "Continue" }).click();
  await expect(guest).toHaveURL(/\/p\/demo\/screens$/);

  const here = (page: Page) => page.getByRole("list", { name: "Here now" });
  await expect(here(boss).getByTitle("Robin (guest)")).toBeVisible();
  await expect(here(guest).getByTitle("Boss")).toBeVisible();

  await guest.goto(`${base()}/p/demo/t/screen/tasks-list`);
  const added = await boss.request.post(`${base()}/api/projects/demo/comments`, {
    headers: { "x-board-client": "e2e" },
    data: { kind: "screen", target: "tasks-list", at: { x: 10, y: 10 }, body: "Live from Boss" },
  });
  expect(added.status()).toBe(201);
  await expect(guest.getByText("Live from Boss")).toBeVisible();

  // Following: the guest's view goes wherever Boss goes.
  await guest.getByRole("button", { name: "Follow Boss" }).click();
  await boss.getByRole("link", { name: "Flows" }).click();
  await expect(guest).toHaveURL(/\/p\/demo\/flows/);
  await expect(guest.getByRole("status")).toContainText("Following Boss");
});
