// Coming into the public server (`stack.ts`) by token, as the specs need it.

import { expect, type Page } from "@playwright/test";

export const base = () => process.env.CHARET_E2E_PUBLIC_URL ?? "";
/** The public server's admin (its owner), as `charet-server admin-token` printed it. */
export const adminToken = () => process.env.CHARET_E2E_ADMIN_TOKEN ?? "";

/** Opens the link an admin hands over (`/?t=<token>`); the page is then signed in. */
export async function enter(page: Page, token: string, at = base()) {
  await page.goto(`${at}/?t=${encodeURIComponent(token)}`);
  await expect(page.getByRole("heading", { name: "Projects" })).toBeVisible();
  // The token left the address bar.
  expect(new URL(page.url()).searchParams.has("t")).toBe(false);
}

/** The admin (signed in on `admin`) adds a member; returns their token. */
export async function addMember(
  admin: Page,
  name: string,
  role: "admin" | "editor" | "commenter" | "viewer",
  at = base(),
): Promise<string> {
  const made = await admin.request.post(`${at}/api/admin/users`, {
    headers: { "x-charet-client": "e2e" },
    data: { name, role },
  });
  expect(made.status()).toBe(201);
  return ((await made.json()) as { token: string }).token;
}
