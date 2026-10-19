import { expect, type Page, test } from "@playwright/test";

// Invite links on the public server (`stack.ts`): no provider account needed to join.
const base = () => process.env.BOARD_E2E_PUBLIC_URL ?? "";

async function signIn(page: Page, who: string) {
  await page.goto(`${base()}/signin`);
  await page.getByRole("link", { name: "Sign in with your account" }).click();
  await page.getByRole("link", { name: `${who}@example.com` }).click();
  await expect(page.getByRole("heading", { name: "Projects" })).toBeVisible();
}

test("an invite lets a new member in by name, and they add their phone", async ({ browser }) => {
  const boss = await (await browser.newContext()).newPage();
  await signIn(boss, "boss");
  await boss.getByRole("link", { name: "Team" }).click();
  const invites = boss.getByRole("region", { name: "Invites" });
  await invites.getByRole("radio", { name: "Commenter" }).click();
  await invites.getByRole("textbox", { name: "Label" }).fill("Kim, design");
  await invites.getByRole("button", { name: "Create invite" }).click();
  const url = await invites.getByRole("textbox", { name: "Invite link" }).inputValue();
  expect(url).toContain("/i/");
  expect(url).not.toMatch(/\d+\.\d+\.\d+\.\d+/);

  const kim = await (await browser.newContext()).newPage();
  await kim.goto(url);
  await expect(kim.getByText("Enter your name to join as Commenter.")).toBeVisible();
  await kim.getByRole("textbox", { name: "Your name" }).fill("Kim");
  await kim.getByRole("button", { name: "Join" }).click();
  await expect(kim.getByRole("heading", { name: "Projects" })).toBeVisible();
  await expect(kim.getByRole("link", { name: "Kim" })).toBeVisible();

  // The link worked once.
  const late = await (await browser.newContext()).newPage();
  await late.goto(url);
  await expect(late.getByText(/was used, expired or withdrawn/)).toBeVisible();

  // Kim signs in on another device with a link from their account page.
  await kim.getByRole("link", { name: "Kim" }).click();
  await kim.getByRole("button", { name: "Create sign-in link" }).click();
  const own = await kim.getByRole("textbox", { name: "Invite link" }).inputValue();
  const phone = await (await browser.newContext()).newPage();
  await phone.goto(own);
  await phone.getByRole("button", { name: "Continue as Kim" }).click();
  await expect(phone.getByRole("heading", { name: "Projects" })).toBeVisible();
  await expect(phone.getByRole("link", { name: "Kim" })).toBeVisible();
});
