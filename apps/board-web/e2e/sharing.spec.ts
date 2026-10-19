import { expect, test } from "@playwright/test";

// A local board shared from its Team page (`stack.ts`): the remote door signs people in, this
// machine keeps working without sign-in as the owner.
const local = () => process.env.BOARD_E2E_SHARE_URL ?? "";
const remote = () => process.env.BOARD_E2E_SHARE_REMOTE ?? "";

test("sharing a local board opens a door that only invited people get through", async ({
  browser,
}) => {
  const outsider = await (await browser.newContext()).newPage();
  const closed = await outsider.request.get(`${remote()}/api/auth/me`);
  expect(closed.status()).toBe(503);

  const owner = await (await browser.newContext()).newPage();
  await owner.goto(`${local()}/team`);
  const sharing = owner.getByRole("region", { name: "Sharing" });
  await sharing.getByRole("textbox", { name: "Your name on the team" }).fill("Mina");
  await sharing.getByRole("textbox", { name: "Address people open" }).fill(`${remote()}/`);
  await sharing.getByRole("button", { name: "Turn sharing on" }).click();
  await expect(sharing.getByRole("link", { name: remote() })).toBeVisible();
  // This machine is Mina now, still without signing in.
  await expect(owner.getByRole("link", { name: "Mina" })).toBeVisible();

  await outsider.goto(`${remote()}/`);
  await expect(outsider).toHaveURL(/\/signin/);

  const invites = owner.getByRole("region", { name: "Invites" });
  await invites.getByRole("radio", { name: "Editor" }).click();
  await invites.getByRole("button", { name: "Create invite" }).click();
  const url = await invites.getByRole("textbox", { name: "Invite link" }).inputValue();
  expect(url.startsWith(`${remote()}/i/`)).toBe(true);
  await outsider.goto(url);
  await outsider.getByRole("textbox", { name: "Your name" }).fill("Jun");
  await outsider.getByRole("button", { name: "Join" }).click();
  await expect(outsider.getByRole("heading", { name: "Projects" })).toBeVisible();
  await expect(outsider.getByRole("link", { name: "Jun" })).toBeVisible();

  // The address changes while shared, without turning sharing off.
  const address = sharing.getByRole("textbox", { name: "Address people open" });
  await expect(address).toHaveValue(remote());
  await address.fill(`${remote()}/changed`);
  await sharing.getByRole("button", { name: "Apply" }).click();
  await expect(sharing.getByRole("link", { name: `${remote()}/changed` })).toBeVisible();

  await sharing.getByRole("button", { name: "Turn sharing off" }).click();
  await expect(sharing.getByRole("button", { name: "Turn sharing on" })).toBeVisible();
  const after = await outsider.request.get(`${remote()}/api/auth/me`);
  expect(after.status()).toBe(503);
});

test("sharing through Tailscale shows the machine's tailnet name", async ({ page }) => {
  await page.goto(`${local()}/team`);
  const sharing = page.getByRole("region", { name: "Sharing" });
  await sharing.getByRole("radio", { name: "Tailscale network" }).click();
  await expect(sharing.getByText("Address: https://e2e.tail1.ts.net")).toBeVisible();
  await sharing.getByRole("button", { name: "Turn sharing on" }).click();
  await expect(sharing.getByRole("link", { name: "https://e2e.tail1.ts.net" })).toBeVisible();
  await sharing.getByRole("button", { name: "Turn sharing off" }).click();
  await expect(sharing.getByRole("button", { name: "Turn sharing on" })).toBeVisible();
  // ngrok asks for the account's authtoken (not tried here: it needs a real account).
  await sharing.getByRole("radio", { name: "ngrok" }).click();
  await expect(sharing.getByLabel("ngrok authtoken")).toHaveAttribute("type", "password");
});

test("sharing on the local network opens the remote door on this machine's IP", async ({
  page,
}) => {
  await page.goto(`${local()}/team`);
  const sharing = page.getByRole("region", { name: "Sharing" });
  await sharing.getByRole("radio", { name: "Local network" }).click();
  await sharing.getByRole("button", { name: "Turn sharing on" }).click();
  const link = sharing.getByRole("link", { name: /^http:\/\/\d+\.\d+\.\d+\.\d+:\d+$/ });
  await expect(link).toBeVisible();
  const url = (await link.textContent()) ?? "";
  const me = await page.request.get(`${url}/api/auth/me`);
  expect(me.status()).toBe(401);
  await sharing.getByRole("button", { name: "Turn sharing off" }).click();
  await expect(sharing.getByRole("button", { name: "Turn sharing on" })).toBeVisible();
  // Closed, or (on a connection kept alive) answering that sharing is off.
  const after = await page.request
    .get(`${url}/api/auth/me`)
    .then((r) => r.status())
    .catch(() => 0);
  expect([0, 503]).toContain(after);
});
