import { chromium, expect } from "@playwright/test";

const base = "https://aigen-test.ngrok.app";
const out = "/home/patent/.claude/jobs/57ce3a13/tmp/p4real";
const owner = process.env.T;
const browser = await chromium.launch();
const shot = (p, n) => p.screenshot({ path: `${out}/${n}.png` });
const log = (...a) => console.log(new Date().toISOString().slice(11, 19), ...a);

const users = await (
  await fetch(`${base}/api/admin/users`, { headers: { authorization: `Bearer ${owner}` } })
).json();
const ted = (users.users ?? users).find((u) => u.name === "Test Editor");
const editorToken = ted.token;
log("editor added");

const ectx = await browser.newContext({ viewport: { width: 1400, height: 900 } });
const ed = await ectx.newPage();
await ed.goto(`${base}/?t=${encodeURIComponent(editorToken)}`);
await expect(ed.getByRole("heading", { name: "Projects" })).toBeVisible();
const branchUrl = `${base}/p/example/branches/3`;
await ed.goto(branchUrl);
await ed.getByRole("button", { name: "Request review" }).click();
await expect(ed.getByText("In review", { exact: true })).toBeVisible();
log("review requested", branchUrl);

// The owner reviews and merges.
const octx = await browser.newContext({ viewport: { width: 1400, height: 900 } });
const op = await octx.newPage();
await op.goto(`${base}/?t=${encodeURIComponent(owner)}`);
await expect(op.getByRole("heading", { name: "Projects" })).toBeVisible();
await op.goto(branchUrl);
await expect(op.getByText("In review", { exact: true })).toBeVisible();
const frames = op.locator("figure iframe");
await expect(frames).toHaveCount(2, { timeout: 240000 });
await op.waitForTimeout(5000);
await shot(op, "5-owner-review");
await op.getByRole("button", { name: "Merge" }).click();
await expect(op.getByText(/Merged as v\d+/)).toBeVisible({ timeout: 300000 });
await shot(op, "6-merged");
log(
  "merged",
  await op
    .getByText(/Merged as v\d+/)
    .first()
    .textContent(),
);
await op.goto(`${base}/team`);
await op.waitForTimeout(2000);
log(
  "costs",
  (await op.getByRole("region", { name: "Agent costs" }).innerText()).replace(/\n+/g, " | "),
);
await browser.close();
