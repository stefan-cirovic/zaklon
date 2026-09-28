import { test } from "@playwright/test";
import { join } from "node:path";

// Screenshots of every screen, to look at by eye; not a check. Runs only when
// ZAKLON_SHOTS_DIR names the folder to put them in (SHOT_LANG=en|sr, default sr;
// SHOT_SIZE=1600x900 sets the laptop window, default the project's):
//   ZAKLON_SHOTS_DIR=C:/temp/shots pnpm e2e zz-shot
const DIR = process.env.ZAKLON_SHOTS_DIR;
const LANG = process.env.SHOT_LANG ?? "sr";
const SIZE = process.env.SHOT_SIZE?.match(/^(\d+)x(\d+)$/);
test.skip(!DIR, "set ZAKLON_SHOTS_DIR to take screenshots");

test("shots", async ({ page }, info) => {
  test.setTimeout(120000);
  if (SIZE && info.project.name === "laptop") await page.setViewportSize({ width: Number(SIZE[1]), height: Number(SIZE[2]) });
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
    ["home", "/#home"], ["tools", "/#tools"], ["library", "/#library"], ["maps", "/#maps"], ["supplies", "/#supplies"],
    ["assistant", "/#assistant"], ["addons", "/#addons"], ["household", "/#household"],
    ...["devices", "network", "backups", "privacy", "appearance", "language", "assistant", "updates", "about"].map(
      (c) => [`household-${c}`, `/#household/${c}`] as [string, string],
    ),
  ];
  for (const [name, url] of shots) {
    await page.goto(url);
    await page.waitForTimeout(1200);
    await page.screenshot({ path: file(name), fullPage: true });
  }
  // Household's search, and a setting it opened.
  await page.goto("/#household");
  await page.locator(".set-search input").fill(LANG === "sr" ? "lozinka" : "password");
  await page.waitForTimeout(400);
  await page.screenshot({ path: file("household-search"), fullPage: false });
  await page.keyboard.press("Enter");
  await page.waitForTimeout(600);
  await page.screenshot({ path: file("household-search-opened"), fullPage: false });
  // Add-ons as a file explorer, with some packs on the hub, one on its way and one
  // that failed (states made up for the picture; nothing is downloaded).
  await page.route("**/api/catalog", async (r) => {
    const json = await (await r.fetch()).json();
    for (const p of json.packs) {
      if (["wikipedia-sr-maxi", "wikimed-en", "kiwix-tools", "llama-cpp", "qwen35-4b"].includes(p.id)) p.state = { ...p.state, status: "installed", bytes_done: p.size, bytes_total: p.size };
      if (p.id === "ifixit-en") p.state = { ...p.state, status: "downloading", bytes_done: Math.round(p.size * 0.45), bytes_total: p.size, speed: 3_400_000 };
      if (p.id === "zimgit-water-en") p.state = { ...p.state, status: "failed", error: "checksum mismatch" };
      if (p.id === "qwen35-9b") p.state = { ...p.state, status: "paused", bytes_done: Math.round(p.size * 0.2), bytes_total: p.size };
    }
    return r.fulfill({ json });
  });
  await page.route("**/api/maps", async (r) => {
    const json = await (await r.fetch()).json();
    const serbia = json.countries.find((c: { id: string }) => c.id === "Serbia");
    for (const reg of serbia.regions) reg.status = "installed";
    json.installed_bytes = serbia.size;
    return r.fulfill({ json });
  });
  for (const [name, url, view] of [
    ["addons-x", "/#addons", "tiles"],
    ["addons-x-models", "/#addons/models", "tiles"],
    ["addons-x-skills", "/#addons/skills", "tiles"],
    ["addons-x-details", "/#addons", "details"],
    ["addons-x-reference-details", "/#addons/reference", "details"],
    ["addons-x-maps-details", "/#addons/maps", "details"],
    ["addons-x-library", "/#addons/library", "details"],
  ]) {
    await page.evaluate((v) => localStorage.setItem("zaklon.addonsView", v), view);
    await page.goto(url);
    await page.reload();
    await page.waitForTimeout(1200);
    await page.screenshot({ path: file(name), fullPage: name !== "addons-x-maps-details" });
  }
  await page.getByRole("searchbox").fill("wiki");
  await page.waitForTimeout(400);
  await page.screenshot({ path: file("addons-x-search"), fullPage: true });
  await page.getByRole("searchbox").fill("");
  if (info.project.name === "laptop") {
    // A USB drive, when the machine has another drive.
    await page.goto("/#addons");
    await page.reload();
    await page.waitForTimeout(1200);
    const other = page.locator(".drive-grid button.drive-tile").nth(1);
    if (await other.count()) {
      await other.click();
      await page.waitForTimeout(800);
      await page.screenshot({ path: file("addons-x-drive"), fullPage: true });
    }
  }
  await page.evaluate(() => localStorage.setItem("zaklon.addonsView", "tiles"));
  await page.unroute("**/api/catalog");
  await page.unroute("**/api/maps");

  // The bar with a pinned tool, and the logo while pointed at (laptop).
  await page.request.post("/api/pinned-tool", { data: { tool: "supplies" } });
  await page.goto("/#tools");
  await page.reload();
  await page.waitForTimeout(1200);
  if (info.project.name === "laptop") await page.locator(".nav-brand").hover();
  await page.waitForTimeout(400);
  await page.screenshot({ path: file("tools-pinned"), fullPage: false });
  await page.request.post("/api/pinned-tool", { data: { tool: null } });
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
