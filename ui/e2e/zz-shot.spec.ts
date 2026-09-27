import { test } from "@playwright/test";
import { join } from "node:path";

// Screenshots of every screen, to look at by eye; not a check. Runs only when
// ZAKLON_SHOTS_DIR names the folder to put them in (SHOT_LANG=en|sr, default sr):
//   ZAKLON_SHOTS_DIR=C:/temp/shots pnpm e2e zz-shot
const DIR = process.env.ZAKLON_SHOTS_DIR;
const LANG = process.env.SHOT_LANG ?? "sr";
test.skip(!DIR, "set ZAKLON_SHOTS_DIR to take screenshots");

test("shots", async ({ page }, info) => {
  test.setTimeout(120000);
  const file = (name: string) => join(DIR ?? "", `${LANG}-${info.project.name}-${name}.png`);
  await page.addInitScript((l) => localStorage.setItem("zaklon.lang", l), LANG);
  await page.goto("/#household");
  const setup = page.getByText(/Set up your household|Podesi domaćinstvo/);
  if (await setup.isVisible({ timeout: 3000 }).catch(() => false)) {
    await page.locator('input[type="password"]').nth(0).fill("correct horse");
    await page.locator('input[type="password"]').nth(1).fill("correct horse");
    await page.locator("form button.btn").click();
    await page.waitForTimeout(800);
    for (const it of [
      { name: "Mleko", quantity: 1, unit: "l", category: "drink", expiry: "2026-09-29", min_quantity: 2 },
      { name: "Brašno tip 500", quantity: 5, unit: "kg", category: "food", expiry: "2027-03-01", place: "Ostava" },
      { name: "Pasulj", quantity: 4, unit: "pcs", category: "food", expiry: "2026-09-20" },
      { name: "Paracetamol 500 mg", quantity: 2, unit: "pack", category: "medicine", expiry: "2027-12-01" },
    ]) await page.request.post("/api/items", { data: it });
    await page.request.post("/api/shopping", { data: { text: "Šećer", quantity: 1, unit: "kg" } });
  }
  const shots: [string, string][] = [
    ["home", "/#home"], ["library", "/#library"], ["maps", "/#maps"], ["supplies", "/#supplies"],
    ["assistant", "/#assistant"], ["addons", "/#addons"], ["household", "/#household"],
  ];
  for (const [name, url] of shots) {
    await page.goto(url);
    await page.waitForTimeout(1200);
    await page.screenshot({ path: file(name), fullPage: true });
  }
  await page.goto("/#supplies");
  await page.waitForTimeout(800);
  for (const [i, n] of [[1, "history"], [2, "shopping"], [3, "putaway"]] as const) {
    await page.locator(".segmented.tabs-4 button").nth(i).click();
    await page.waitForTimeout(600);
    await page.screenshot({ path: file(`supplies-${n}`), fullPage: true });
  }
  await page.locator(".segmented.tabs-4 button").nth(0).click();
  await page.locator(".item").first().click();
  await page.waitForTimeout(600);
  await page.screenshot({ path: file("supplies-edit"), fullPage: true });
});
