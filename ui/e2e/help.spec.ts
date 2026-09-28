import { expect, test, type Page } from "@playwright/test";

// The user guide: opened from Tools and from "How it works" on a screen, in
// English and Serbian, with a search. Runs against the same hub as the other
// interface tests (see app.spec.ts), in whatever state they left it.

const PASSWORD = "correct horse";

/** Ends on Household's tiles, with the hub answering. */
async function ensureSetUp(page: Page) {
  await page.goto("/#household");
  const setup = page.getByRole("heading", { name: /Set up your household|Podesi domaćinstvo/ });
  const ready = page.locator(".hub-card .hub-state", { hasText: /Running|Radi/ });
  await expect(setup.or(ready)).toBeVisible({ timeout: 10_000 });
  if (await setup.isVisible()) {
    await page.getByLabel(/Hub name|Ime huba/).fill("E2E hub");
    await page.getByLabel(/^Household password$|^Lozinka domaćinstva$/).fill(PASSWORD);
    await page.getByLabel(/Repeat password|Ponovi lozinku/).fill(PASSWORD);
    await page.getByRole("button", { name: /Finish setup|Završi podešavanje/ }).click();
  }
  await expect(ready).toBeVisible({ timeout: 20_000 });
}

/** The app's language, chosen under Household > Language (the page stays loaded, so it holds). */
async function setLanguage(page: Page, lang: "en" | "sr") {
  await page.goto("/#household/language");
  await page.getByRole("combobox", { name: /^(Language|Jezik)$/ }).selectOption(lang);
}

async function noHorizontalScroll(page: Page) {
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow, "page must not be wider than the screen").toBeLessThanOrEqual(1);
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

test("help: opens from Tools, lists every topic, and a topic opens with its sections and links", async ({ page }) => {
  await ensureSetUp(page);
  await page.goto("/#tools");
  await page.locator(".tool-grid").getByRole("link", { name: /^Help/ }).click();
  await expect(page).toHaveURL(/#help$/);
  await expect(page.getByRole("heading", { name: "Help", level: 1 })).toBeVisible();
  // Help belongs under Tools in the bar.
  await expect(page.locator("nav.nav").getByRole("button", { name: "Tools" })).toHaveAttribute("aria-current", "true");
  const tiles = page.locator(".help-home .set-tiles").getByRole("link");
  await expect(tiles).toHaveCount(11);
  await expect(tiles.first()).toContainText("Getting started");
  await expect(page.getByRole("heading", { name: "Offline and troubleshooting", level: 2 })).toBeVisible();
  await noHorizontalScroll(page);

  await page.getByRole("link", { name: /^Pairing a phone/ }).click();
  await expect(page).toHaveURL(/#help\/pairing$/);
  const title = page.getByRole("heading", { name: "Pairing a phone", level: 1 });
  await expect(title).toBeVisible();
  await expect(title).toBeFocused();
  await expect(page.getByRole("heading", { name: "Pair with the QR code (easiest)", level: 2 })).toBeVisible();
  await expect(page.getByRole("note").first()).toContainText("Good to know");
  await noHorizontalScroll(page);

  // A link in the text opens the screen it names; Back returns to the page.
  await page.locator(".help-article").getByRole("link", { name: "Household › Devices" }).first().click();
  await expect(page).toHaveURL(/#household\/devices$/);
  await expect(page.getByRole("heading", { name: "Devices", level: 1 })).toBeVisible();
  await page.goBack();
  await expect(title).toBeVisible();

  // Next leads to the following topic; the way back returns to the list.
  await page.getByRole("link", { name: /^Next.*Home$/ }).click();
  await expect(page).toHaveURL(/#help\/home$/);
  await expect(page.getByRole("heading", { name: "Home", level: 1 })).toBeVisible();
  await page.getByRole("link", { name: "Back to Help" }).click();
  await expect(page).toHaveURL(/#help$/);
  await expect(tiles).toHaveCount(11);
});

test("help: How it works on a screen opens that screen's topic", async ({ page }, info) => {
  await ensureSetUp(page);
  await page.goto("/#supplies");
  const link = page.getByRole("link", { name: "How it works" });
  if (info.project.name === "phone") {
    // Only the "?" shows on a phone, a fingertip wide.
    const box = await link.boundingBox();
    expect(box!.width).toBeGreaterThanOrEqual(44);
    expect(box!.height).toBeGreaterThanOrEqual(44);
  }
  await link.click();
  await expect(page).toHaveURL(/#help\/supplies$/);
  await expect(page.getByRole("heading", { name: "Supplies", level: 1 })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Barcode scanning (phones)", level: 2 })).toBeVisible();
  await noHorizontalScroll(page);
  // The page ends with the way to its screen.
  await page.getByRole("link", { name: "Open Supplies" }).click();
  await expect(page).toHaveURL(/#supplies$/);
  await expect(page.getByRole("heading", { name: "Supplies", level: 1 })).toBeVisible();

  // A Household page opens its own part of the Household topic.
  await page.goto("/#household/backups");
  await page.getByRole("link", { name: "How it works" }).click();
  await expect(page).toHaveURL(/#help\/household\/backups$/);
  await expect(page.getByRole("heading", { name: "Household settings", level: 1 })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Backups", level: 2 })).toBeInViewport();

  // Every screen has one.
  for (const tab of ["home", "assistant", "library", "maps", "addons", "household"]) {
    await page.goto(`/#${tab}`);
    await expect(page.getByRole("link", { name: "How it works" })).toBeVisible();
    await noHorizontalScroll(page);
  }
});

test("help: switching the language shows the guide in Serbian", async ({ page }) => {
  await ensureSetUp(page);
  await page.goto("/#help/supplies");
  await expect(page.getByRole("heading", { name: "Supplies", level: 1 })).toBeVisible();
  await setLanguage(page, "sr");
  await page.goto("/#help/supplies");
  await expect(page.getByRole("heading", { name: "Zalihe", level: 1 })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Skeniranje barkoda (telefoni)", level: 2 })).toBeVisible();
  await expect(page.getByRole("navigation", { name: "Putanja" }).getByRole("link", { name: "Pomoć" })).toBeVisible();
  await expect(page.getByRole("link", { name: "Otvori Zalihe" })).toBeVisible();
  await page.goto("/#help");
  await expect(page.getByRole("heading", { name: "Pomoć", level: 1 })).toBeVisible();
  await expect(page.locator(".help-home .set-tiles").getByRole("link").first()).toContainText("Prvi koraci");
  await page.goto("/#tools");
  await expect(page.locator(".tool-grid").getByRole("link", { name: /^Pomoć/ })).toBeVisible();
  await page.goto("/#maps");
  await expect(page.getByRole("link", { name: "Kako radi" })).toBeVisible();
  await noHorizontalScroll(page);
  await setLanguage(page, "en");
});

test("help: the search finds topics by any word, with or without accents", async ({ page }) => {
  await ensureSetUp(page);
  await page.goto("/#help");
  const box = page.getByRole("searchbox", { name: "Search help" });
  const results = page.locator(".help-results");
  await box.fill("barcode");
  await expect(results.locator(".help-result").first()).toContainText("Supplies");
  await expect(results.getByRole("link", { name: "Barcode scanning (phones)", exact: true })).toBeVisible();
  await box.fill("firewall");
  await expect(results).toContainText("Troubleshooting");
  await expect(results).toContainText("Household settings");
  await box.fill("xyzzy");
  await expect(page.getByText("No help topic matches that. Try another word.")).toBeVisible();
  await noHorizontalScroll(page);
  // Enter opens the first topic found, at the section with the words.
  await box.fill("emergency numbers");
  await box.press("Enter");
  await expect(page).toHaveURL(/#help\/assistant\/health$/);
  await expect(page.getByRole("heading", { name: "Health and first aid", level: 2 })).toBeInViewport();

  // In Serbian, typed with or without the accents.
  await setLanguage(page, "sr");
  await page.goto("/#help");
  const srBox = page.getByRole("searchbox", { name: "Pretraži pomoć" });
  await srBox.fill("sifrovanje");
  await expect(results).toContainText("Podešavanja domaćinstva");
  await srBox.fill("uredaji");
  await expect(results).toContainText("Uparivanje telefona");
  await srBox.fill("šifrovanje kopija");
  await expect(results.getByRole("link", { name: "Rezervne kopije", exact: true })).toBeVisible();
  await setLanguage(page, "en");
});
