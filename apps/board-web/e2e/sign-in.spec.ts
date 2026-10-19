import { expect, type Page, test } from "@playwright/test";

// The public server (`stack.ts`): sign-in through the stand-in provider, boss@example.com is an
// admin address, everyone else waits for approval.
const base = () => process.env.BOARD_E2E_PUBLIC_URL ?? "";

test.describe.configure({ mode: "serial" });

async function signIn(page: Page, who: string) {
  await page.goto(`${base()}/signin`);
  await page.getByRole("link", { name: "Sign in with your account" }).click();
  await page.getByRole("link", { name: `${who}@example.com` }).click();
}

test("an admin address signs in, sees the board and signs out", async ({ page }) => {
  await page.goto(`${base()}/`);
  await expect(page).toHaveURL(/\/signin\?returnTo=%2F$/);
  await page.getByRole("link", { name: "Sign in with your account" }).click();
  await page.getByRole("link", { name: "boss@example.com" }).click();
  await expect(page).toHaveURL(`${base()}/`);
  await expect(page.getByRole("heading", { name: "Projects" })).toBeVisible();
  await expect(page.getByText("Boss", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Sign out" }).click();
  await expect(page).toHaveURL(/\/signin$/);
  await page.goto(`${base()}/`);
  await expect(page).toHaveURL(/\/signin/);
});

test("someone outside the allowlist waits for approval", async ({ page }) => {
  await page.goto(`${base()}/signin`);
  await page.getByRole("link", { name: "Sign in with your account" }).click();
  await page.getByRole("link", { name: "eve@example.com" }).click();
  await expect(page.getByText("Waiting for approval")).toBeVisible();
  await expect(page.getByText(/eve@example\.com asked to join/)).toBeVisible();
});

test("a setup link without its token says what to do", async ({ page }) => {
  await page.goto(`${base()}/setup`);
  await expect(page.getByText(/has no token/)).toBeVisible();
});

test("the admin approves a request and the waiting page lets them in", async ({ browser }) => {
  const eve = await (await browser.newContext()).newPage();
  await signIn(eve, "eve");
  await expect(eve.getByText("Waiting for approval")).toBeVisible();

  const boss = await (await browser.newContext()).newPage();
  await signIn(boss, "boss");
  await boss.getByRole("link", { name: "Team" }).click();
  const requests = boss.getByRole("region", { name: "Access requests" });
  await expect(requests.getByText("eve@example.com")).toBeVisible();
  await requests.getByRole("button", { name: "Approve" }).click();
  await expect(
    boss.getByRole("region", { name: "Members" }).getByText("eve@example.com"),
  ).toBeVisible();

  // No reload: the waiting page notices the approval by itself.
  await expect(eve.getByRole("heading", { name: "Projects" })).toBeVisible({ timeout: 20_000 });

  const allowlist = boss.getByRole("region", { name: "Allowlist" });
  await allowlist.getByRole("textbox").fill("team.example");
  await allowlist.getByRole("button", { name: "Add" }).click();
  await expect(allowlist.getByText("team.example", { exact: true })).toBeVisible();
});
