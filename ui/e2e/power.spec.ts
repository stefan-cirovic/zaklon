import { expect, test, type Page } from "@playwright/test";

// The power calculator (Tools › Power calculator) against the real hub of the
// interface tests. The household's list lives on the hub; every test starts
// from a list of its own, so the laptop and phone projects do not meet.

const PASSWORD = "correct horse";

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
  // Setup hashes the password with Argon2, slow on purpose; a busy machine needs longer.
  await expect(ready).toBeVisible({ timeout: 20_000 });
}

async function noHorizontalScroll(page: Page) {
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow, "page must not be wider than the screen").toBeLessThanOrEqual(1);
}

/** The household's list as the hub has it. */
async function onHub(page: Page): Promise<{ plan: { lines: { id: string; qty: number }[]; days: number } | null; updated_by: string | null }> {
  return (await page.request.get("/api/power")).json();
}

/** Every change has reached the hub. */
async function saved(page: Page) {
  await expect(page.getByText("Saving…")).toHaveCount(0);
  await expect(page.getByRole("status").filter({ hasText: "Saved on the hub" })).toBeVisible();
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

test("power calculator: a list adds up, the days and the month change the answer, and the hub keeps it", async ({ page }) => {
  await ensureSetUp(page);
  await page.request.put("/api/power", { data: { plan: null } });
  await page.goto("/#tools");
  await page.locator(".topic-grid").getByRole("button", { name: /^Power calculator/ }).click();
  await expect(page).toHaveURL(/#power$/);
  await expect(page.getByRole("heading", { name: "Power calculator", level: 1 })).toBeVisible();
  // The safety notes are always there, open.
  const safety = page.getByRole("region", { name: "Safety first" });
  await expect(safety).toContainText("licensed electrician");
  await expect(safety).toContainText("BMS");
  await expect(safety).toContainText("hydrogen");
  const results = page.getByRole("region", { name: "What you need" });
  await expect(results).toContainText("Add at least one appliance");
  await expect(page.getByText("Nothing on the list yet.")).toBeVisible();

  // A refrigerator counts by its energy a day; LED bulbs by watts × hours.
  const add = async (id: string) => {
    await page.getByRole("combobox", { name: "Appliance to add" }).selectOption(id);
    await page.getByRole("button", { name: "Add", exact: true }).click();
  };
  await add("fridge");
  await add("lights-led");
  await page.getByRole("textbox", { name: "How many: LED bulb" }).fill("6");
  await page.getByRole("textbox", { name: "Hours a day: LED bulb" }).fill("5");
  const rows = page.locator(".pw-table tbody tr");
  await expect(rows).toHaveCount(2);
  await expect(rows.nth(0)).toContainText("Refrigerator with freezer");
  await expect(rows.nth(0)).toContainText("1,200 Wh");
  await expect(rows.nth(1)).toContainText("300 Wh");
  await expect(page.locator(".pw-table tfoot")).toContainText("1,500 Wh");

  // One day in December in Belgrade (the defaults): 1500 Wh ÷ 0.85 ÷ 0.8 at 12 V is 184 Ah;
  // 1765 Wh ÷ (1.8 h × 0.75 × 0.95) is 1376 W of panels; the fridge's start needs a 600 W inverter.
  const summary = results.locator(".pw-summary");
  await expect(summary).toHaveText("2 × 100 Ah 12 V LiFePO4 batteries, about 1,400 W of solar panels and a 600 W pure sine wave inverter.");
  await expect(results).toContainText("184 Ah");
  await expect(results).toContainText("Refrigerator with freezer: a motor takes several times its power");
  await expect(results).toContainText("1,060 W");

  // Three days need three times the battery.
  await page.getByRole("button", { name: "3 days" }).click();
  await expect(page.getByRole("button", { name: "3 days" })).toHaveAttribute("aria-pressed", "true");
  await expect(summary).toContainText("3 × 200 Ah 12 V LiFePO4 batteries");
  await expect(results).toContainText("552 Ah");
  // Any number of days.
  await page.getByRole("textbox", { name: "Other number of days" }).fill("2");
  await expect(page.getByRole("button", { name: "3 days" })).toHaveAttribute("aria-pressed", "false");
  await expect(summary).toContainText("4 × 100 Ah");
  await page.getByRole("button", { name: "3 days" }).click();

  // July's sun needs far fewer panels than December's.
  await page.getByRole("button", { name: /^July:/ }).click();
  await expect(page.getByRole("button", { name: /^July:/ })).toHaveAttribute("aria-pressed", "true");
  await expect(summary).toContainText("about 400 W of solar panels");
  await expect(page.getByRole("combobox", { name: "Month" })).toHaveValue("7");

  // The formulas show this list's own numbers.
  await page.getByText("How this is calculated").click();
  await expect(page.locator(".pw-how")).toContainText("1,200 + 300 = 1,500 Wh");

  // Saved on the hub, the same for every device, and still there after a reload.
  await saved(page);
  const hub = await onHub(page);
  expect(hub.plan!.lines.map((l) => [l.id, l.qty])).toEqual([["fridge", 1], ["lights-led", 6]]);
  expect(hub.plan!.days).toBe(3);
  expect(hub.updated_by).toBe("laptop");
  await page.reload();
  await expect(rows).toHaveCount(2);
  await expect(page.getByRole("textbox", { name: "How many: LED bulb" })).toHaveValue("6");
  await expect(page.getByRole("button", { name: "3 days" })).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByRole("button", { name: /^July:/ })).toHaveAttribute("aria-pressed", "true");
  await expect(summary).toContainText("3 × 200 Ah");
  await expect(page.getByRole("status").filter({ hasText: "Last change: the laptop" })).toBeVisible();
  await noHorizontalScroll(page);
});

test("power calculator: another device's change shows, and lead-acid needs twice the battery", async ({ page }) => {
  await ensureSetUp(page);
  await page.request.put("/api/power", { data: { plan: { v: 1, lines: [{ k: "r", id: "router", qty: 1, watts: 12, hours: 24 }], days: 1 } } });
  await page.goto("/#power");
  const rows = page.locator(".pw-table tbody tr");
  await expect(rows).toHaveCount(1);
  await expect(rows.first()).toContainText("288 Wh");
  // The router straight from the battery: no inverter, no inverter losses.
  await page.getByRole("combobox", { name: "Power from: Wi-Fi router and modem" }).selectOption("dc");
  const results = page.getByRole("region", { name: "What you need" });
  await expect(results).toContainText("Not needed: everything runs on DC.");
  await expect(results.locator(".pw-summary")).toHaveText("1 × 50 Ah 12 V LiFePO4 battery and about 250 W of solar panels.");
  await page.getByRole("combobox", { name: "Battery type" }).selectOption("lead");
  await expect(page.getByRole("textbox", { name: "Usable part of the battery" })).toHaveValue("50");
  // 288 Wh ÷ 0.5 = 576 Wh, 48 Ah.
  await expect(results).toContainText("48 Ah");
  await saved(page);
  // A phone changes the list meanwhile; the laptop's screen follows when it looks again.
  await page.request.put("/api/power", { data: { plan: { v: 1, lines: [{ k: "t", id: "tv", qty: 2, watts: 65, hours: 3 }], days: 1 } } });
  // Nobody is typing (the list is not replaced under someone's fingers), and the app comes back to the screen.
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
  await page.evaluate(() => document.dispatchEvent(new Event("visibilitychange")));
  await expect(rows.first()).toContainText("Small TV", { timeout: 20_000 });
  await expect(rows.first()).toContainText("390 Wh");
});

test("power calculator: a link opens a list of its own, kept only when asked", async ({ page }) => {
  await ensureSetUp(page);
  const household = { v: 1, lines: [{ k: "a", id: "fridge", qty: 1, watts: 200, hours: 24, whDay: 1200 }], days: 1 };
  await page.request.put("/api/power", { data: { plan: household } });
  await page.goto("/#power?items=phone:2,router:1,nothing:3&days=7");
  const banner = page.getByText("Opened from a link.");
  await expect(banner).toBeVisible();
  // Used once: the address is the calculator's own again.
  await expect(page).toHaveURL(/#power$/);
  const rows = page.locator(".pw-table tbody tr");
  await expect(rows).toHaveCount(2);
  await expect(rows.nth(0)).toContainText("Phone charging");
  await expect(rows.nth(1)).toContainText("Wi-Fi router and modem");
  await expect(page.getByRole("button", { name: "7 days" })).toHaveAttribute("aria-pressed", "true");
  // Changing it saves nothing: the household's list is still the refrigerator.
  await page.getByRole("textbox", { name: "How many: Phone charging" }).fill("3");
  await page.waitForTimeout(1000);
  expect((await onHub(page)).plan!.lines.map((l) => l.id)).toEqual(["fridge"]);
  await page.getByRole("button", { name: "Show the saved list" }).click();
  await expect(banner).toHaveCount(0);
  await expect(rows).toHaveCount(1);
  await expect(rows.first()).toContainText("Refrigerator with freezer");

  // Under Tools too, and followed while the calculator is open; this one is kept.
  await page.goto("/#tools/power?items=fan:2:8");
  await expect(page.getByRole("heading", { name: "Power calculator", level: 1 })).toBeVisible();
  await expect(banner).toBeVisible();
  await expect(rows).toHaveCount(1);
  await expect(rows.first()).toContainText("Fan");
  await page.getByRole("button", { name: "Save as the household's list" }).click();
  await expect(banner).toHaveCount(0);
  await saved(page);
  expect((await onHub(page)).plan!.lines.map((l) => [l.id, l.qty])).toEqual([["fan", 2]]);
  await page.reload();
  await expect(rows).toHaveCount(1);
  await expect(page.getByRole("textbox", { name: "Hours a day: Fan" })).toHaveValue("8");

  // Removed, a typical list, then emptied.
  await page.getByRole("button", { name: "Remove: Fan" }).click();
  await expect(page.getByText("Nothing on the list yet.")).toBeVisible();
  await page.getByRole("button", { name: "Start with a typical list" }).click();
  await expect(rows).toHaveCount(5);
  // A line of one's own.
  await page.getByRole("combobox", { name: "Appliance to add" }).selectOption("custom");
  await page.getByRole("button", { name: "Add", exact: true }).click();
  await expect(page.getByRole("textbox", { name: "Name of your own appliance" })).toBeFocused();
  await page.getByRole("textbox", { name: "Name of your own appliance" }).fill("Aquarium pump");
  await page.getByRole("textbox", { name: "Power (each): Aquarium pump" }).fill("7,5");
  await page.getByRole("textbox", { name: "Hours a day: Aquarium pump" }).fill("24");
  await expect(rows.nth(5)).toContainText("180 Wh");
  await noHorizontalScroll(page);
  await page.getByRole("button", { name: "Clear the list" }).click();
  await page.getByRole("button", { name: "Yes, clear it" }).click();
  await expect(page.getByText("Nothing on the list yet.")).toBeVisible();
  await saved(page);
  expect((await onHub(page)).plan!.lines).toEqual([]);
});
