import { expect, test, type Page } from "@playwright/test";

// Tools › Water calculator: drinking water to store and drip irrigation for a
// garden, saved on the hub for the whole household, and opened filled in from
// a link. Runs against the same hub as the other interface tests (see
// app.spec.ts), in whatever state they left it.

const PASSWORD = "correct horse";

/** Ends on Settings › Devices, with the hub answering. */
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

async function noHorizontalScroll(page: Page) {
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow, "page must not be wider than the screen").toBeLessThanOrEqual(1);
}

/** What the hub keeps for the household. */
async function hubPlan(page: Page) {
  return (await (await page.request.get("/api/water")).json()).plan;
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

test("water: drinking water to store, saved for the whole household", async ({ page }) => {
  await ensureSetUp(page);
  // Nothing saved yet, whatever ran before.
  await page.request.put("/api/water", { data: { plan: null } });
  await page.goto("/#tools");
  // A tile of its own, once.
  await page.locator('[data-tool="water"]').getByRole("button", { name: /^Water calculator/ }).click();
  await expect(page).toHaveURL(/#water$/);
  await expect(page.getByRole("heading", { name: "Water calculator", level: 1 })).toBeVisible();
  await expect(page.locator("nav").getByRole("button", { name: "Tools" })).toHaveAttribute("aria-current", "true");

  // At first two adults for 3 days: 2 × 3.8 L × 3 = 22.8 L.
  const drinking = page.getByTestId("water-drinking");
  await expect(drinking).toHaveText("23 L");
  const adults = page.getByLabel("Adults", { exact: true });
  await page.getByRole("button", { name: "More: Adults" }).click();
  await expect(adults).toHaveValue("3");
  await adults.fill("4");
  await page.getByRole("button", { name: "1 week", exact: true }).click();
  await expect(page.getByRole("button", { name: "1 week", exact: true })).toHaveAttribute("aria-pressed", "true");
  // 4 × 3.8 L × 7 = 106.4 L; with hygiene 4 × 15 L × 7.
  await expect(drinking).toHaveText("107 L");
  await expect(page.getByTestId("water-hygiene")).toHaveText("420 L");
  await expect(page.getByText("At least one gallon (3.8 L) per person a day, as Ready.gov advises, for 7 days.")).toBeVisible();
  await expect(page.getByRole("row", { name: /Drinking and cooking/ }).getByRole("cell")).toHaveText(["22", "11", "6", "1"]);
  await expect(page.getByRole("row", { name: /With hygiene/ }).getByRole("cell")).toHaveText(["84", "42", "21", "3"]);

  // A cat drinks too (0.65 L a day), and any number of days.
  await page.getByRole("button", { name: "More: Cats and small dogs" }).click();
  await expect(drinking).toHaveText("111 L");
  await page.getByRole("button", { name: "Other", exact: true }).click();
  await page.getByLabel("Number of days").fill("10");
  await expect(drinking).toHaveText("159 L");

  // How to make water safe, with the official numbers and where they come from.
  await expect(page.getByRole("heading", { name: "Make water safe to drink" })).toBeVisible();
  await expect(page.getByText(/rolling boil for 1 minute, or for 3 minutes above 1,000 m/)).toBeVisible();
  await expect(page.getByText(/Add 2 drops per liter of clear water, 4 if it is cloudy/)).toBeVisible();
  await expect(page.getByText("Source: CDC and EPA; not endorsed by them.")).toBeVisible();
  await expect(page.getByRole("link", { name: "EPA" }).first()).toHaveAttribute("href", /epa\.gov/);
  await page.getByText("How this is calculated").click();
  await expect(page.getByText(/Ready\.gov's one gallon per person a day/)).toBeVisible();
  await noHorizontalScroll(page);

  // Saved on the hub for everyone, and still there after a reload.
  await expect.poll(async () => (await hubPlan(page))?.drink).toEqual({ people: 4, children: 0, smallPets: 1, largePets: 0, days: 10 });
  await expect(page.getByRole("status").filter({ hasText: "Saved on the hub: the same numbers for the whole household." })).toBeVisible();
  expect((await (await page.request.get("/api/water")).json()).updated_by).toBe("laptop");
  await page.reload();
  await expect(adults).toHaveValue("4");
  await expect(page.getByLabel("Number of days")).toHaveValue("10");
  await expect(page.getByLabel("Cats and small dogs", { exact: true })).toHaveValue("1");
  await expect(drinking).toHaveText("159 L");
  await expect(page.getByRole("status").filter({ hasText: "Last change: the laptop" })).toBeVisible();

  // Another device changes the numbers meanwhile; this screen follows when it looks again
  // (not while someone is typing), for example when the app comes back to the screen.
  await page.request.put("/api/water", { data: { plan: { v: 1, drink: { people: 1, children: 0, smallPets: 0, largePets: 0, days: 3 } } } });
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
  await page.evaluate(() => document.dispatchEvent(new Event("visibilitychange")));
  await expect(adults).toHaveValue("1", { timeout: 20_000 });
  // 1 × 3.8 L × 3 days = 11.4 L.
  await expect(drinking).toHaveText("12 L");
  await expect(page.getByRole("button", { name: "3 days", exact: true })).toHaveAttribute("aria-pressed", "true");
});

test("water: drip irrigation for a garden, and rainwater from the roof", async ({ page }) => {
  await ensureSetUp(page);
  await page.request.put("/api/water", { data: { plan: null } });
  await page.goto("/#water");
  await page.getByRole("button", { name: "Drip irrigation for a garden" }).click();
  await expect(page).toHaveURL(/#water\/drip$/);
  await expect(page.getByText("Enter the size of at least one bed.")).toBeVisible();

  // A 3 × 1.2 m bed of tomatoes: three drip lines are suggested for its width.
  const bed1 = page.getByRole("group", { name: "Bed 1", exact: true });
  await bed1.getByLabel("Length (m)").fill("3");
  await bed1.getByLabel("Width (m)").fill("1.2");
  await expect(bed1.getByLabel("Drip lines along the bed")).toHaveAttribute("placeholder", "3");
  await bed1.getByLabel("What grows there").selectOption("tomatoes");

  // July at 45°N with 30 °C days and 17 °C nights: ET₀ about 5.7 mm a day (Hargreaves-Samani).
  await page.getByRole("combobox", { name: /^Month/ }).selectOption("7");
  await page.getByLabel("Latitude (°)").fill("45");
  await page.getByLabel("Usual daytime high (°C)").fill("30");
  await page.getByLabel("Usual night low (°C)").fill("17");
  await expect(page.getByTestId("water-et0")).toContainText("In this weather a lawn loses about 5.7 mm of water a day.");
  // 5.66 × 1.05 / 0.9 × 3.6 m² = 23.8 L from 30 drippers of 2 L/h (every 30 cm): 24 minutes.
  await expect(page.getByTestId("water-per-day")).toHaveText("24 L");
  await expect(page.getByTestId("water-drippers")).toHaveText("30");
  await expect(page.getByTestId("water-run")).toHaveText("24 min a day");

  // A second bed given by its area, and the household's own ET₀ for round numbers:
  // 5 × 1.05 / 0.9 × 3.6 = 21 L and 5 × 1.1 / 0.9 × 6 = 36.7 L; 30 + 50 drippers.
  await page.getByRole("button", { name: "Add a bed" }).click();
  const bed2 = page.getByRole("group", { name: "Bed 2", exact: true });
  await bed2.getByRole("button", { name: "Area", exact: true }).click();
  await bed2.getByLabel("Area (m²)").fill("6");
  await bed2.getByLabel("What grows there").selectOption("potatoes");
  await page.getByLabel("Your own ET₀ (mm a day)").fill("5");
  await expect(page.getByTestId("water-per-day")).toHaveText("58 L");
  await expect(page.getByTestId("water-drippers")).toHaveText("80");
  await expect(page.getByTestId("water-run")).toHaveText("22 min a day");
  await expect(page.getByText("3.6 m² · 21 L a day · 30 drippers")).toBeVisible();

  // 31 mm of rain in July is 1 mm a day: (5.25 − 1) / 0.9 × 3.6 + (5.5 − 1) / 0.9 × 6 = 47 L.
  await page.getByLabel("Rain in that month (mm)").fill("31");
  await expect(page.getByTestId("water-per-day")).toHaveText("47 L");
  // The roof: 50 m² × 31 mm × 0.8 = 1,240 L, 26 days of this garden.
  await page.getByLabel("Roof area (m²)").fill("50");
  await expect(page.getByTestId("water-roof")).toHaveText("In a month with 31 mm of rain, the roof collects about 1,240 L. That is enough for this garden for about 26 days.");
  await expect(page.getByText(/Rainwater must be treated before drinking/)).toBeVisible();

  // A hot spell with slow drippers far apart: over an hour, so twice a day.
  // (8.4 − 1) / 0.9 × 3.6 + (8.8 − 1) / 0.9 × 6 = 81.6 L from 21 + 37 drippers of 1 L/h: 2 × 43 minutes.
  await page.getByLabel("Your own ET₀ (mm a day)").fill("8");
  await page.getByRole("button", { name: "40 cm", exact: true }).click();
  await page.getByRole("button", { name: "1 L/h", exact: true }).click();
  await expect(page.getByTestId("water-drippers")).toHaveText("58");
  await expect(page.getByTestId("water-run")).toHaveText("43 min twice a day (morning and evening)");
  await expect(page.getByText("82 L · a 200 L drum")).toBeVisible();
  await expect(page.getByText(/a 1 L\/h dripper gives about 167 mL/)).toBeVisible();
  await page.getByText("How this is calculated").click();
  await expect(page.getByRole("row", { name: /Potatoes/ })).toContainText("1.10");
  await noHorizontalScroll(page);

  // The hub has it all; a reload opens the garden again with every input.
  await expect.poll(async () => (await hubPlan(page))?.garden?.flow).toBe(1);
  await expect.poll(async () => (await hubPlan(page))?.garden?.et0).toBe(8);
  await page.reload();
  await expect(bed1.getByLabel("Length (m)")).toHaveValue("3");
  await expect(bed1.getByLabel("Width (m)")).toHaveValue("1.2");
  await expect(bed2.getByLabel("Area (m²)")).toHaveValue("6");
  await expect(bed2.getByLabel("What grows there")).toHaveValue("potatoes");
  await expect(page.getByLabel("Latitude (°)")).toHaveValue("45");
  await expect(page.getByLabel("Roof area (m²)")).toHaveValue("50");
  await expect(page.getByRole("button", { name: "1 L/h", exact: true })).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByTestId("water-run")).toHaveText("43 min twice a day (morning and evening)");

  // A bed can be removed.
  await page.getByRole("button", { name: "Remove: Bed 2" }).click();
  await expect(page.getByRole("group", { name: "Bed 2", exact: true })).toHaveCount(0);
  await expect(page.getByTestId("water-drippers")).toHaveText("21");
});

test("water: a link opens the calculator with numbers of its own, kept only when asked", async ({ page }) => {
  await ensureSetUp(page);
  const household = { v: 1, drink: { people: 3, children: 0, smallPets: 0, largePets: 0, days: 3 }, garden: { beds: [], month: 7, spacing: 30, flow: 2 } };
  await page.request.put("/api/water", { data: { plan: household } });
  await page.goto("/#tools/water?people=4&days=7");
  await expect(page.getByRole("heading", { name: "Water calculator", level: 1 })).toBeVisible();
  const banner = page.getByText("Opened from a link.");
  await expect(banner).toBeVisible();
  // Used once: the address is the calculator's own again.
  await expect(page).toHaveURL(/#water$/);
  const adults = page.getByLabel("Adults", { exact: true });
  await expect(adults).toHaveValue("4");
  await expect(page.getByRole("button", { name: "1 week", exact: true })).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByTestId("water-drinking")).toHaveText("107 L");
  // Changing it saves nothing: the household still has 3 people for 3 days.
  await page.getByRole("button", { name: "More: Children" }).click();
  await page.waitForTimeout(1000);
  expect((await hubPlan(page)).drink).toEqual(household.drink);
  await page.getByRole("button", { name: "Show the saved numbers" }).click();
  await expect(banner).toHaveCount(0);
  await expect(adults).toHaveValue("3");
  // 3 × 3.8 L × 3 days = 34.2 L.
  await expect(page.getByTestId("water-drinking")).toHaveText("35 L");

  // A garden link followed while the calculator is open, over the household's numbers; this one is kept.
  await page.goto("/#tools/water?part=drip&beds=3x1.2:tomatoes,2x1:greens&lat=44.8");
  await expect(banner).toBeVisible();
  await expect(page).toHaveURL(/#water\/drip$/);
  const bed1 = page.getByRole("group", { name: "Bed 1", exact: true });
  const bed2 = page.getByRole("group", { name: "Bed 2", exact: true });
  await expect(bed1.getByLabel("Length (m)")).toHaveValue("3");
  await expect(bed1.getByLabel("Width (m)")).toHaveValue("1.2");
  await expect(bed2.getByLabel("What grows there")).toHaveValue("greens");
  await expect(page.getByLabel("Latitude (°)")).toHaveValue("44.8");
  await page.getByRole("button", { name: "Save for the household" }).click();
  await expect(banner).toHaveCount(0);
  await expect(page.getByRole("status").filter({ hasText: "Saved on the hub" })).toBeVisible();
  await expect.poll(async () => (await hubPlan(page))?.garden?.lat).toBe(44.8);
  const kept = await hubPlan(page);
  expect(kept.garden.beds.map((b: { length: number; width: number; crop: string }) => [b.length, b.width, b.crop])).toEqual([
    [3, 1.2, "tomatoes"],
    [2, 1, "greens"],
  ]);
  expect(kept.drink.people).toBe(3);
  await page.reload();
  await expect(bed2.getByLabel("What grows there")).toHaveValue("greens");
  await expect(banner).toHaveCount(0);
  await page.getByRole("button", { name: "Drinking water to store" }).click();
  await expect(adults).toHaveValue("3");

  // Opened from another screen by the other address form ("#water?..."), in Serbian
  // (chosen in Settings; the page stays loaded, so it holds).
  await page.goto("/#settings/language");
  await page.getByRole("combobox", { name: /^(Language|Jezik)$/ }).selectOption("sr");
  await page.goto("/#water?children=2");
  await expect(page.getByRole("heading", { name: "Kalkulator vode", level: 1 })).toBeVisible();
  await expect(page.getByText("Otvoreno preko linka.")).toBeVisible();
  await expect(page.getByLabel("Deca", { exact: true })).toHaveValue("2");
  // 5 people × 3.8 L × 3 days = 57 L.
  await expect(page.getByTestId("water-drinking")).toHaveText("57 L");
  await expect(page.getByText(/kako savetuje Ready\.gov, za 3 dana\./)).toBeVisible();
  await expect(page).toHaveURL(/#water$/);
  await noHorizontalScroll(page);
  await page.request.put("/api/water", { data: { plan: null } });
});
