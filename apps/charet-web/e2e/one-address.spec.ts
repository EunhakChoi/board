import { expect, test } from "@playwright/test";
import { adminToken, base, enter } from "./people.ts";

// The public server (`stack.ts`) has no preview address of its own: previews come from the tool's
// address under /_preview, sandboxed into an opaque origin.

test("a live preview on the tool's address renders but cannot reach the tool", async ({ page }) => {
  test.setTimeout(240_000);
  await enter(page, adminToken());
  await page.goto(`${base()}/p/demo/t/screen/tasks-list`);
  await page.getByRole("radio", { name: "Live" }).click();

  const iframe = page.locator('iframe[title="tasks-list"]');
  await expect(iframe).toHaveAttribute("sandbox", /allow-scripts/, { timeout: 200_000 });
  expect(await iframe.getAttribute("src")).toMatch(/\/_preview\/[^/]+\/demo\//);
  // The screen renders with its mocks (no service worker in an opaque origin).
  await expect(
    page.frameLocator('iframe[title="tasks-list"]').getByText("Write the release notes"),
  ).toBeVisible({
    timeout: 30_000,
  });

  const frame = page.frames().find((f) => f.url().includes("/_preview/"));
  if (!frame) throw new Error("no preview frame");
  expect(await frame.evaluate(() => self.origin)).toBe("null");
  const reach = await frame.evaluate(async () => {
    const results: string[] = [];
    try {
      results.push(window.parent.document.title);
    } catch {
      results.push("parent blocked");
    }
    try {
      const response = await fetch("/api/auth/me", { credentials: "include" });
      results.push(`api ${response.status}`);
    } catch {
      results.push("api blocked");
    }
    return results;
  });
  expect(reach).toEqual(["parent blocked", "api blocked"]);
});
