import { expect, test, type Page, type Request } from "@playwright/test";

// The Zaklon map, drawn from the test hub's map assets (e2e/fixtures/map-assets:
// the world at zoom 0-1 and a few places), and the household's home location.

const PASSWORD = "correct horse";

/** Ends on Settings › Devices, with the hub answering (as in app.spec.ts). */
async function ensureSetUp(page: Page) {
  await page.goto("/#settings/devices");
  const setup = page.getByRole("heading", { name: /Set up your household|Podesi domaćinstvo/ });
  const ready = page.getByRole("heading", { name: /^(Paired devices|Upareni uređaji)$/ });
  await expect(setup.or(ready)).toBeVisible({ timeout: 10_000 });
  if (await setup.isVisible()) {
    await page.getByLabel(/Hub name|Ime huba/).fill("E2E hub");
    await page.getByLabel(/^Household password$|^Lozinka domaćinstva$/).fill(PASSWORD);
    await page.getByLabel(/Repeat password|Ponovi lozinku/).fill(PASSWORD);
    await page.getByRole("button", { name: /Finish setup|Završi podešavanje/ }).click();
  }
  await expect(ready).toBeVisible({ timeout: 20_000 });
}

/** Every request the page makes outside this computer (there must be none). */
function watchOutside(page: Page): string[] {
  const outside: string[] = [];
  page.on("request", (r: Request) => {
    const url = new URL(r.url());
    if ((url.protocol === "http:" || url.protocol === "https:") && url.hostname !== "127.0.0.1" && url.hostname !== "localhost") outside.push(r.url());
  });
  return outside;
}

/** Every map on the page is drawn over its whole box (not in a strip, not hidden). */
async function fillsItsBox(page: Page) {
  await expect
    .poll(() =>
      page.evaluate(() =>
        [...document.querySelectorAll(".zmap")].map((box) => {
          const canvas = box.querySelector("canvas.maplibregl-canvas");
          if (!canvas) return false;
          const [b, c] = [box.getBoundingClientRect(), canvas.getBoundingClientRect()];
          return c.height > 100 && Math.abs(c.height - (b.height - 2)) <= 2 && Math.abs(c.width - (b.width - 2)) <= 2;
        }),
      ),
    )
    .not.toContain(false);
}

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    try {
      localStorage.setItem("zaklon.lang", "en");
    } catch {
      /* ignore */
    }
  });
});

test("maps: the Zaklon map is drawn from the hub with its credit, next to Navigation", async ({ page }) => {
  await ensureSetUp(page);
  const outside = watchOutside(page);
  const fromHub: string[] = [];
  page.on("request", (r) => {
    const path = new URL(r.url()).pathname;
    if (path.startsWith("/tiles/") || path.startsWith("/map/")) fromHub.push(path);
  });
  await page.goto("/#maps");
  await expect(page.getByRole("tab", { name: "Zaklon map" })).toHaveAttribute("aria-selected", "true");
  const map = page.getByRole("region", { name: "Zaklon map" });
  await expect(map.locator("canvas.maplibregl-canvas")).toBeVisible({ timeout: 20_000 });
  await fillsItsBox(page);
  const credit = map.getByRole("link", { name: "Protomaps © OpenStreetMap" });
  await expect(credit).toBeVisible();
  await expect(credit).toHaveAttribute("href", "https://www.openstreetmap.org/copyright");
  // Only the overview on this hub: the way to the world map is shown.
  await expect(map.getByText("Only the world overview: countries and larger cities.")).toBeVisible();
  // The way to the world map, where the catalog offers it (only once its checksum is known).
  const world = (await (await page.request.get("/api/map")).json()).world;
  const toAddons = map.getByRole("link", { name: "The world map is in Add-ons." });
  if (world) await expect(toAddons).toHaveAttribute("href", "#addons/maps");
  else await expect(toAddons).toHaveCount(0);
  // Tiles and fonts come from the hub.
  await expect.poll(() => fromHub.some((p) => /^\/tiles\/\d+\/\d+\/\d+\.mvt$/.test(p)), { timeout: 15_000 }).toBe(true);
  await expect.poll(() => fromHub.some((p) => p.startsWith("/map/fonts/")), { timeout: 15_000 }).toBe(true);
  await map.getByRole("button", { name: "Zoom in" }).click();
  await map.getByRole("button", { name: "Show the whole world" }).click();

  // The other part: CoMaps for phones.
  await page.getByRole("tab", { name: "Navigation" }).click();
  await expect(page).toHaveURL(/#maps\/navigation$/);
  await expect(page.getByRole("heading", { name: "Maps on a phone" })).toBeVisible();
  await expect(page.locator("canvas.maplibregl-canvas")).toHaveCount(0);
  await page.getByRole("tab", { name: "Zaklon map" }).click();
  await expect(page).toHaveURL(/#maps$/);
  await expect(page.getByRole("region", { name: "Zaklon map" }).locator("canvas.maplibregl-canvas")).toBeVisible({ timeout: 20_000 });
  expect(outside, "nothing is asked of the internet").toEqual([]);
});

test("maps: the home location is set by finding a town or tapping the map, and Home shows it", async ({ page }) => {
  await ensureSetUp(page);
  const outside = watchOutside(page);
  await page.request.delete("/api/home-location");

  // Home without a home location: the world, and a button to set it.
  await page.goto("/#home");
  const card = page.locator(".home-map");
  await expect(card.locator("canvas.maplibregl-canvas")).toBeVisible({ timeout: 20_000 });
  await fillsItsBox(page);
  await expect(card.getByRole("link", { name: "Protomaps © OpenStreetMap" })).toBeVisible();
  await card.getByRole("link", { name: "Set home location" }).click();
  await expect(page).toHaveURL(/#maps\/home$/);

  // Find the town by its Serbian name, choose it, save it.
  const panel = page.getByRole("region", { name: "Home location" });
  await expect(panel.getByRole("button", { name: "Use this phone's location" })).toHaveCount(0);
  const find = panel.getByLabel("Find a town or city");
  const belgrade = panel.getByRole("button", { name: "Belgrade, Central Serbia, Serbia" });
  await find.fill("belgrade");
  await expect(belgrade).toBeVisible();
  await expect(panel.getByRole("button", { name: /Belgrade, Montana/ })).toBeVisible();
  await find.fill("beograd");
  await expect(panel.getByRole("button", { name: /Belgrade, Montana/ })).toHaveCount(0);
  await expect(belgrade).toBeVisible();
  await expect(panel.getByRole("button", { name: "Save home location" })).toBeDisabled();
  await belgrade.click();
  await expect(panel.getByText("Belgrade, Central Serbia, Serbia", { exact: false }).last()).toBeVisible();
  await expect(page.locator(".zmap-marker.pending")).toBeVisible();
  await panel.getByRole("button", { name: "Save home location" }).click();
  await expect(panel.getByText("Saved. Every device in the household sees it.")).toBeVisible();
  await expect(panel.getByRole("button", { name: "Belgrade, Central Serbia, Serbia" })).toBeVisible();
  await expect(page.getByRole("region", { name: "Zaklon map" })).toHaveAttribute("data-home", "44.804,20.4651");
  const saved = (await (await page.request.get("/api/home-location")).json()).home;
  expect(saved).toMatchObject({ lat: 44.804, lon: 20.4651, label: "Belgrade, Central Serbia, Serbia", source: "place", set_by: "laptop" });

  // Home shows it on the map.
  await page.goto("/#home");
  await expect(page.locator(".home-map")).toContainText("Belgrade, Central Serbia, Serbia");
  await expect(page.locator(".home-map .zmap")).toHaveAttribute("data-home", "44.804,20.4651", { timeout: 20_000 });
  await expect(page.locator(".home-map").getByRole("link", { name: "Set home location" })).toHaveCount(0);

  // Change it with a tap on the map.
  await page.goto("/#maps");
  const again = page.getByRole("region", { name: "Home location" });
  await again.getByRole("button", { name: "Change" }).click();
  const canvas = page.getByRole("region", { name: "Zaklon map" }).locator("canvas.maplibregl-canvas");
  await expect(canvas).toBeVisible({ timeout: 20_000 });
  const box = (await canvas.boundingBox())!;
  await page.mouse.click(box.x + box.width * 0.3, box.y + box.height * 0.6);
  await expect(again.getByText(/Chosen place:.*°/)).toBeVisible();
  await again.getByRole("button", { name: "Save home location" }).click();
  await expect(again.getByText("Saved. Every device in the household sees it.")).toBeVisible();
  const tapped = (await (await page.request.get("/api/home-location")).json()).home;
  expect(tapped.source).toBe("map");
  expect(Math.abs(tapped.lat) <= 90 && Math.abs(tapped.lon) <= 180).toBe(true);

  // Removed on the laptop: Home offers to set it again.
  await again.getByRole("button", { name: "Remove" }).click();
  await again.getByRole("button", { name: "Yes, remove" }).click();
  await expect(again.getByText(/^Not set yet/)).toBeVisible();
  expect((await (await page.request.get("/api/home-location")).json()).home).toBeNull();
  expect(outside, "nothing is asked of the internet").toEqual([]);
});
