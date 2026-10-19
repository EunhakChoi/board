import { execFileSync, spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
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

test("a share link lets a guest in under a name until it is revoked", async ({ browser }) => {
  const boss = await (await browser.newContext()).newPage();
  await signIn(boss, "boss");
  const created = await boss.request.post(`${base()}/api/projects`, {
    headers: { "x-board-client": "e2e" },
    data: { key: "shared", title: "Shared" },
  });
  expect(created.status()).toBe(201);
  await boss.goto(`${base()}/p/shared/screens`);
  await boss.getByRole("button", { name: "Share" }).click();
  const panel = boss.getByRole("region", { name: "Share links" });
  await panel.getByRole("radio", { name: "Comment" }).click();
  await panel.getByRole("textbox", { name: "Label" }).fill("review");
  await panel.getByRole("button", { name: "Create link" }).click();
  const url = await panel.getByRole("textbox", { name: "Link address" }).inputValue();
  expect(url).toContain("/l/");

  const guest = await (await browser.newContext()).newPage();
  await guest.goto(url);
  await expect(guest.getByText("Shared")).toBeVisible();
  await guest.getByRole("textbox", { name: "Your name" }).fill("Dana");
  await guest.getByRole("button", { name: "Continue" }).click();
  await expect(guest).toHaveURL(/\/p\/shared\/screens$/);
  await expect(guest.getByText("Dana", { exact: true })).toBeVisible();
  await expect(guest.getByRole("button", { name: "Capture" })).toHaveCount(0);
  await expect(guest.getByRole("button", { name: "Share" })).toHaveCount(0);

  await panel.getByRole("button", { name: "Revoke review" }).click();
  await expect(panel.getByText("Revoked")).toBeVisible();
  await guest.reload();
  await expect(guest).toHaveURL(/\/signin/);
});

test("board login is approved in the browser and the CLI acts as that member", async ({
  browser,
}) => {
  const root = resolve(import.meta.dirname, "../../..");
  const config = mkdtempSync(join(tmpdir(), "board-cli-login-"));
  const env = { ...process.env, BOARD_CONFIG_DIR: config, BOARD_URL: base() };
  const cli = join(root, "apps/board-cli/src/main.ts");
  const login = spawn(process.execPath, [cli, "login", "--name", "e2e-laptop"], { env });
  let stderr = "";
  login.stderr.on("data", (chunk) => {
    stderr += chunk;
  });
  const exited = new Promise<number | null>((done) => login.on("exit", done));
  const printed = () => /\b([A-Z]{4}-[A-Z]{4})\b/.exec(stderr)?.[1];
  await expect.poll(printed, { message: `board login said: ${stderr}` }).toBeTruthy();
  const code = printed() ?? "";

  const boss = await (await browser.newContext()).newPage();
  await signIn(boss, "boss");
  await boss.goto(`${base()}/device?code=${code}`);
  await expect(boss.getByText(/“e2e-laptop” act as boss@example\.com/)).toBeVisible();
  await boss.getByRole("button", { name: "Approve" }).click();
  await expect(boss.getByText("Approved.")).toBeVisible();
  expect(await exited).toBe(0);

  const whoami = execFileSync(process.execPath, [cli, "whoami"], { env }).toString();
  expect(whoami).toContain("Boss <boss@example.com>, admin");
  await boss.goto(`${base()}/account`);
  await expect(boss.getByText("e2e-laptop")).toBeVisible();
  rmSync(config, { recursive: true, force: true });
});
