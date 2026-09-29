import { expect, test, type Locator, type Page } from "@playwright/test";

// The hub starts empty for the whole run; tests run in order and build on
// each other (setup first). The "phone" project runs after "laptop" against
// the same hub, so every test must work whether or not setup already happened.

const PASSWORD = "correct horse";
/** The hub's "install the app" port (see start-hub.mjs). */
const INSTALL_PORT = Number(process.env.ZAKLON_E2E_PORT_BASE || 28480);

/** Ends on Settings › Devices, with the hub answering. */
async function ensureSetUp(page: Page) {
  await page.goto("/#settings/devices");
  const setup = page.getByRole("heading", { name: /Set up your household|Podesi domaćinstvo/ });
  // The paired phones show once the hub has answered.
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

/** The app's language, chosen under Settings › Language (the page stays loaded, so it holds). */
async function setLanguage(page: Page, lang: "en" | "sr") {
  await page.goto("/#settings/language");
  await page.getByRole("combobox", { name: /^(Language|Jezik)$/ }).selectOption(lang);
}

async function noHorizontalScroll(page: Page) {
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow, "page must not be wider than the screen").toBeLessThanOrEqual(1);
}

test.beforeEach(async ({ page }) => {
  // Start every test in English with a clean interface state.
  await page.addInitScript(() => {
    try {
      localStorage.setItem("zaklon.lang", "en");
    } catch {
      /* ignore */
    }
  });
});

test("first run: setup rejects mismatched passwords, then succeeds", async ({ page }) => {
  await page.goto("/");
  const setup = page.getByRole("heading", { name: "Set up your household" });
  if (await setup.isVisible({ timeout: 3000 }).catch(() => false)) {
    await page.getByLabel(/^Household password$/).fill(PASSWORD);
    await page.getByLabel("Repeat password").fill("something else");
    await page.getByRole("button", { name: "Finish setup" }).click();
    await expect(page.getByText("The passwords do not match.")).toBeVisible();
  }
  await ensureSetUp(page);
  await page.goto("/#home");
  await expect(page.getByText("Running", { exact: true })).toBeVisible();
});

test("pairing shows the download QR, the pairing QR and a 6-digit code", async ({ page }) => {
  await ensureSetUp(page);
  await page.goto("/#settings/devices");
  await page.getByRole("button", { name: "Add a phone" }).click();
  await expect(page.getByRole("heading", { name: "Pair a phone" })).toBeVisible();
  await expect(page.getByRole("img", { name: /QR code/ })).toHaveCount(2);
  await expect(page.locator(".code")).toHaveText(/^\d{6}$/);
  await expect(page.getByText(new RegExp(`http://.+:${INSTALL_PORT}/get`))).toBeVisible();
  await expect(page.locator(".sec-code")).toHaveText(/^[0-9A-F]{4} [0-9A-F]{4}$/);
  await expect(page.getByText(/Valid for [45]:\d\d/)).toBeVisible();
  // A new code replaces the old one.
  const first = await page.locator(".code").textContent();
  await page.getByRole("button", { name: "New code" }).click();
  await expect(page.locator(".code")).not.toHaveText(first ?? "");
  await page.getByRole("button", { name: "Cancel" }).click();
  await expect(page.getByRole("button", { name: "Add a phone" })).toBeVisible();
});

test("supplies: add, adjust, running low, shopping list, history, home", async ({ page }, info) => {
  await ensureSetUp(page);
  const name = `Flour ${info.project.name}`;
  await page.goto("/#supplies");
  await page.getByRole("button", { name: "Add item" }).click();
  await page.getByLabel("Name").fill(name);
  await page.getByLabel("Quantity").fill("2");
  await page.getByLabel("Unit").selectOption("kg");
  await page.getByLabel("Category").selectOption("food");
  await page.getByRole("combobox", { name: /^Place/ }).selectOption("pantry");
  await page.getByLabel("Warn below").fill("3");
  await page.getByRole("button", { name: "Save" }).click();

  const row = page.locator(".item.supply", { hasText: name });
  await expect(row).toBeVisible();
  await expect(row.locator(".qty-val")).toContainText("2");
  await expect(row.getByText("Running low")).toBeVisible();

  await row.getByRole("button", { name: /^Add one/ }).click();
  await expect(row.locator(".qty-val")).toContainText("3");
  await expect(row.getByText("Running low")).toHaveCount(0);
  await row.getByRole("button", { name: /^Use one/ }).click();
  await expect(row.locator(".qty-val")).toContainText("2");

  await page.getByRole("button", { name: "Shopping list" }).click();
  await expect(page.locator(".item.shop", { hasText: name })).toBeVisible();

  await page.getByRole("button", { name: "History" }).click();
  await expect(page.getByText(name).first()).toBeVisible();
  await expect(page.getByText("used").first()).toBeVisible();

  // Home shows it under Running low, already on the shopping list (suggested there because it runs low).
  await page.goto("/#home");
  const low = page.getByRole("region", { name: "Running low" }).locator(".sup-row", { hasText: name });
  await expect(low).toContainText("2 of 3 kg");
  await expect(low.locator(".sup-listed")).toHaveAttribute("title", "On the list");

  // Home's "+ N more" under Running low opens the items with that filter, just once.
  await page.goto("/#supplies/low");
  await expect(page.getByRole("button", { name: "Running low", exact: true })).toHaveAttribute("aria-pressed", "true");
  await expect(page).toHaveURL(/#supplies$/);
  await expect(row).toBeVisible();
  for (const shown of await page.locator(".item.supply").all()) await expect(shown.getByText("Running low")).toBeVisible();
});

test("supplies: an expired item is flagged and a bad date is refused", async ({ page }, info) => {
  await ensureSetUp(page);
  const name = `Old pills ${info.project.name}`;
  await page.goto("/#supplies");
  await page.getByRole("button", { name: "Add item" }).click();
  await page.getByLabel("Name").fill(name);
  await page.getByLabel("Category").selectOption("medicine");
  await page.getByLabel("Expiry date").fill("2020-01-31");
  await page.getByRole("button", { name: "Save" }).click();
  const row = page.locator(".item.supply", { hasText: name });
  await expect(row.getByText(/expired · 31 Jan 2020/)).toBeVisible();

  // Home's "+ N more" under Expiring opens the items with that filter: this one is there, what has no date is not.
  await page.goto("/#supplies/expiring");
  await expect(page.getByRole("button", { name: "Expiring", exact: true })).toHaveAttribute("aria-pressed", "true");
  await expect(row).toBeVisible();
  await expect(page.locator(".item.supply", { hasText: `Flour ${info.project.name}` })).toHaveCount(0);
  await page.getByRole("button", { name: "All", exact: true }).click();
  await expect(page.locator(".item.supply", { hasText: `Flour ${info.project.name}` })).toBeVisible();

  // An impossible date is refused by the hub with a clear message.
  await page.getByRole("button", { name: "Add item" }).click();
  await page.getByLabel("Name").fill(`Bad date ${info.project.name}`);
  await page.evaluate(() => {
    const el = document.querySelector('input[type="date"]') as HTMLInputElement;
    el.type = "text";
  });
  await page.getByLabel("Expiry date").fill("2027-02-31");
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByRole("alert")).toHaveText("That date does not exist.");
  await page.getByRole("button", { name: "Cancel" }).click();

  // Edit, then delete: needs a second, deliberate tap.
  await row.locator(".supply-main").click();
  await expect(page.getByRole("heading", { name: "Edit item" })).toBeVisible();
  await page.getByRole("button", { name: "Delete" }).click();
  await page.getByRole("button", { name: "Keep it" }).click();
  await expect(page.getByRole("heading", { name: "Edit item" })).toBeVisible();
  await page.getByRole("button", { name: "Delete" }).click();
  await page.getByRole("button", { name: "Yes, delete" }).click();
  await expect(page.locator(".item.supply", { hasText: name })).toHaveCount(0);
});

test("language switch to Serbian and back, with Serbian number format", async ({ page }, info) => {
  await ensureSetUp(page);
  await setLanguage(page, "sr");
  await expect(page.getByRole("heading", { name: "Jezik", level: 1 })).toBeVisible();
  await expect(page.locator("nav.nav").getByRole("button", { name: "Podešavanja" })).toHaveAttribute("aria-current", "page");
  await page.goto("/#supplies");
  await expect(page.getByRole("heading", { name: "Zalihe" })).toBeVisible();
  const name = `Šećer ${info.project.name}`;
  await page.getByRole("button", { name: "Dodaj stavku" }).click();
  await page.getByLabel("Naziv").fill(name);
  await page.getByLabel("Količina").fill("1,5");
  await page.getByLabel("Jedinica").selectOption("kg");
  await page.getByRole("button", { name: "Sačuvaj" }).click();
  await expect(page.locator(".item.supply", { hasText: name }).locator(".qty-val")).toContainText("1,5");
  await setLanguage(page, "en");
  await expect(page.getByRole("heading", { name: "Language", level: 1 })).toBeVisible();
  await expect(page.locator("nav.nav").getByRole("button", { name: "Settings" })).toHaveAttribute("aria-current", "page");
});

test("library without packs points to add-ons, which lists the catalog", async ({ page }) => {
  await ensureSetUp(page);
  await page.goto("/#library");
  await expect(page.getByText(/No knowledge packs yet/)).toBeVisible();
  await page.getByRole("button", { name: "Open Add-ons" }).click();
  await expect(page.getByRole("heading", { name: "Add-ons" })).toBeVisible();
  // The hub's drive with how full it is, then the catalog in folders.
  const library = page.locator(".drive-grid .drive-tile").first();
  await expect(library).toContainText("Zaklon library");
  await expect(library).toContainText(/free of/);
  await page.getByRole("list", { name: "Folders" }).getByRole("button", { name: /^Knowledge/ }).click();
  await expect(page.getByText("Wikipedia in Serbian (with pictures)")).toBeVisible();
});

test("every screen fits the width of the device", async ({ page }, info) => {
  await ensureSetUp(page);
  // A long name, expiring soon: it shows on Supplies and on Home, and must be cut short there, not widen the page.
  const soon = new Date(Date.now() + 5 * 86400000).toISOString().slice(0, 10);
  const name = `Extra virgin olive oil from the cooperative in Istria, 750 ml (${info.project.name})`;
  const res = await page.request.post("/api/items", { data: { name, quantity: 2, unit: "pcs", category: "food", expiry: soon, min_quantity: 3 } });
  expect(res.ok()).toBe(true);
  const settings = ["devices", "network", "backups", "privacy", "appearance", "language", "assistant", "updates", "about"].map((c) => `settings/${c}`);
  for (const tab of ["home", "tools", "library", "maps", "supplies", "assistant", "addons", "settings", "help", ...settings]) {
    await page.goto(`/#${tab}`);
    await page.waitForTimeout(300);
    await noHorizontalScroll(page);
  }
  // Supplies: the name ends in "…" and the −/+ buttons stay on the screen.
  await page.goto("/#supplies");
  const row = page.locator(".item.supply", { hasText: name });
  await expect(row).toBeVisible();
  const width = page.viewportSize()!.width;
  const plus = await row.getByRole("button", { name: /^Add one/ }).boundingBox();
  expect(plus!.x + plus!.width).toBeLessThanOrEqual(width);
  // Home: every line of the supplies stays inside its column, and a long name takes two lines at most.
  await page.goto("/#home");
  const line = page.locator(".sup-row", { hasText: name }).first();
  await expect(line).toBeVisible();
  const outside = await page.evaluate(() =>
    [...document.querySelectorAll(".sup-row")].filter((row) => {
      const col = row.closest(".sup-col")!.getBoundingClientRect();
      return [...row.querySelectorAll(".sup-name, .sup-state")].some((c) => {
        const r = c.getBoundingClientRect();
        return r.right > col.right + 1 || r.left < col.left - 1;
      });
    }).length,
  );
  expect(outside, "nothing sticks out of the supplies' columns").toBe(0);
  expect((await line.locator(".sup-name").boundingBox())!.height, "two lines at most").toBeLessThan(45);
  await noHorizontalScroll(page);
  const del = await page.request.get("/api/items");
  const item = (await del.json()).find((i: { name: string }) => i.name === name);
  await page.request.delete(`/api/items/${item.id}`);
});

test("the tab is remembered in the address", async ({ page }) => {
  await ensureSetUp(page);
  await page.request.post("/api/pinned-tool", { data: { tool: null } });
  await page.goto("/#home");
  await page.locator("nav").getByRole("button", { name: "Tools" }).click();
  await expect(page).toHaveURL(/#tools$/);
  await page.getByRole("region", { name: "Food", exact: true }).getByRole("button", { name: /^Supplies/ }).click();
  await expect(page).toHaveURL(/#supplies$/);
  await page.reload();
  await expect(page.getByRole("heading", { name: "Supplies" })).toBeVisible();
  // A tool that is not pinned is found under Tools, and the bar says so.
  await expect(page.locator("nav").getByRole("button", { name: "Tools" })).toHaveAttribute("aria-current", "true");
  // Every screen keeps its own address.
  for (const [tab, heading] of [["library", "Library"], ["maps", "Maps"], ["addons", "Add-ons"], ["assistant", "Assistant"], ["tools", "Tools"], ["help", "Help"]]) {
    await page.goto(`/#${tab}`);
    await expect(page.getByRole("heading", { name: heading, exact: true })).toBeVisible();
  }
  await page.goto("/#settings/about");
  await page.reload();
  await expect(page.getByRole("heading", { name: "About", level: 1 })).toBeVisible();
  await expect(page.locator("nav").getByRole("button", { name: "Settings" })).toHaveAttribute("aria-current", "page");
});

/** The buttons in the bar, by name. */
async function barItems(page: Page) {
  return page.locator("nav .nav-items button").allInnerTexts();
}

test("tools: every tool is listed, and one can be pinned to the bar for the whole household", async ({ page }) => {
  await ensureSetUp(page);
  // The pinned tool lives on the hub; start from none whatever ran before.
  await page.request.post("/api/pinned-tool", { data: { tool: null } });
  await page.goto("/#tools");
  await expect(page.getByRole("heading", { name: "Tools", exact: true })).toBeVisible();
  for (const name of ["Supplies", "Library", "Maps", "Power calculator", "Water calculator", "Add-ons"]) {
    await expect(page.locator(".tool-card", { hasText: name }).first()).toBeVisible();
  }
  await expect(page.getByText(/expiry dates and a shopping list/).first()).toBeVisible();
  // Supplies (food and medicines) is listed under Food and under Health and first aid,
  // the water calculator under Water and under Garden.
  await expect(page.locator(".tool-card", { hasText: "Supplies" })).toHaveCount(2);
  await expect(page.locator(".tool-card", { hasText: "Water calculator" })).toHaveCount(2);
  // Only the tools: Help is in Settings now.
  await expect(page.locator(".tool-card")).toHaveCount(8);
  await expect(page.locator(".topic-grid").getByRole("link", { name: /^Help/ })).toHaveCount(0);
  expect(await barItems(page)).toEqual(["Home", "Assistant", "Tools", "Settings"]);

  // The bar is along the bottom of the window, on the laptop too.
  const nav = await page.locator("nav").boundingBox();
  const height = page.viewportSize()!.height;
  expect(nav!.y + nav!.height).toBeGreaterThan(height - 2);
  expect(nav!.y + nav!.height).toBeLessThanOrEqual(height + 1);

  await page.getByRole("button", { name: "Pin to the bar: Maps" }).click();
  await expect(page.locator("nav").getByRole("button", { name: "Maps" })).toBeVisible();
  expect(await barItems(page)).toEqual(["Home", "Assistant", "Maps", "Tools", "Settings"]);
  await expect(page.locator(".tool-card.pinned")).toHaveCount(1);
  await expect(page.locator(".tool-card.pinned")).toContainText("Pinned");
  await noHorizontalScroll(page);
  // All five fit on the screen.
  const width = page.viewportSize()!.width;
  for (const b of await page.locator("nav .nav-items button").all()) {
    const box = await b.boundingBox();
    expect(box!.x + box!.width).toBeLessThanOrEqual(width + 1);
  }

  // It opens its screen and is the current item there.
  await page.locator("nav").getByRole("button", { name: "Maps" }).click();
  await expect(page).toHaveURL(/#maps$/);
  await expect(page.getByRole("heading", { name: "Maps", exact: true })).toBeVisible();
  await expect(page.locator("nav").getByRole("button", { name: "Maps" })).toHaveAttribute("aria-current", "page");

  // Pinning another replaces it; the hub keeps it for everyone.
  await page.goto("/#tools");
  await page.getByRole("button", { name: "Pin to the bar: Library" }).click();
  await expect(page.locator("nav").getByRole("button", { name: "Library" })).toBeVisible();
  await expect(page.locator("nav").getByRole("button", { name: "Maps" })).toHaveCount(0);
  expect((await (await page.request.get("/api/pinned-tool")).json()).tool).toBe("library");
  await page.reload();
  expect(await barItems(page)).toEqual(["Home", "Assistant", "Library", "Tools", "Settings"]);
  // Five items: none is cut short, in Serbian either (the longest is "Podešavanja").
  await setLanguage(page, "sr");
  await expect(page.locator("nav").getByRole("button", { name: "Podešavanja" })).toBeVisible();
  const cut = await page.evaluate(() =>
    [...document.querySelectorAll(".nav-label")].filter((l) => l.scrollWidth > l.clientWidth + 1).map((l) => l.textContent),
  );
  expect(cut, "labels cut short in the bar").toEqual([]);
  await setLanguage(page, "en");
  await page.goto("/#tools");

  await page.getByRole("button", { name: "Unpin: Library" }).click();
  await expect(page.locator("nav").getByRole("button", { name: "Library" })).toHaveCount(0);
  expect(await barItems(page)).toEqual(["Home", "Assistant", "Tools", "Settings"]);
  expect((await (await page.request.get("/api/pinned-tool")).json()).tool).toBe(null);
});

test("tools: sorted by topic; a topic shows its tools, and its guides open Add-ons at that topic's folder", async ({ page }) => {
  await ensureSetUp(page);
  await page.goto("/#tools");
  // The same topics as the folders of Add-ons, then Downloads (Add-ons itself).
  await expect(page.locator(".topic-panel h2")).toHaveText(["Health and first aid", "Water", "Food", "Garden", "Power", "Build and install", "Knowledge", "Maps", "Downloads"]);
  const topic = (name: string) => page.getByRole("region", { name, exact: true });
  // Each with a line on what it is about, and its tools (a topic may have none yet, only guides).
  const water = topic("Water");
  await expect(water).toContainText("Finding, cleaning and storing safe drinking water.");
  await expect(water.locator(".tool-title")).toHaveText(["Water calculator"]);
  await expect(topic("Garden").locator(".tool-title")).toHaveText(["Water calculator"]);
  await expect(topic("Build and install").locator(".tool-card")).toHaveCount(0);
  await expect(topic("Health and first aid").locator(".tool-title")).toHaveText(["Supplies"]);
  await expect(topic("Food").locator(".tool-title")).toHaveText(["Supplies"]);
  await expect(topic("Knowledge").locator(".tool-title")).toHaveText(["Library"]);
  await expect(topic("Maps").locator(".tool-title")).toHaveText(["Maps"]);
  await expect(topic("Downloads").locator(".tool-title")).toHaveText(["Add-ons"]);
  // Every topic leads to its guides; Downloads is Add-ons already.
  await expect(page.getByRole("link", { name: /^Guides for this topic: / })).toHaveCount(7);
  await expect(topic("Maps").getByRole("link", { name: "Maps to download: Maps" })).toHaveAttribute("href", "#addons/maps");
  await expect(topic("Downloads").getByRole("link")).toHaveCount(0);
  await noHorizontalScroll(page);
  // A tool opens its screen.
  await topic("Knowledge").getByRole("button", { name: /^Library/ }).click();
  await expect(page).toHaveURL(/#library$/);
  await page.goBack();

  // The guides open Add-ons at the topic's folder: the safe water guides, and the
  // sustainable living answers, which are about more than water.
  await water.getByRole("link", { name: "Guides for this topic: Water" }).click();
  await expect(page).toHaveURL(/#addons\/water$/);
  await expect(page.getByRole("heading", { name: "Water", level: 1 })).toBeVisible();
  await expect(page.getByRole("navigation", { name: "Location" }).locator('[aria-current="page"]')).toHaveText("Water");
  await expect(page.locator('[data-entry="zimgit-water-en"]')).toBeVisible();
  await expect(page.locator('[data-entry="stackexchange-sustainability-en"]')).toBeVisible();
  await expect(page.locator('[data-entry="wikimed-en"]')).toHaveCount(0);
  // The browser's (or the phone's) Back returns to Tools.
  await page.goBack();
  await expect(page).toHaveURL(/#tools$/);
  await topic("Build and install").getByRole("link", { name: /^Guides for this topic/ }).click();
  await expect(page.getByRole("heading", { name: "Build and install", level: 1 })).toBeVisible();
  await expect(page.locator('[data-entry="ifixit-en"]')).toBeVisible();

  // In Serbian, the same topics.
  await setLanguage(page, "sr");
  await page.goto("/#tools");
  await expect(page.locator(".topic-panel h2")).toHaveText(["Zdravlje i prva pomoć", "Voda", "Hrana", "Bašta", "Struja", "Ugradnja", "Znanje", "Mape", "Preuzimanja"]);
  await expect(page.getByRole("region", { name: "Voda", exact: true }).getByRole("link", { name: "Vodiči za ovu temu: Voda" })).toBeVisible();
  await noHorizontalScroll(page);
  await setLanguage(page, "en");
});

test("the logo in the bar opens the Zaklon website", async ({ page, context }, info) => {
  await ensureSetUp(page);
  // Answered here, without going online.
  await context.route(/^https:\/\/zaklon\.com\//, (r) => r.fulfill({ contentType: "text/html", body: "<title>Zaklon</title>" }));
  await page.goto("/#home");
  const logo = page.getByRole("link", { name: "Zaklon website" });
  await expect(logo).toHaveAttribute("href", "https://zaklon.com");
  await expect(logo).toHaveAttribute("rel", "noopener");
  const wordmark = logo.locator(".wordmark");
  if (info.project.name === "laptop") {
    // At rest only the mark; pointing at it brings the name in below it.
    await expect(wordmark).toHaveCSS("opacity", "0");
    await logo.hover();
    await expect(wordmark).toHaveCSS("opacity", "1");
    const mark = await logo.locator(".mark").boundingBox();
    const name = await wordmark.boundingBox();
    expect(name!.y).toBeGreaterThan(mark!.y + mark!.height - 2);
    expect(Math.abs(name!.x + name!.width / 2 - (mark!.x + mark!.width / 2))).toBeLessThan(3);
    // Not stuck to the edge of the window.
    expect(mark!.x).toBeGreaterThan(24);
  } else {
    await expect(wordmark).toBeHidden();
  }
  // A new tab (no opener: rel=noopener), not this window.
  const opened = context.waitForEvent("page");
  await logo.click();
  const site = await opened;
  await site.waitForLoadState();
  expect(site.url()).toMatch(/^https:\/\/zaklon\.com\/?$/);
  await expect(page).toHaveURL(/#home$/);
  await site.close();
});

test("an idle screen does not flood the hub with requests", async ({ page }) => {
  test.setTimeout(90_000);
  await ensureSetUp(page);
  for (const tab of ["home", "supplies", "settings", "library", "addons", "tools"]) {
    await page.goto(`/#${tab}`);
    await page.waitForTimeout(1500);
    const counts: Record<string, number> = {};
    const onRequest = (r: { url: () => string }) => {
      const path = new URL(r.url()).pathname;
      if (path.startsWith("/api/")) counts[path] = (counts[path] ?? 0) + 1;
    };
    page.on("request", onRequest);
    await page.waitForTimeout(6000);
    page.off("request", onRequest);
    const total = Object.values(counts).reduce((a, b) => a + b, 0);
    // Status every 10 s, add-ons every 10 s while idle: a handful at most.
    expect(total, `${tab}: ${JSON.stringify(counts)}`).toBeLessThanOrEqual(6);
  }
});

test("liters are written out with the right form", async ({ page }, info) => {
  await ensureSetUp(page);
  await setLanguage(page, "sr");
  await page.goto("/#supplies");
  for (const [qty, word] of [["1", "litar"], ["2", "litra"], ["5", "litara"], ["1,5", "litra"]]) {
    const name = `Mleko ${qty} ${info.project.name}`;
    await page.getByRole("button", { name: "Dodaj stavku" }).click();
    await page.getByLabel("Naziv").fill(name);
    await page.getByLabel("Količina").fill(qty);
    await page.getByLabel("Jedinica").selectOption("l");
    await page.getByRole("button", { name: "Sačuvaj" }).click();
    await expect(page.locator(".item.supply", { hasText: name }).locator(".qty-val")).toContainText(word);
  }
  await setLanguage(page, "en");
});

test("the Latin-script switch is remembered on this device", async ({ page }) => {
  await ensureSetUp(page);
  await page.goto("/#settings/language");
  const box = page.getByRole("checkbox", { name: /Serbian articles in Latin script/ });
  await expect(box).not.toBeChecked();
  await box.check();
  await page.reload();
  await expect(page.getByRole("checkbox", { name: /Serbian articles in Latin script/ })).toBeChecked();
  await page.getByRole("checkbox", { name: /Serbian articles in Latin script/ }).uncheck();
});

test("shopping: bought goes to Put away, which adds a dated batch", async ({ page }, info) => {
  await ensureSetUp(page);
  const name = `Rice ${info.project.name}`;
  await page.goto("/#supplies");
  // An item that is running low shows on the shopping list.
  await page.getByRole("button", { name: "Add item" }).click();
  await page.getByLabel("Name").fill(name);
  await page.getByLabel("Quantity").fill("1");
  await page.getByLabel("Unit").selectOption("kg");
  await page.getByLabel("Warn below").fill("2");
  await page.getByRole("button", { name: "Save" }).click();
  await page.getByRole("button", { name: "Shopping list" }).click();
  const entry = page.locator(".item.shop", { hasText: name });
  await expect(entry).toBeVisible();

  // Bought in the shop: off the list, on "Put away".
  await entry.getByRole("button", { name: `Bought: ${name}` }).click();
  await expect(page.locator(".item.shop", { hasText: name })).toHaveCount(0);
  await page.getByRole("button", { name: /^Put away/ }).first().click();
  const card = page.getByLabel(name);
  await card.getByLabel("Quantity").fill("3");
  await card.getByLabel("Expiry date").fill("2031-03-31");
  await card.getByRole("button", { name: "Put away" }).click();
  await expect(page.getByLabel(name)).toHaveCount(0);

  // The item now has two batches: 1 kg without a date and 3 kg dated.
  await page.getByRole("button", { name: "Items" }).click();
  const row = page.locator(".item.supply", { hasText: name });
  await expect(row.locator(".qty-val")).toContainText("4");
  await expect(row.getByText("Running low")).toHaveCount(0);
  await expect(row.getByText("2 batches")).toBeVisible();
  await row.locator(".supply-main").click();
  await expect(page.locator(".batch-row")).toHaveCount(3); // two batches + the "add" row
  // Using 2 takes from the dated batch first.
  await page.getByRole("button", { name: "Cancel" }).click();
  await row.getByRole("button", { name: /^Use one/ }).click();
  await row.getByRole("button", { name: /^Use one/ }).click();
  await expect(row.locator(".qty-val")).toContainText("2");
  await row.locator(".supply-main").click();
  await expect(page.locator(".batch-row").first().locator('input[type="date"]')).toHaveValue("2031-03-31");
  await expect(page.locator(".batch-row").first().locator('input[inputmode="decimal"]')).toHaveValue("1");
  await page.getByRole("button", { name: "Cancel" }).click();

  // "Delete" on the list: not bought, gone from the list.
  await page.getByRole("button", { name: "Shopping list" }).click();
  await page.getByLabel("Add to the list…").first().fill(`Candles ${info.project.name}`);
  await page.getByRole("button", { name: "Add to the list…" }).click();
  const candles = page.locator(".item.shop", { hasText: `Candles ${info.project.name}` });
  await candles.getByRole("button", { name: /^Delete/ }).click();
  await expect(candles).toHaveCount(0);
});

test("maps: the phone steps show the hub address, and the world is searchable", async ({ page }) => {
  await ensureSetUp(page);
  await page.goto("/#maps/navigation");
  await expect(page.getByRole("heading", { name: "Maps", exact: true })).toBeVisible();
  await expect(page.getByRole("tab", { name: "Navigation" })).toHaveAttribute("aria-selected", "true");
  await expect(page.getByRole("heading", { name: "Maps on a phone" })).toBeVisible();
  await expect(page.locator("code.url").first()).toHaveText(/^http:\/\/.+:\d+\/$/);
  await expect(page.getByText(/added to the hub with the first map/)).toBeVisible();
  const search = page.getByRole("searchbox", { name: /Search a country/ });
  await search.fill("serbia");
  const serbia = page.locator(".map-country").filter({ hasText: /^Serbia/ });
  await expect(serbia).toHaveCount(1);
  await expect(serbia.getByRole("button", { name: "Download" })).toBeVisible();
  await search.fill("srbija"); // Serbian names are searchable too
  await expect(serbia).toHaveCount(1);
  await search.fill("germany");
  const germany = page.locator(".map-country").filter({ hasText: /^Germany/ });
  await expect(germany.getByText(/regions/)).toBeVisible();
  await germany.getByRole("button", { name: /Regions of/ }).click();
  await expect(germany.locator(".region").first()).toBeVisible();
  expect(await germany.locator(".region").count()).toBeGreaterThan(5);
  await search.fill("zzzz-nowhere");
  await expect(page.getByText("Nothing found.")).toBeVisible();
  await noHorizontalScroll(page);
  // In Add-ons the maps are a folder of countries (not their pieces), which points here for phones.
  await page.goto("/#addons/maps");
  await expect(page.getByRole("heading", { name: "Maps", exact: true })).toBeVisible();
  await expect(page.getByText(/Phones show maps in the CoMaps app/)).toBeVisible();
  await expect(page.locator('[data-entry="map:Montenegro"]')).toBeVisible();
  await expect(page.locator('[data-entry^="map:Germany"]')).toHaveCount(1);
  await page.getByRole("link", { name: "Open Maps" }).click();
  await expect(page).toHaveURL(/#maps\/navigation$/);
  await expect(page.getByRole("heading", { name: "Maps on a phone" })).toBeVisible();
});

test("settings: accent color is remembered, password can be changed, hub facts and privacy are shown", async ({ page }) => {
  await ensureSetUp(page);
  await page.goto("/#settings/appearance");
  await page.getByRole("radio", { name: "Blue" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-accent", "blue");
  await page.getByLabel(/Pure black background/).check();
  await expect(page.locator("html")).toHaveAttribute("data-oled", "1");
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-accent", "blue");
  await expect(page.getByRole("radio", { name: "Blue" })).toHaveAttribute("aria-checked", "true");
  await page.getByRole("radio", { name: "Amber" }).click();
  await page.getByLabel(/Pure black background/).uncheck();

  await page.goto("/#settings/privacy");
  const form = page.locator("form").filter({ has: page.getByRole("heading", { name: "Household password" }) });
  await form.getByLabel("New password").fill("something else");
  await form.getByLabel("Repeat password").fill("something elsE");
  await form.getByRole("button", { name: "Change password" }).click();
  await expect(form.getByRole("alert")).toHaveText(/do not match/i);
  await form.getByLabel("New password").fill(PASSWORD);
  await form.getByLabel("Repeat password").fill(PASSWORD);
  await form.getByRole("button", { name: "Change password" }).click();
  await expect(form.getByText("Password changed.")).toBeVisible();
  await expect(page.getByRole("heading", { name: "Privacy", exact: true })).toBeVisible();
  await expect(page.getByText(/No tracking, no analytics/)).toBeVisible();
  await noHorizontalScroll(page);

  await page.goto("/#settings/about");
  await expect(page.getByRole("heading", { name: "This hub" })).toBeVisible();
  await expect(page.getByText("Memory", { exact: true })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Licenses and credits" })).toBeVisible();
  await expect(page.getByText("Kiwix (kiwix-serve)")).toBeVisible();
  await expect(page.getByText(/free and open source \(GPL-3\.0-or-later\), forever/)).toBeVisible();
  await noHorizontalScroll(page);
});

test("add-ons: copy to USB and import offer the laptop's drives", async ({ page }) => {
  await ensureSetUp(page);
  await page.goto("/#addons");
  await expect(page.getByRole("heading", { name: "Copy to USB" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Import from a USB stick or folder" })).toBeVisible();
  const importDrive = page.getByRole("combobox", { name: "Import from a USB stick or folder", exact: true });
  await expect.poll(async () => importDrive.locator("option").count()).toBeGreaterThan(1);
  await expect(importDrive.locator("option").nth(1)).toHaveText(/[A-Z]:.*free/);
  await noHorizontalScroll(page);
  // The laptop's other drives are tiles under "Devices and drives"; one opens with
  // Copy and Import already set to it. (The test machine has more than one drive.)
  // A pack made to look installed, so there is something to copy.
  await page.route("**/api/catalog", async (r) => {
    const json = await (await r.fetch()).json();
    const pack = json.packs.find((p: { id: string }) => p.id === "wikimed-en");
    pack.state = { ...pack.state, status: "installed", bytes_done: pack.size, bytes_total: pack.size };
    return r.fulfill({ json });
  });
  await page.reload();
  await expect(page.getByRole("checkbox", { name: /Medical Wikipedia/ })).toBeVisible();
  const other = page.locator(".drive-grid button.drive-tile").nth(1);
  const name = await other.locator(".drive-title").innerText();
  const letter = name.match(/\(([A-Z]):\)$/)![1];
  await other.click();
  await expect(page).toHaveURL(new RegExp(`#addons/drive/${letter}$`));
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(name);
  await expect(page.getByRole("combobox", { name: "Drive", exact: true })).toHaveValue(`${letter}:\\`);
  await expect(page.getByRole("combobox", { name: "Import from a USB stick or folder", exact: true })).toHaveValue(`${letter}:\\`);
  await noHorizontalScroll(page);
  await page.getByRole("button", { name: "Back", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Add-ons", exact: true })).toBeVisible();
});

test("add-ons: a folder opens; the breadcrumb, Back and the browser's Back return to the top", async ({ page }) => {
  await ensureSetUp(page);
  await page.goto("/#addons");
  const folders = page.getByRole("list", { name: "Folders" });
  // The topics (the same as on the Tools screen), then AI models and programs.
  await expect(folders.locator(".folder-name")).toHaveText([
    "Health and first aid", "Water", "Food", "Garden", "Power", "Build and install", "Knowledge", "Maps", "AI models", "Programs",
  ]);
  await expect(folders.getByRole("button", { name: /^AI models/ })).toContainText(/4 add-ons/);
  await folders.getByRole("button", { name: /^AI models/ }).click();
  await expect(page).toHaveURL(/#addons\/models$/);
  await expect(page.getByRole("heading", { name: "AI models", exact: true })).toBeVisible();
  const crumbs = page.getByRole("navigation", { name: "Location" });
  await expect(crumbs.locator('[aria-current="page"]')).toHaveText("AI models");
  await expect(page.locator('[data-entry="qwen35-4b"]')).toContainText("Apache-2.0");
  await expect(page.locator('[data-entry="kiwix-tools"]')).toHaveCount(0);
  // A download the hub accepts without a body (202) is not an error.
  let asked = false;
  await page.route("**/api/packs/qwen35-08b/download", (route) => {
    asked = true;
    return route.fulfill({ status: 202, body: "" });
  });
  await page.locator('[data-entry="qwen35-08b"]').getByRole("button", { name: /^Download/ }).click();
  await expect.poll(() => asked).toBe(true);
  await page.waitForTimeout(300);
  await expect(page.getByRole("alert")).toHaveCount(0);
  await noHorizontalScroll(page);

  // The breadcrumb goes back to the top.
  await crumbs.getByRole("button", { name: "Add-ons" }).click();
  await expect(page).toHaveURL(/#addons$/);
  await expect(page.getByRole("heading", { name: "Add-ons", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Back", exact: true })).toBeDisabled();
  // So does Back...
  await folders.getByRole("button", { name: /^Programs/ }).click();
  await expect(page.locator('[data-entry="kiwix-tools"]')).toBeVisible();
  await page.getByRole("button", { name: "Back", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Add-ons", exact: true })).toBeVisible();
  // ...and the browser's (or the phone's) Back.
  await folders.getByRole("button", { name: /^Health and first aid/ }).click();
  await expect(page.locator('[data-entry="wikimed-en"]')).toBeVisible();
  await page.goBack();
  await expect(page.getByRole("heading", { name: "Add-ons", exact: true })).toBeVisible();

  // The hub's drive opens with what is on it.
  await page.locator(".drive-grid button.drive-tile").first().click();
  await expect(page).toHaveURL(/#addons\/library$/);
  await expect(page.getByText(/Everything Zaklon keeps on this drive/)).toBeVisible();
  await page.getByRole("button", { name: "Back", exact: true }).click();
  await expect(page).toHaveURL(/#addons$/);

  // Straight into a folder from elsewhere: Back goes up to Add-ons, not away from it.
  await page.goto("/#addons/programs");
  await expect(page.getByRole("heading", { name: "Programs", exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Back", exact: true }).click();
  await expect(page).toHaveURL(/#addons$/);
  await expect(page.getByRole("heading", { name: "Add-ons", exact: true })).toBeVisible();
});

test("add-ons: the search finds packs and countries in every folder", async ({ page }) => {
  await ensureSetUp(page);
  await page.goto("/#addons/models");
  const search = page.getByRole("searchbox", { name: "Search add-ons" });
  await search.fill("wikipedia");
  await expect(page.getByRole("heading", { name: /^Search results/ })).toBeVisible();
  await expect(page.getByRole("navigation", { name: "Location" }).locator('[aria-current="page"]')).toHaveText("Search results");
  await expect(page.locator('[data-entry="wikipedia-sr-maxi"]')).toBeVisible();
  // Medical Wikipedia is in another folder, and says so.
  await expect(page.locator('[data-entry="wikimed-en"]')).toContainText("Health and first aid");
  await expect(page.locator('[data-entry="qwen35-2b"]')).toHaveCount(0);
  // Serbian titles, words in any order, and countries by name (in either language).
  await search.fill("srpskom vikipedija");
  await expect(page.locator('[data-entry="wikipedia-sr-maxi"]')).toBeVisible();
  await search.fill("crna gora");
  await expect(page.locator('[data-entry="map:Montenegro"]')).toBeVisible();
  await search.fill("zzzz-nowhere");
  await expect(page.getByText("Nothing found.")).toBeVisible();
  await noHorizontalScroll(page);
  // Back ends the search, where it started.
  await page.getByRole("button", { name: "Back", exact: true }).click();
  await expect(search).toHaveValue("");
  await expect(page.getByRole("heading", { name: "AI models", exact: true })).toBeVisible();
});

test("add-ons: tiles or details, remembered on this device", async ({ page }) => {
  await ensureSetUp(page);
  await page.goto("/#addons");
  const view = page.getByRole("group", { name: "View" });
  await expect(view.getByRole("button", { name: "Tiles" })).toHaveAttribute("aria-pressed", "true");
  await view.getByRole("button", { name: "Details" }).click();
  await expect(view.getByRole("button", { name: "Details" })).toHaveAttribute("aria-pressed", "true");
  // The folders as a table (its column names are left out on a phone).
  const table = page.getByRole("table", { name: "Folders" });
  await expect(table.getByRole("columnheader", { name: "Items", includeHidden: true })).toHaveCount(1);
  await noHorizontalScroll(page);
  await table.getByRole("button", { name: /^Knowledge/ }).click();
  const row = page.getByRole("table", { name: "Knowledge", exact: true }).getByRole("row").filter({ hasText: "Wikipedia in Serbian (with pictures)" });
  await expect(row).toContainText("CC BY-SA 4.0");
  await expect(row).toContainText("Not downloaded");
  await expect(row.getByRole("button", { name: /^Download/ })).toBeVisible();
  await noHorizontalScroll(page);
  // Still details after a reload; back to tiles.
  await page.reload();
  await expect(page.getByRole("table", { name: "Knowledge", exact: true })).toBeVisible();
  await page.getByRole("group", { name: "View" }).getByRole("button", { name: "Tiles" }).click();
  await expect(page.getByRole("list", { name: "Knowledge", exact: true })).toBeVisible();
  expect(await page.evaluate(() => localStorage.getItem("zaklon.addonsView"))).toBe("tiles");
});

test("add-ons: a pack about several topics is in the folder of each, and counted in each", async ({ page }) => {
  await ensureSetUp(page);
  const catalog = await (await page.request.get("/api/catalog")).json();
  const about = (topic: string) => catalog.packs.filter((p: { topics?: string[] }) => p.topics?.includes(topic)).length;
  const water = '[data-entry="zimgit-water-en"]';
  // The safe water guides are about water and health.
  for (const [id, name] of [["water", "Water"], ["health", "Health and first aid"]]) {
    await page.goto(`/#addons/${id}`);
    await expect(page.getByRole("heading", { name, level: 1 })).toBeVisible();
    await expect(page.locator(water)).toContainText("Safe water guides (English)");
    // Everything about the topic, nothing else.
    await expect(page.locator("[data-entry]")).toHaveCount(about(id));
  }
  // The sustainable living answers: power, water, garden, and building and installing.
  for (const id of ["power", "water", "garden", "build"]) {
    await page.goto(`/#addons/${id}`);
    await expect(page.locator('[data-entry="stackexchange-sustainability-en"]')).toBeVisible();
  }
  await page.goto("/#addons/knowledge");
  await expect(page.locator('[data-entry="wikipedia-sr-maxi"]')).toBeVisible();
  await expect(page.locator('[data-entry="stackexchange-sustainability-en"]')).toHaveCount(0);
  // Each folder counts all it holds, so a pack counts in each of its folders.
  await page.goto("/#addons");
  const folders = page.getByRole("list", { name: "Folders" });
  expect(about("water")).toBeGreaterThan(1);
  for (const [id, name] of [["water", "Water"], ["health", "Health and first aid"], ["power", "Power"]]) {
    const n = about(id);
    await expect(folders.getByRole("button", { name: new RegExp(`^${name}`) })).toContainText(`${n} ${n === 1 ? "add-on" : "add-ons"} ·`);
  }
  // The search lists it once, with every folder it is in.
  await page.getByRole("searchbox", { name: "Search add-ons" }).fill("safe water");
  await expect(page.locator(water)).toHaveCount(1);
  await expect(page.locator(water)).toContainText("Water, Health and first aid");
  await noHorizontalScroll(page);
});

test("add-ons: the addresses of the folders from before the topics lead to the new folders", async ({ page }) => {
  await ensureSetUp(page);
  // "Wikipedia and books" is Knowledge now, "Repair and skills" Build and install.
  await page.goto("/#addons/reference");
  await expect(page).toHaveURL(/#addons\/knowledge$/);
  await expect(page.getByRole("heading", { name: "Knowledge", level: 1 })).toBeVisible();
  await expect(page.locator('[data-entry="wikipedia-sr-maxi"]')).toBeVisible();
  await page.goto("/#addons/skills");
  await expect(page).toHaveURL(/#addons\/build$/);
  await expect(page.getByRole("heading", { name: "Build and install", level: 1 })).toBeVisible();
  await expect(page.locator('[data-entry="ifixit-en"]')).toBeVisible();
  // The others kept their addresses ("Other knowledge" was #addons/knowledge).
  for (const [id, name, pack] of [
    ["health", "Health and first aid", "wikimed-en"],
    ["garden", "Garden", "gardenology-en"],
    ["knowledge", "Knowledge", "wikibooks-sr"],
    ["models", "AI models", "qwen35-4b"],
    ["programs", "Programs", "kiwix-tools"],
  ]) {
    await page.goto(`/#addons/${id}`);
    await expect(page.getByRole("heading", { name, level: 1 })).toBeVisible();
    await expect(page.locator(`[data-entry="${pack}"]`)).toBeVisible();
  }
  // A link inside the app to an old address, and the app opened fresh at one.
  await page.evaluate(() => {
    location.hash = "addons/skills";
  });
  await expect(page).toHaveURL(/#addons\/build$/);
  await expect(page.getByRole("heading", { name: "Build and install", level: 1 })).toBeVisible();
  await page.goto("about:blank");
  await page.goto("/#addons/reference");
  await expect(page).toHaveURL(/#addons\/knowledge$/);
  await expect(page.getByRole("heading", { name: "Knowledge", level: 1 })).toBeVisible();
  // Back goes up to Add-ons, not away from it.
  await page.getByRole("button", { name: "Back", exact: true }).click();
  await expect(page).toHaveURL(/#addons$/);
});

test("add-ons: packs people download themselves are apart, say why, and ask to confirm their license (requests intercepted)", async ({ page }) => {
  await ensureSetUp(page);
  // The hub itself refuses one without the confirmation (nothing is downloaded).
  const refused = await page.request.post("/api/packs/ifixit-en/download");
  expect(refused.status()).toBe(400);
  expect((await refused.json()).code).toBe("license_not_confirmed");
  const asked: string[] = [];
  await page.route("**/api/packs/*/download*", (r) => {
    asked.push(new URL(r.request().url()).pathname + new URL(r.request().url()).search);
    return r.fulfill({ status: 202, body: "" });
  });
  await page.goto("/#addons/build");
  await expect(page.getByRole("heading", { name: "Build and install", level: 1 })).toBeVisible();
  // Offered to everyone first; iFixit (non-commercial) at the bottom, in a group of its own, with the reason.
  const group = page.getByRole("region", { name: "You download these yourself" });
  await expect(group).toContainText("Zaklon never downloads them for you");
  const ifixit = group.locator('[data-entry="ifixit-en"]');
  await expect(ifixit).toContainText("Free for non-commercial use only (CC BY-NC-SA 3.0)");
  await expect(ifixit.getByText("recommended")).toHaveCount(0);
  await expect(group.locator('[data-entry="restarters-en"]')).toHaveCount(0);
  await expect(page.locator('[data-entry="restarters-en"]')).toBeVisible();
  const order = await page.locator("[data-entry]").evaluateAll((els) => els.map((e) => e.getAttribute("data-entry")));
  expect(order.indexOf("restarters-en")).toBeLessThan(order.indexOf("ifixit-en"));
  // Its license and where it comes from, in its details.
  await ifixit.getByText("License and credit").click();
  await expect(ifixit).toContainText("iFixit and its contributors");

  // Download asks first, naming the license; Cancel asks nothing of the hub.
  await ifixit.getByRole("button", { name: "Download: iFixit repair guides (English)" }).click();
  const question = ifixit.locator(".license-ask");
  await expect(question).toHaveAttribute("role", "group");
  await expect(question).toContainText("CC BY-NC-SA 3.0");
  await expect(question).toContainText("free for non-commercial use only");
  await expect(question.getByRole("button", { name: "Accept and download: iFixit repair guides (English)" })).toBeFocused();
  await noHorizontalScroll(page);
  await question.getByRole("button", { name: "Cancel" }).click();
  await expect(question).toHaveCount(0);
  await expect(ifixit.getByRole("button", { name: /^Download/ })).toBeFocused();
  await page.waitForTimeout(300);
  expect(asked).toEqual([]);
  // Accepted: downloaded, with the confirmation.
  await ifixit.getByRole("button", { name: /^Download/ }).click();
  await ifixit.getByRole("button", { name: /^Accept and download/ }).click();
  await expect.poll(() => asked).toEqual(["/api/packs/ifixit-en/download?accept_license=true"]);
  // A pack offered to everyone downloads straight away.
  await page.locator('[data-entry="restarters-en"]').getByRole("button", { name: /^Download/ }).click();
  await expect.poll(() => asked.length).toBe(2);
  expect(asked[1]).toBe("/api/packs/restarters-en/download");

  // The same in the details table, and in Serbian.
  await page.getByRole("group", { name: "View" }).getByRole("button", { name: "Details" }).click();
  const table = page.getByRole("table", { name: "You download these yourself" });
  await table.getByRole("button", { name: /^Download: iFixit/ }).click();
  await expect(table.locator(".license-ask")).toContainText("CC BY-NC-SA 3.0");
  await noHorizontalScroll(page);
  await page.getByRole("group", { name: "View" }).getByRole("button", { name: "Tiles" }).click();
  await setLanguage(page, "sr");
  await page.goto("/#addons/food");
  const grupa = page.getByRole("region", { name: "Ove preuzimaš sam" });
  await expect(grupa.locator('[data-entry="grimgrains-en"]')).toContainText("Besplatno samo za nekomercijalnu upotrebu (CC BY-NC-SA 4.0)");
  await page.goto("/#addons/water");
  await expect(page.getByRole("region", { name: "Ove preuzimaš sam" }).locator('[data-entry="zimgit-water-en"]')).toContainText("Mešovite licence");
  await setLanguage(page, "en");
});

test("add-ons: the starter set holds only packs offered to everyone", async ({ page }) => {
  await ensureSetUp(page);
  const catalog = await (await page.request.get("/api/catalog")).json();
  const offer = new Map<string, string>(catalog.packs.map((p: { id: string; offer: string }) => [p.id, p.offer]));
  expect(catalog.starter_sets.map((s: { lang: string }) => s.lang)).toEqual(["sr", "en"]);
  for (const set of catalog.starter_sets) for (const id of set.packs) expect(offer.get(id), id).toBe("auto");
  // Even when a catalog tries to put others in, the set leaves them out.
  await page.route("**/api/catalog", async (r) => {
    const json = await (await r.fetch()).json();
    json.system.disk_free = 1e12;
    json.starter_sets = [{ lang: "en", packs: ["ifixit-en", "zimgit-water-en", "military-medicine-en"] }];
    return r.fulfill({ json });
  });
  const asked: string[] = [];
  await page.route("**/api/packs/*/download*", (r) => {
    asked.push(new URL(r.request().url()).pathname);
    return r.fulfill({ status: 202, body: "" });
  });
  await page.goto("/#addons");
  const panel = page.locator(".starter");
  await expect(panel.getByRole("heading", { name: "English essentials" })).toBeVisible();
  await expect(panel).toContainText("First aid and field medicine manuals (English)");
  await expect(panel).not.toContainText("iFixit");
  await expect(panel).not.toContainText("Safe water");
  await expect(panel.getByRole("button", { name: "Download all" })).toBeEnabled();
  const lines = await panel.getByRole("listitem").count();
  await panel.getByRole("button", { name: "Download all" }).click();
  // Every line of the set asked for (the AI model last), before the made-up answers stop:
  // a request after that would reach the real hub and really download.
  await expect.poll(() => asked.length).toBe(lines);
  expect(asked).toContain("/api/packs/military-medicine-en/download");
  expect(asked.filter((p) => /ifixit|zimgit/.test(p))).toEqual([]);
  await page.unrouteAll({ behavior: "ignoreErrors" });
});

test("add-ons: a pack Zaklon no longer offers stays listed, usable and removable (states simulated)", async ({ page }) => {
  await ensureSetUp(page);
  // As the hub lists a pack it has that the catalog withdrew.
  await page.route("**/api/catalog", async (r) => {
    const json = await (await r.fetch()).json();
    json.packs.push({
      id: "zimgit-medicine-en",
      title: { en: "First aid and medicine guides (English)", sr: "Vodiči za prvu pomoć i medicinu (engleski)" },
      description: { en: "Field manuals on first aid and medical care when no doctor is available.", sr: "Priručnici za prvu pomoć i lečenje kad lekar nije dostupan." },
      category: "knowledge",
      topics: ["health"],
      version: "2024-08",
      size: 70179585,
      license: "Various; see each document",
      attribution: "Various authors, collected by Kiwix",
      source: "https://library.kiwix.org",
      offer: "auto",
      languages: ["eng"],
      recommended_for: [],
      withdrawn: true,
      state: { status: "installed", bytes_done: 70179585, bytes_total: 70179585, speed: 0 },
    });
    return r.fulfill({ json });
  });
  let removed = false;
  await page.route("**/api/packs/zimgit-medicine-en", (r) => {
    removed = r.request().method() === "DELETE";
    return r.fulfill({ status: 204, body: "" });
  });
  await page.goto("/#addons/health");
  const pack = page.locator('[data-entry="zimgit-medicine-en"]');
  await expect(pack).toContainText("No longer offered by Zaklon");
  await expect(pack).toContainText("Installed");
  // Nothing to download or update; it can be removed on the laptop.
  await expect(pack.getByRole("button", { name: /^(Download|Update|Resume|Retry)/ })).toHaveCount(0);
  await expect(page.getByRole("region", { name: "You download these yourself" }).locator('[data-entry="zimgit-medicine-en"]')).toHaveCount(0);
  // On the library's drive, with what else is there; not among what can be copied to USB.
  await page.goto("/#addons/library");
  await expect(page.locator('[data-entry="zimgit-medicine-en"]')).toContainText("No longer offered by Zaklon");
  await page.goto("/#addons");
  await expect(page.getByRole("heading", { name: "Copy to USB" })).toBeVisible();
  await expect(page.getByRole("checkbox", { name: /First aid and medicine guides/ })).toHaveCount(0);
  await page.goto("/#addons/health");
  await pack.getByRole("button", { name: "Remove: First aid and medicine guides (English)" }).click();
  await pack.getByRole("button", { name: "Yes, remove" }).click();
  await expect.poll(() => removed).toBe(true);
  // The screen asks again every few seconds: stop the made-up answers before the page closes.
  await page.unrouteAll({ behavior: "ignoreErrors" });
  await noHorizontalScroll(page);
});

test("add-ons: the world map names the build it offers, and a newer one as an update (states simulated)", async ({ page }) => {
  await ensureSetUp(page);
  const GB = 1024 ** 3;
  // As the hub describes the world map: the build Protomaps' list offers, and the one on the hub.
  let world: Record<string, unknown> = {
    offered: "20260811",
    offered_size: 128 * GB,
    installed: null,
    installed_size: 0,
    update: false,
    needed: 129 * GB,
    disk_free: 500 * GB,
    room_for_both: true,
    listed: true,
    checked_at: "2026-09-29T09:00:00Z",
  };
  let state: Record<string, unknown> = { status: "not_installed", bytes_done: 0, bytes_total: 128 * GB, speed: 0 };
  let checked = 0;
  await page.route("**/api/world-map/check", (r) => {
    checked++;
    return r.fulfill({ json: { checking: false } });
  });
  await page.route("**/api/catalog", async (r) => {
    const json = await (await r.fetch()).json();
    const pack = json.packs.find((p: { id: string }) => p.id === "world-map");
    Object.assign(pack, { version: world.offered, size: world.offered_size, state });
    json.world = world;
    return r.fulfill({ json });
  });
  const asked: unknown[] = [];
  await page.route("**/api/world-map/update", (r) => {
    asked.push(r.request().postDataJSON());
    return r.fulfill({ status: 202, body: "" });
  });
  await page.goto("/#addons/maps");
  await expect.poll(() => checked, { message: "opening Add-ons lets the hub look for a newer build" }).toBeGreaterThan(0);
  const entry = page.locator('[data-entry="world-map"]');
  await expect(entry).toContainText("Map data of 11 Aug 2026");
  await expect(entry).toContainText("Not downloaded");
  await expect(entry.getByRole("button", { name: "Download: World map (towns, streets and buildings)" })).toBeVisible();

  // An older build on the hub and a newer one offered: an update, with room for both maps.
  world = { ...world, offered: "20261019", offered_size: 139 * GB, installed: "20260811", installed_size: 128 * GB, update: true, needed: 140 * GB };
  state = { status: "installed", bytes_done: 139 * GB, bytes_total: 139 * GB, speed: 0, update_available: true };
  await page.reload();
  await expect(entry).toContainText("A newer world map is available (19 Oct 2026, 139 GB)");
  await expect(entry).toContainText("Map data of 11 Aug 2026");
  await expect(entry.getByRole("button", { name: /^(Download|Update|Resume|Retry):/ })).toHaveCount(0);
  await entry.getByRole("button", { name: "Update the map: World map (towns, streets and buildings)" }).click();
  await expect.poll(() => asked).toEqual([{ remove_old: false }]);

  // No room for both: it says so, and the old map is removed first only once confirmed.
  world = { ...world, disk_free: 100 * GB, room_for_both: false };
  await page.reload();
  await entry.getByRole("button", { name: /^Update the map/ }).click();
  const noRoom = entry.getByText("There is not enough free space for the new map next to the old one: it needs 140 GB, and 100 GB is free.", { exact: false });
  await expect(noRoom).toBeVisible();
  await entry.getByRole("button", { name: "Cancel" }).click();
  await expect(noRoom).toHaveCount(0);
  expect(asked).toHaveLength(1);
  await entry.getByRole("button", { name: /^Update the map/ }).click();
  await entry.getByRole("button", { name: "Remove the old map and update: World map (towns, streets and buildings)" }).click();
  await expect.poll(() => asked).toEqual([{ remove_old: false }, { remove_old: true }]);
  await noHorizontalScroll(page);

  // While the newer build downloads, the map on the hub keeps being used.
  state = { status: "downloading", bytes_done: 13 * GB, bytes_total: 139 * GB, speed: 20 * 1024 ** 2, update_available: true };
  await page.reload();
  await expect(entry).toContainText("The current map is used until the new one is downloaded and checked");
  await expect(entry.getByRole("button", { name: /^Pause/ })).toBeVisible();
  // The screen asks again every few seconds: stop the made-up answers before the page closes.
  await page.unrouteAll({ behavior: "ignoreErrors" });
});

test("a download that the hub accepts without a body is not reported as an error", async ({ page }) => {
  await ensureSetUp(page);
  // The hub answers 202 with an empty body; answer the same way without going online.
  let asked = false;
  await page.route("**/api/maps/Montenegro/download", (route) => {
    asked = true;
    return route.fulfill({ status: 202, body: "" });
  });
  await page.goto("/#maps/navigation");
  await page.getByRole("searchbox", { name: /Search a country/ }).fill("montenegro");
  await page.locator(".map-country").filter({ hasText: /^Montenegro/ }).getByRole("button", { name: "Download" }).click();
  await expect.poll(() => asked).toBe(true);
  await page.waitForTimeout(300);
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("assistant: without a model it offers the one that fits this computer", async ({ page }) => {
  await ensureSetUp(page);
  await page.goto("/#assistant");
  await expect(page.getByRole("heading", { name: "Assistant", exact: true })).toBeVisible();
  await expect(page.getByRole("heading", { name: "The assistant needs an AI model" })).toBeVisible();
  await expect(page.getByText(/Recommended for this computer with/)).toBeVisible();
  await expect(page.getByRole("button", { name: "Download" })).toBeVisible();
  await page.getByRole("button", { name: "Other models" }).click();
  await expect(page.getByRole("heading", { name: "Add-ons" })).toBeVisible();
  await page.getByRole("list", { name: "Folders" }).getByRole("button", { name: /^Programs/ }).click();
  await expect(page.getByText("AI engine (llama.cpp)")).toBeVisible();
});

/** The hub's assistant on an 8 GB computer with the 2B and the 9B model installed; the 9B does not fit. */
const EIGHT_GB = {
  engine: "stopped", engine_installed: true, selected: "qwen35-2b", recommended: "qwen35-2b", ram_total: 8.2e9, books: 1,
  models: [
    { id: "qwen35-08b", title_en: "Small AI model (Qwen3.5 0.8B)", title_sr: "x", size: 8.3e8, installed: false, recommended: false, fits: true, needs_ram: 3.4e9 },
    { id: "qwen35-2b", title_en: "AI model for phones (Qwen3.5 2B)", title_sr: "x", size: 1.3e9, installed: true, recommended: true, fits: true, needs_ram: 3.9e9 },
    { id: "qwen35-4b", title_en: "AI model for laptops (Qwen3.5 4B)", title_sr: "x", size: 2.7e9, installed: false, recommended: false, fits: true, needs_ram: 5.8e9 },
    { id: "qwen35-9b", title_en: "AI model for 16 GB computers (Qwen3.5 9B)", title_sr: "x", size: 5.7e9, installed: true, recommended: false, fits: false, needs_ram: 8.8e9 },
  ],
};

test("assistant: a model that needs more memory than the computer has is marked and cannot be chosen", async ({ page }) => {
  await ensureSetUp(page);
  await page.route("**/api/assistant", (route) => route.fulfill({ json: EIGHT_GB }));
  let chosen: string | null = null;
  await page.route("**/api/assistant/model", (route) => {
    chosen = route.request().postDataJSON().id;
    return route.fulfill({ status: 204, body: "" });
  });
  await page.goto("/#assistant");
  const model = page.getByRole("combobox", { name: "Model" });
  await expect(model).toHaveValue("qwen35-2b");
  const big = model.locator('option[value="qwen35-9b"]');
  await expect(big).toHaveText(/9B · needs more memory$/);
  // Playwright does not count an option as disabled; the property says it.
  await expect(big).toHaveJSProperty("disabled", true);
  await expect(model.locator('option[value="qwen35-2b"]')).toHaveText(/2B · recommended$/);
  await expect(model.locator('option[value="qwen35-2b"]')).toHaveJSProperty("disabled", false);

  // The same on Settings › AI assistant.
  await page.goto("/#settings/assistant");
  const pick = page.getByRole("combobox", { name: "Model" });
  await expect(pick).toHaveValue("qwen35-2b");
  await expect(pick.locator('option[value="qwen35-9b"]')).toHaveText(/needs more memory$/);
  await expect(pick.locator('option[value="qwen35-9b"]')).toHaveJSProperty("disabled", true);
  await expect(page.getByText(/Recommended for this computer with .*: AI model for phones \(Qwen3\.5 2B\)/)).toBeVisible();
  expect(chosen).toBeNull();

  // And in Add-ons, where models are downloaded.
  await page.goto("/#addons/models");
  await expect(page.locator('[data-entry="qwen35-9b"]')).toContainText("Needs more memory than this computer has.");
  await expect(page.locator('[data-entry="qwen35-4b"]')).not.toContainText("Needs more memory");
  await noHorizontalScroll(page);
});

test("assistant: a computer without the memory for any model says so; the rest still works", async ({ page }) => {
  await ensureSetUp(page);
  const models = EIGHT_GB.models.map((m) => ({ ...m, installed: false, recommended: false, fits: false }));
  await page.route("**/api/assistant", (route) =>
    route.fulfill({ json: { ...EIGHT_GB, engine: "no_model", selected: null, recommended: null, ram_total: 3.2e9, models } }),
  );
  await page.goto("/#assistant");
  await expect(page.getByRole("heading", { name: "The assistant is not available on this computer" })).toBeVisible();
  await expect(page.getByText("This computer does not have enough memory for an AI model. The library, maps, supplies and phones all still work.")).toBeVisible();
  // Nothing to download, and no question box.
  await expect(page.getByRole("button", { name: "Download" })).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "The assistant needs an AI model" })).toHaveCount(0);
  await expect(page.getByRole("textbox", { name: "Ask something" })).toHaveCount(0);
  await page.goto("/#settings/assistant");
  await expect(page.getByText(/The assistant is not available on this computer\. This computer does not have enough memory/)).toBeVisible();
  // The library still opens.
  await page.goto("/#library");
  await expect(page.getByRole("heading", { name: "Library", exact: true })).toBeVisible();
  await noHorizontalScroll(page);
});

test("assistant: an answer that finds too little free memory says so in the user's words", async ({ page }) => {
  await ensureSetUp(page);
  await page.route("**/api/assistant", (route) => route.fulfill({ json: { ...EIGHT_GB, engine: "stopped" } }));
  await page.route("**/api/assistant/ask", (route) => route.fulfill({ json: { id: "m1" } }));
  await page.route("**/api/assistant/answers/m1", (route) =>
    route.fulfill({
      json: {
        id: "m1", question: "How long does rice keep?", status: "failed", grounded: false, language: "en", tokens_per_second: 0, sources: [], text: "",
        error: "not enough free memory for the AI right now; close some programs and try again",
      },
    }),
  );
  await page.goto("/#assistant");
  await page.getByRole("textbox", { name: "Ask something" }).fill("How long does rice keep?");
  await page.getByRole("button", { name: "Ask the assistant" }).click();
  await expect(page.locator(".exchange .error")).toHaveText("Not enough free memory for the AI right now. Close some programs and try again.");
});

test("assistant: an answer shows its sources, which open the article", async ({ page }) => {
  await ensureSetUp(page);
  // A hub with a model and a library, answering from a source (served by the test, not a real model).
  await page.route("**/api/assistant", (route) =>
    route.fulfill({
      json: {
        engine: "ready", engine_installed: true, selected: "qwen35-2b", recommended: "qwen35-2b", ram_total: 8e9, books: 1,
        models: [{ id: "qwen35-2b", title_en: "AI model for phones (Qwen3.5 2B)", title_sr: "x", size: 1e9, installed: true, recommended: true }],
      },
    }),
  );
  await page.route("**/api/assistant/ask", (route) => route.fulfill({ json: { id: "a1" } }));
  await page.route("**/api/assistant/answers/a1", (route) =>
    route.fulfill({
      json: {
        id: "a1", question: "How long do beans keep?", status: "done", grounded: true, language: "en", tokens_per_second: 12.5, error: null,
        text: "Dry beans keep for **years** when stored dry [1].",
        sources: [{ n: 1, title: "Bean", url: "/kiwix/content/test/A/Bean", book_title_en: "Wikipedia", book_title_sr: "Vikipedija" }],
      },
    }),
  );
  await page.goto("/#assistant");
  await page.getByRole("textbox", { name: "Ask something" }).fill("How long do beans keep?");
  await page.getByRole("button", { name: "Ask the assistant" }).click();
  await expect(page.locator(".chat").getByText("Dry beans keep for")).toBeVisible();
  await expect(page.locator(".answer-text strong")).toHaveText("years");
  await expect(page.getByText("Sources", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: /Bean · Wikipedia/ }).click();
  await expect(page.locator(".reader-title")).toHaveText("Bean");
  await page.getByRole("button", { name: /Back/ }).click();
  await expect(page.locator(".chat").getByText("Dry beans keep for")).toBeVisible();
  await noHorizontalScroll(page);
});

test("assistant: a proposed supplies change happens only after confirming", async ({ page }) => {
  await ensureSetUp(page);
  const name = `Test milk ${Date.now()}`;
  await page.route("**/api/assistant", (route) =>
    route.fulfill({
      json: {
        engine: "ready", engine_installed: true, selected: "qwen35-2b", recommended: "qwen35-2b", ram_total: 8e9, books: 0,
        models: [{ id: "qwen35-2b", title_en: "AI model for phones (Qwen3.5 2B)", title_sr: "x", size: 1e9, installed: true, recommended: true }],
      },
    }),
  );
  await page.route("**/api/assistant/ask", (route) => route.fulfill({ json: { id: "p1" } }));
  await page.route("**/api/assistant/answers/p1", (route) =>
    route.fulfill({
      json: {
        id: "p1", question: "Add 2 liters of milk", status: "done", grounded: true, from_supplies: true, language: "en",
        tokens_per_second: 0, error: null, sources: [], searched: [], text: `Add a new item "${name}", 2 l?`,
        proposal: { action: "add", item_id: null, name, quantity: 2, unit: "l", category: "drink", current: null },
      },
    }),
  );
  await page.goto("/#assistant");
  await page.getByRole("textbox", { name: "Ask something" }).fill("Add 2 liters of milk");
  await page.getByRole("button", { name: "Ask the assistant" }).click();
  await expect(page.locator(".chat").getByText(`Add a new item "${name}", 2 l?`)).toBeVisible();
  // Nothing is in the supplies yet.
  const before = await page.request.get("/api/items");
  expect((await before.json()).some((i: { name: string }) => i.name === name)).toBe(false);
  await page.getByRole("button", { name: "Yes, do it" }).click();
  await expect(page.getByText("Done")).toBeVisible();
  const after = await page.request.get("/api/items");
  const item = (await after.json()).find((i: { name: string }) => i.name === name);
  expect(item.quantity).toBe(2);
  expect(item.unit).toBe("l");
});

test("settings: a backup can be made and a restore is prepared for the next start", async ({ page }) => {
  await ensureSetUp(page);
  await page.goto("/#settings/backups");
  await expect(page.getByRole("heading", { name: "Backups", level: 1 })).toBeVisible();
  await page.getByRole("button", { name: "Make a backup now" }).click();
  await expect(page.getByText(/^Saved: .*zaklon-backup-.*\.zip$/)).toBeVisible();
  const row = page.locator(".backup-row").first();
  await expect(row).toContainText("saved by hand");
  await expect(row).toContainText(/· encrypted ·/);
  await row.getByRole("button", { name: "Restore" }).click();
  // What a restore keeps is said, and the backup asks for the password it was made with.
  await expect(row.getByText(/the phones paired now, the household password and this hub's identity stay as they are/)).toBeVisible();
  const yes = row.getByRole("button", { name: "Yes, restore" });
  await expect(yes).toBeDisabled();
  const password = row.getByLabel("Household password from when the backup was made");
  await password.fill("not the password");
  await yes.click();
  await expect(row.getByText(/This password does not open the backup/)).toBeVisible({ timeout: 20_000 });
  await password.fill(PASSWORD);
  await yes.click();
  // The password is checked with Argon2, slow on purpose; a busy machine needs longer.
  await expect(page.getByText(/The backup is checked and ready/)).toBeVisible({ timeout: 20_000 });
  // Wait until that restore has finished (the notice may already be there from an earlier run).
  // The password is checked with Argon2, slow on purpose; a busy machine needs longer.
  await expect(page.getByRole("button", { name: "Make a backup now" })).toBeEnabled({ timeout: 20_000 });
  // A file that is not a backup is refused.
  await page.getByRole("textbox", { name: "Restore from a backup file" }).fill("C:\\Windows\\win.ini");
  const fromFile = page.locator(".restore-file");
  await fromFile.getByRole("button", { name: "Restore", exact: true }).click();
  await fromFile.getByRole("button", { name: "Yes, restore" }).click();
  await expect(page.getByText("This is not a Zaklon backup, or it is damaged.")).toBeVisible();
  await noHorizontalScroll(page);
});

test("updates: Home tells about a newer version; Settings has the switch", async ({ page }) => {
  await ensureSetUp(page);
  await page.route("**/api/updates", (route) =>
    route.fulfill({
      json: { enabled: true, current: "0.1.0", latest: "0.2.0", newer: true, url: "https://github.com/stefan-cirovic/zaklon/releases/tag/v0.2.0", checked_at: "2026-09-28T08:00:00Z", error: null },
    }),
  );
  await page.goto("/#home");
  await expect(page.getByText("A newer Zaklon is available:")).toBeVisible();
  // With the warnings, above the cards.
  await expect(page.locator(".home-alerts .update-banner strong")).toHaveText("0.2.0");
  // Settings › Updates has the switch.
  await page.goto("/#settings/updates/updates");
  await expect(page.getByRole("heading", { name: "Updates", level: 1 })).toBeVisible();
  await expect(page.getByText(/Check once a day whether a newer Zaklon is out/)).toBeVisible();
  await expect(page.getByRole("button", { name: "Check now" })).toBeVisible();
  await noHorizontalScroll(page);
});

/** An item as the hub lists it, for simulated answers. */
function fakeItem(id: string, name: string, more: Record<string, unknown> = {}) {
  return {
    id, name, quantity: 1, unit: "pcs", category: "food", place: null, expiry: null, barcode: null, min_quantity: null, notes: null,
    updated_at: "2026-09-28T08:00:00Z", updated_by: null, ...more,
  };
}

/** A day from today as the app writes dates ("2026-09-30"), in this computer's time zone. */
function dayFromToday(n: number) {
  const d = new Date(Date.now() + n * 86_400_000);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

test("home: the hub at the top, the supplies in three columns, and the tools not in the bar", async ({ page }, info) => {
  await ensureSetUp(page);
  await page.request.post("/api/pinned-tool", { data: { tool: null } });
  // The supplies and the shopping list, simulated so the lists are known. Milk expires soon and runs low too.
  const milk = fakeItem("s1", "Milk", { unit: "l", expiry: dayFromToday(1), min_quantity: 2 });
  await page.route("**/api/supplies/summary", (r) =>
    r.fulfill({
      json: {
        total_items: 42,
        expired: [fakeItem("e1", "Bread", { expiry: dayFromToday(-2) }), fakeItem("e2", "Beans", { quantity: 4, expiry: "2020-01-31" })],
        expiring_soon: [fakeItem("s2", "Yogurt", { expiry: dayFromToday(3) }), milk, fakeItem("s3", "Cheese", { expiry: dayFromToday(0) })],
        running_low: [milk, fakeItem("l1", "Rice", { unit: "kg", min_quantity: 4 }), fakeItem("l2", "Candles", { quantity: 0, min_quantity: 6 })],
        to_put_away: 2,
      },
    }),
  );
  // Rice is on the list already (suggested because it runs low); Sugar was bought and waits to be put away.
  let shop: Record<string, unknown>[] = [
    { id: "low:l1", item_id: "l1", text: "Rice", quantity: 3, unit: "kg", status: "open", source: "running_low" },
    { id: "b1", item_id: null, text: "Matches", quantity: 2, unit: "pack", status: "open", source: "manual" },
    { id: "b2", item_id: null, text: "Salt", quantity: null, unit: null, status: "open", source: "manual" },
    { id: "b3", item_id: null, text: "Sugar", quantity: 1, unit: "kg", status: "bought", source: "manual" },
  ];
  await page.route("**/api/shopping", (r) => r.fulfill({ json: shop }));
  // A tick on Home is "Bought", as on the shopping list.
  const bought: string[] = [];
  await page.route("**/api/shopping/*/bought", (r) => {
    const id = decodeURIComponent(r.request().url().split("/").at(-2) ?? "");
    bought.push(id);
    shop = shop.map((e) => (e.id === id ? { ...e, status: "bought" } : e));
    return r.fulfill({ status: 204 });
  });
  const hub = await (await page.request.get("/api/status")).json();
  await page.goto("/#home");

  // The hub in one line: its name, that it runs, its power, the paired devices and the address.
  const head = page.locator(".home-head");
  await expect(head.getByRole("heading", { level: 1 })).toHaveText(hub.hub_name);
  await expect(head.locator(".home-state")).toHaveText("Running");
  const facts = head.locator(".home-facts");
  await expect(facts.locator("dt")).toHaveText(["Power", "Devices", "Addresses"]);
  // The power comes with the add-ons' answer: a battery, or "On power".
  await expect(facts.locator("dd").first()).toHaveText(/^(On power|\d+% · (charging|on battery))$/);
  await expect(facts.locator("dd").nth(1)).toHaveText(/^\d+/);
  await expect(facts.getByRole("button", { name: "Add a phone" })).toBeVisible();
  await expect(head.getByRole("link", { name: "How it works" })).toBeVisible();

  // The supplies in three columns, the most urgent first in each.
  const supplies = page.getByRole("region", { name: "Supplies" });
  const cols = supplies.locator(".sup-col");
  await expect(cols.getByRole("heading", { level: 3 })).toHaveText(["Expiring", "Running low", "To buy"]);
  // Expiring: what expired (the longest ago first), then what expires soon (the soonest first). The state is
  // short, since the heading says the rest; expired in red, the rest in gray.
  const expiring = supplies.getByRole("region", { name: "Expiring" });
  await expect(expiring.locator(".sup-name")).toHaveText(["Beans", "Bread", "Cheese", "Milk", "Yogurt"]);
  await expect(expiring.locator(".sup-state")).toHaveText(["expired 31 Jan 2020", "expired 2 days ago", "today", "tomorrow", "in 3 days"]);
  const color = (el: Locator) => el.evaluate((e) => getComputedStyle(e).color);
  expect(await color(expiring.locator(".sup-state").nth(0))).toBe("rgb(238, 123, 110)");
  expect(await color(expiring.locator(".sup-state").nth(2))).toBe("rgb(157, 157, 157)");
  // Five lines: all of them, so no link to more.
  await expect(expiring.getByRole("link")).toHaveCount(0);
  // Running low: the emptiest first; what is out entirely in red; a cart where it is on the shopping list.
  const low = supplies.getByRole("region", { name: "Running low" });
  await expect(low.locator(".sup-name")).toHaveText(["Candles", "Rice", "Milk"]);
  await expect(low.locator(".sup-state")).toHaveText(["0 of 6 pcs", "1 of 4 kg", "1 of 2 liters"]);
  expect(await color(low.locator(".sup-state").nth(0))).toBe("rgb(238, 123, 110)");
  await expect(low.locator(".sup-row").nth(1).locator(".sup-listed")).toHaveAttribute("title", "On the list");
  await expect(low.locator(".sup-listed")).toHaveCount(1);
  // To buy: the shopping list itself (not what was bought), a box to tick on each line.
  const buy = supplies.getByRole("region", { name: "To buy" });
  await expect(buy.getByRole("checkbox")).toHaveText(["Rice3 kg", "Matches2 packs", "Salt"]);
  await expect(buy.getByRole("checkbox", { name: "Bought: Matches" })).not.toBeChecked();
  await expect(buy.getByRole("link", { name: "To put away: 2" })).toHaveAttribute("href", "#supplies/putaway");
  // The name and its state together: right after it on the line, or right below it when there is no room.
  for (const row of await supplies.locator(".sup-row", { has: page.locator(".sup-state") }).all()) {
    const [name, state] = [(await row.locator(".sup-name").boundingBox())!, (await row.locator(".sup-state").boundingBox())!];
    if (state.y < name.y + name.height - 4) expect(state.x - (name.x + name.width), "the state right after the name").toBeLessThanOrEqual(12);
    else expect(Math.abs(state.x - name.x), "the state right below the name").toBeLessThan(2);
  }
  // Below them: adding an item (no scanner in a browser: Scan is the phone app's), and how many items there are.
  const foot = supplies.locator(".sup-foot");
  await expect(foot.getByRole("link", { name: "Add item" })).toHaveAttribute("href", "#supplies/add");
  await expect(foot.getByRole("link", { name: "Scan" })).toHaveCount(0);
  await expect(foot).toContainText("42 items at home");

  // A tick marks it bought, as "Bought" on the shopping list: it leaves the list.
  await buy.getByRole("checkbox", { name: "Bought: Matches" }).click();
  await expect(buy.getByRole("checkbox")).toHaveText(["Rice3 kg", "Salt"]);
  expect(bought).toEqual(["b1"]);

  // The tools not in the bar, and Help.
  const tools = page.getByRole("region", { name: "Quick access" });
  await expect(tools.getByRole("link")).toHaveText(["Supplies", "Library", "Maps", "Power calculator", "Water calculator", "Add-ons", "Help"]);
  await noHorizontalScroll(page);

  const box = async (name: string) => (await page.getByRole("region", { name, exact: true }).boundingBox())!;
  if (info.project.name === "laptop") {
    // A tall window: the assistant across the top; the supplies and the home on the map side
    // by side, half the width each, sharing the height that is left; the library and the tools
    // below them, just above the bar.
    await page.setViewportSize({ width: 1400, height: 1200 });
    const [ask, sup, map, lib, quick] = [await box("Assistant"), await box("Supplies"), await box("Home on the map"), await box("Library and add-ons"), await box("Quick access")];
    expect(ask.width).toBeGreaterThan(1250);
    expect(Math.abs(sup.y - map.y)).toBeLessThan(2);
    expect(Math.abs(sup.height - map.height)).toBeLessThan(2);
    expect(map.x).toBeGreaterThan(sup.x + sup.width);
    expect(Math.abs(sup.width - map.width), "half the width each").toBeLessThan(2);
    expect(lib.y).toBeGreaterThanOrEqual(sup.y + sup.height);
    expect(Math.abs(lib.y - quick.y)).toBeLessThan(2);
    expect(quick.x).toBeGreaterThan(lib.x + lib.width);
    // Below them only the version line (about 100 px with its spacing).
    const nav = (await page.locator("nav.nav").boundingBox())!;
    expect(nav.y - Math.max(lib.y + lib.height, quick.y + quick.height), "the cards reach down to the bar").toBeLessThan(120);
    // The supplies' three columns side by side, the same width each, the footer at the bottom of the card.
    const [c1, c2, c3] = [(await expiring.boundingBox())!, (await low.boundingBox())!, (await buy.boundingBox())!];
    expect(Math.abs(c1.y - c2.y) + Math.abs(c2.y - c3.y)).toBeLessThan(2);
    expect(c2.x).toBeGreaterThan(c1.x + c1.width);
    expect(c3.x).toBeGreaterThan(c2.x + c2.width);
    expect(Math.abs(c1.width - c3.width)).toBeLessThan(2);
    const f = (await foot.boundingBox())!;
    expect(sup.y + sup.height - (f.y + f.height)).toBeLessThan(24);
    await noHorizontalScroll(page);
  } else {
    // A phone: one card under the other, and everything to tap a fingertip wide.
    const [sup, lib] = [await box("Supplies"), await box("Library and add-ons")];
    expect(lib.y).toBeGreaterThanOrEqual(sup.y + sup.height);
    // The supplies' columns one under the other, each the card's width.
    const [c1, c2, c3] = [(await expiring.boundingBox())!, (await low.boundingBox())!, (await buy.boundingBox())!];
    expect(c2.y).toBeGreaterThanOrEqual(c1.y + c1.height);
    expect(c3.y).toBeGreaterThanOrEqual(c2.y + c2.height);
    expect(Math.abs(c1.x - c3.x)).toBeLessThan(2);
    expect(c1.width).toBeGreaterThan(sup.width - 40);
    const small = await page.evaluate(() =>
      [...document.querySelectorAll(".home a, .home button, .home input")]
        .map((el) => ({ el, r: el.getBoundingClientRect() }))
        .filter(({ r }) => r.width > 0 && (r.height < 44 || r.width < 44))
        .map(({ el }) => el.textContent || el.getAttribute("aria-label")),
    );
    expect(small, "controls smaller than 44 px").toEqual([]);
  }

  // A link opens its view of Supplies, which stays in the address.
  await buy.getByRole("link", { name: "To put away: 2" }).click();
  await expect(page).toHaveURL(/#supplies\/putaway$/);
  await expect(page.getByRole("button", { name: /^Put away/ })).toHaveAttribute("aria-pressed", "true");
  await page.reload();
  await expect(page.getByRole("button", { name: /^Put away/ })).toHaveAttribute("aria-pressed", "true");
  await page.getByRole("button", { name: "History" }).click();
  await expect(page).toHaveURL(/#supplies\/history$/);

  // A pinned tool is in the bar, so not among the tiles.
  await page.request.post("/api/pinned-tool", { data: { tool: "maps" } });
  await page.goto("/#home");
  await page.reload();
  await expect(tools.getByRole("link")).toHaveText(["Supplies", "Library", "Power calculator", "Water calculator", "Add-ons", "Help"]);
  await page.request.post("/api/pinned-tool", { data: { tool: null } });
});

test("home: an empty column of the supplies says Nothing, calmly, so all is well there", async ({ page }) => {
  await ensureSetUp(page);
  await page.route("**/api/supplies/summary", (r) => r.fulfill({ json: { total_items: 3, expired: [], expiring_soon: [], running_low: [], to_put_away: 0 } }));
  await page.route("**/api/shopping", (r) => r.fulfill({ json: [] }));
  await page.goto("/#home");
  const supplies = page.getByRole("region", { name: "Supplies" });
  for (const name of ["Expiring", "Running low", "To buy"]) {
    const col = supplies.getByRole("region", { name });
    await expect(col.locator(".sup-nothing")).toHaveText("Nothing");
    await expect(col.getByRole("listitem")).toHaveCount(0);
    await expect(col.getByRole("link")).toHaveCount(0);
  }
  // Gray words and a green check: nothing to worry about.
  const nothing = supplies.locator(".sup-nothing").first();
  expect(await nothing.evaluate((el) => getComputedStyle(el).color)).toBe("rgb(157, 157, 157)");
  expect(await nothing.locator("svg").evaluate((el) => getComputedStyle(el).color)).toBe("rgb(127, 203, 147)");
  await expect(supplies.locator(".sup-foot")).toContainText("3 items at home");
  await expect(supplies.getByRole("link", { name: "Add item" })).toBeVisible();
  await noHorizontalScroll(page);
});

test("home: something expired goes on the shopping list with the cart beside it", async ({ page }, info) => {
  await ensureSetUp(page);
  // Bread expired and is not on the list; Beans expired and are on it already; Yogurt only expires soon.
  await page.route("**/api/supplies/summary", (r) =>
    r.fulfill({
      json: {
        total_items: 5,
        expired: [fakeItem("e1", "Bread", { quantity: 2, expiry: dayFromToday(-2) }), fakeItem("e2", "Beans", { expiry: dayFromToday(-5) })],
        expiring_soon: [fakeItem("s1", "Yogurt", { expiry: dayFromToday(3) })],
        running_low: [],
        to_put_away: 0,
      },
    }),
  );
  let shop: Record<string, unknown>[] = [{ id: "b1", item_id: "e2", text: "Beans", quantity: 1, unit: "pcs", status: "open", source: "manual" }];
  const posted: Record<string, unknown>[] = [];
  await page.route("**/api/shopping", async (r) => {
    if (r.request().method() === "POST") {
      const body = r.request().postDataJSON() as Record<string, unknown>;
      posted.push(body);
      shop = [...shop, { id: `n${posted.length}`, ...body, status: "open", source: "manual" }];
      return r.fulfill({ status: 201, json: shop.at(-1) });
    }
    return r.fulfill({ json: shop });
  });
  await page.goto("/#home");
  const expiring = page.getByRole("region", { name: "Supplies" }).getByRole("region", { name: "Expiring" });
  const bread = expiring.locator(".sup-row", { hasText: "Bread" });
  const beans = expiring.locator(".sup-row", { hasText: "Beans" });
  const yogurt = expiring.locator(".sup-row", { hasText: "Yogurt" });
  // A cart to press where it is not on the list; the mark where it is; nothing for what only expires soon.
  const add = bread.getByRole("button", { name: "Add to the shopping list: Bread" });
  await expect(add).toBeVisible();
  await expect(beans.locator(".sup-listed")).toHaveAttribute("title", "On the list");
  await expect(beans.getByRole("button")).toHaveCount(0);
  await expect(yogurt.getByRole("button")).toHaveCount(0);
  await expect(yogurt.locator(".sup-listed")).toHaveCount(0);
  if (info.project.name === "phone") {
    const box = (await add.boundingBox())!;
    expect(box.width).toBeGreaterThanOrEqual(44);
    expect(box.height).toBeGreaterThanOrEqual(44);
  }
  // One press: on the list as much as there was, linked to the item; the cart turns into the mark.
  await add.click();
  await expect.poll(() => posted.length).toBe(1);
  expect(posted[0]).toMatchObject({ text: "Bread", quantity: 2, item_id: "e1" });
  await expect(bread.locator(".sup-listed")).toBeVisible();
  await expect(bread.getByRole("button")).toHaveCount(0);
  await expect(page.getByRole("region", { name: "To buy" }).getByRole("checkbox", { name: "Bought: Bread" })).toBeVisible();
  await noHorizontalScroll(page);
});

test("home: Add item on the supplies card opens a new item in Supplies, once", async ({ page }) => {
  await ensureSetUp(page);
  await page.goto("/#home");
  await page.getByRole("region", { name: "Supplies" }).getByRole("link", { name: "Add item" }).click();
  await expect(page.getByRole("heading", { name: "Add item", level: 1 })).toBeVisible();
  await expect(page.getByLabel("Name")).toBeFocused();
  // The address is the supplies again, so a reload or the way back does not open it again.
  await expect(page).toHaveURL(/#supplies$/);
  await page.getByRole("button", { name: "Cancel" }).click();
  await expect(page.getByRole("heading", { name: "Supplies", level: 1 })).toBeVisible();
  await page.reload();
  await expect(page.getByRole("heading", { name: "Supplies", level: 1 })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Add item" })).toHaveCount(0);
  await page.goBack();
  await expect(page).toHaveURL(/#home$/);
});

test("home: a long column shows the five most urgent and leads to the rest in Supplies", async ({ page }) => {
  await ensureSetUp(page);
  // Out of order on purpose: Home sorts them.
  const soon = [7, 3, 1, 5, 2, 6, 4].map((n) => fakeItem(`s${n}`, `Yogurt ${n}`, { expiry: dayFromToday(n) }));
  const low = [6, 2, 4, 1, 3, 5].map((n) => fakeItem(`l${n}`, `Rice ${n}`, { unit: "kg", quantity: n, min_quantity: 10 }));
  await page.route("**/api/supplies/summary", (r) =>
    r.fulfill({ json: { total_items: 20, expired: [], expiring_soon: soon, running_low: low, to_put_away: 0 } }),
  );
  const shop = [1, 2, 3, 4, 5, 6, 7, 8].map((n) => ({ id: `b${n}`, item_id: null, text: `Candles ${n}`, quantity: null, unit: null, status: "open", source: "manual" }));
  await page.route("**/api/shopping", (r) => r.fulfill({ json: shop }));
  await page.goto("/#home");
  const expiring = page.getByRole("region", { name: "Expiring" });
  await expect(expiring.locator(".sup-name")).toHaveText(["Yogurt 1", "Yogurt 2", "Yogurt 3", "Yogurt 4", "Yogurt 5"]);
  await expect(expiring.getByRole("link")).toHaveText(["+ 2 more"]);
  await expect(expiring.getByRole("link", { name: "+ 2 more" })).toHaveAttribute("href", "#supplies/expiring");
  const lowCol = page.getByRole("region", { name: "Running low" });
  await expect(lowCol.locator(".sup-name")).toHaveText(["Rice 1", "Rice 2", "Rice 3", "Rice 4", "Rice 5"]);
  await expect(lowCol.getByRole("link", { name: "+ 1 more" })).toHaveAttribute("href", "#supplies/low");
  const buy = page.getByRole("region", { name: "To buy" });
  await expect(buy.getByRole("checkbox")).toHaveCount(5);
  await expect(buy.getByRole("link", { name: "+ 3 more" })).toHaveAttribute("href", "#supplies/shopping");
  // The rest of what expires: Supplies with that filter.
  await expiring.getByRole("link", { name: "+ 2 more" }).click();
  await expect(page.getByRole("heading", { name: "Supplies", level: 1 })).toBeVisible();
  await expect(page.getByRole("button", { name: "Expiring", exact: true })).toHaveAttribute("aria-pressed", "true");
  await expect(page).toHaveURL(/#supplies$/);
  // The rest of the shopping list.
  await page.goto("/#home");
  await buy.getByRole("link", { name: "+ 3 more" }).click();
  await expect(page.getByRole("button", { name: "Shopping list" })).toHaveAttribute("aria-pressed", "true");
  await expect(page).toHaveURL(/#supplies\/shopping$/);
});

test("home: Add a phone beside Devices opens the pairing straight away (laptop)", async ({ page }, info) => {
  await ensureSetUp(page);
  await page.goto("/#home");
  const add = page.locator(".home-facts").getByRole("button", { name: "Add a phone" });
  await expect(add).toBeVisible();
  if (info.project.name === "phone") {
    // A narrow window: the "+" alone, a fingertip wide.
    const box = (await add.boundingBox())!;
    expect(box.width).toBeGreaterThanOrEqual(44);
    expect(box.height).toBeGreaterThanOrEqual(44);
  } else {
    await expect(add).toContainText("Add a phone");
  }
  await add.click();
  await expect(page).toHaveURL(/#settings\/devices\/add-phone$/);
  // The same codes as Settings › Devices › Add a phone.
  await expect(page.getByRole("heading", { name: "Pair a phone" })).toBeVisible();
  await expect(page.getByRole("img", { name: /QR code/ })).toHaveCount(2);
  await expect(page.locator(".code")).toHaveText(/^\d{6}$/);
  await expect(page.locator("#set-add-phone")).toBeInViewport();
  await page.getByRole("button", { name: "Cancel" }).click();
  await expect(page.getByRole("button", { name: "Add a phone" })).toBeVisible();
  // Only when asked: Devices opened another way shows no code.
  await page.goto("/#home");
  await page.goto("/#settings/devices");
  await expect(page.getByRole("heading", { name: "Paired devices" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Pair a phone" })).toHaveCount(0);
  await noHorizontalScroll(page);
});

test("home: a question asked there starts a new conversation; the last three open from there", async ({ page }, info) => {
  await ensureSetUp(page);
  // The test hub has no AI: the assistant says it is ready, and every answer then fails at once,
  // which is enough to save the conversation.
  await page.route("**/api/assistant", (route) =>
    route.fulfill({
      json: {
        engine: "ready", engine_installed: true, selected: "qwen35-2b", recommended: "qwen35-2b", ram_total: 8e9, books: 1,
        models: [{ id: "qwen35-2b", title_en: "AI model for phones (Qwen3.5 2B)", title_sr: "x", size: 1e9, installed: true, recommended: true }],
      },
    }),
  );
  const stamp = `${info.project.name} ${Date.now() % 1_000_000}`;
  for (const title of [`Seeds ${stamp}`, `Batteries ${stamp}`]) await page.request.post("/api/conversations", { data: { title } });
  const seeds = (await (await page.request.get("/api/conversations")).json()).find((c: { title: string }) => c.title === `Seeds ${stamp}`);
  const asked: Record<string, unknown>[] = [];
  page.on("request", (r) => {
    if (r.url().endsWith("/api/assistant/ask")) asked.push(r.postDataJSON());
  });

  await page.goto("/#home");
  // A conversation was left open in the Assistant; a question from Home still starts a new one.
  await page.evaluate((id) => localStorage.setItem("zaklon.chat.open", id), seeds.id);
  const card = page.getByRole("region", { name: "Assistant" });
  const box = card.getByRole("textbox", { name: "Ask anything" });
  await expect(card.getByRole("button", { name: "Ask the assistant" })).toBeDisabled();
  const question = `How do I store water for ${stamp}?`;
  await box.fill(question);
  await box.press("Enter");
  await expect(page).toHaveURL(/#assistant$/);
  await expect(page.locator(".exchange .q")).toHaveText([question]);
  await expect(page.locator(".exchange .error")).toBeVisible();
  expect(asked).toHaveLength(1);
  expect(asked[0]).toMatchObject({ question, new_conversation: true });
  expect(asked[0].conversation).toBeUndefined();
  await expect(page.getByRole("heading", { name: question })).toBeVisible();

  // Back on Home: the three used last, the newest first.
  await page.locator("nav.nav").getByRole("button", { name: "Home" }).click();
  const recent = card.getByRole("region", { name: "Recent conversations" }).getByRole("button");
  await expect(recent).toHaveCount(3);
  await expect(recent.nth(0)).toContainText(question);
  await expect(recent.nth(1)).toContainText(`Batteries ${stamp}`);
  await expect(recent.nth(2)).toContainText(`Seeds ${stamp}`);
  await noHorizontalScroll(page);
  // One of them opens in the Assistant.
  await recent.nth(1).click();
  await expect(page).toHaveURL(/#assistant$/);
  await expect(page.getByRole("heading", { name: `Batteries ${stamp}` })).toBeVisible();
  await expect(page.getByRole("textbox", { name: "Ask something" })).toHaveValue("");

  for (const c of (await (await page.request.get("/api/conversations")).json()) as { id: string; title: string }[]) {
    if (c.title.includes(stamp)) await page.request.delete(`/api/conversations/${c.id}`);
  }
});

test("home: what downloads, new versions and the library's drive (states simulated)", async ({ page }) => {
  await ensureSetUp(page);
  const GB = 1024 ** 3;
  await page.route("**/api/catalog", async (r) => {
    const json = await (await r.fetch()).json();
    json.system = { ...json.system, disk_free: 120 * GB, disk_total: 500 * GB };
    json.library_drive = "D:\\";
    for (const p of json.packs) {
      if (p.id === "wikimed-en") p.state = { ...p.state, status: "downloading", bytes_done: Math.round(p.size * 0.45), bytes_total: p.size, speed: 3_000_000 };
      if (p.id === "kiwix-tools") p.state = { ...p.state, status: "installed", bytes_done: p.size, bytes_total: p.size, update_available: true };
      if (p.id === "qwen35-08b") p.state = { ...p.state, status: "paused", bytes_done: Math.round(p.size * 0.2), bytes_total: p.size };
    }
    return r.fulfill({ json });
  });
  await page.route("**/api/maps", async (r) => {
    const json = await (await r.fetch()).json();
    const serbia = json.countries.find((c: { id: string }) => c.id === "Serbia");
    serbia.regions[0].status = "downloading";
    serbia.regions[0].bytes_done = Math.round(serbia.regions[0].size / 2);
    return r.fulfill({ json });
  });
  await page.goto("/#home");
  const card = page.getByRole("region", { name: "Library and add-ons" });
  // Both downloads, with how far each is.
  const downloads = card.getByRole("region", { name: /^Downloads/ });
  await expect(downloads.getByRole("heading")).toHaveText("Downloads 2");
  await expect(downloads.getByRole("progressbar", { name: /Medical Wikipedia/ })).toHaveAttribute("aria-valuenow", "45");
  await expect(downloads.getByRole("progressbar", { name: "Maps: Serbia" })).toBeVisible();
  await expect(downloads).toContainText(/3\sMB\/s/);
  // What waits, and the library's drive.
  await expect(card.getByRole("link", { name: "New version available: 1" })).toHaveAttribute("href", "#addons/library");
  await expect(card.getByRole("link", { name: "Paused: 1" })).toBeVisible();
  await expect(card).toContainText(/120\sGB free of 500\sGB/);
  await expect(card).toContainText("Zaklon library");
  await noHorizontalScroll(page);
  await card.getByRole("button", { name: /The hub's disk \(D:\)/ }).click();
  await expect(page).toHaveURL(/#addons\/library$/);
  await page.goBack();
  await card.getByRole("link", { name: "Open Add-ons" }).click();
  await expect(page).toHaveURL(/#addons$/);
  await expect(page.getByRole("heading", { name: "Add-ons", exact: true })).toBeVisible();
  // A screen showing downloads asks again every few seconds: stop the made-up answers before the page closes.
  await page.unrouteAll({ behavior: "ignoreErrors" });
});

test("home: warnings come first: the firewall, and a hub that stopped answering (simulated)", async ({ page }) => {
  await ensureSetUp(page);
  await page.route("**/api/firewall", (r) =>
    r.fulfill({ json: { checked: true, firewall_on: true, allowed: false, blocked: true, public_network: false, error: null, ok: false } }),
  );
  await page.goto("/#home");
  const firewall = page.locator(".home-alerts .firewall");
  await expect(firewall).toContainText(/Windows Firewall is blocking Zaklon/);
  await expect(firewall.getByRole("button", { name: "Let phones connect" })).toBeVisible();
  const [warning, cards] = [(await firewall.boundingBox())!, (await page.locator(".home-grid").boundingBox())!];
  expect(warning.y + warning.height).toBeLessThanOrEqual(cards.y);
  // The hub stops answering: Home says so at the top, and the facts are marked as old.
  await page.route("**/api/status", (r) => r.abort());
  await expect(page.getByRole("alert").filter({ hasText: "Zaklon's hub is not running on this computer." })).toBeVisible({ timeout: 15_000 });
  await expect(page.locator(".home-state")).toHaveText("Not reachable");
  await expect(page.locator(".home-asof")).toHaveText(/^as of /);
  await expect(page.locator(".home-facts")).toHaveClass(/stale/);
});

test("assistant memory: notes can be added and deleted by hand", async ({ page }, info) => {
  await ensureSetUp(page);
  await page.route("**/api/assistant", (route) =>
    route.fulfill({
      json: {
        engine: "ready", engine_installed: true, selected: "qwen35-2b", recommended: "qwen35-2b", ram_total: 8e9, books: 1,
        models: [{ id: "qwen35-2b", title_en: "AI model for phones (Qwen3.5 2B)", title_sr: "x", size: 1e9, installed: true, recommended: true }],
      },
    }),
  );
  const text = `The water tank holds 200 liters (${Date.now()})`;
  await page.goto("/#assistant");
  // On a phone it is at the bottom of the conversations panel.
  if (info.project.name === "phone") await page.getByRole("button", { name: "Conversations" }).click();
  await page.getByRole("button", { name: /What the assistant remembers/ }).click();
  const panel = page.getByRole("dialog", { name: "What the assistant remembers" });
  await expect(panel).toBeVisible();
  await page.getByRole("textbox", { name: "Add" }).fill(text);
  await page.getByRole("button", { name: "Add", exact: true }).click();
  const row = page.locator(".memory-row").filter({ hasText: text });
  await expect(row).toBeVisible();
  await row.getByRole("button", { name: "Delete" }).click();
  await row.getByRole("button", { name: /Yes, delete/ }).click();
  await expect(row).toHaveCount(0);
  // Escape closes the panel.
  await page.keyboard.press("Escape");
  await expect(panel).toHaveCount(0);
});

test("assistant: online research is off until switched on, and only for this conversation", async ({ page }) => {
  await ensureSetUp(page);
  await page.route("**/api/assistant", (route) =>
    route.fulfill({
      json: {
        engine: "ready", engine_installed: true, selected: "qwen35-2b", recommended: "qwen35-2b", ram_total: 8e9, books: 1,
        models: [{ id: "qwen35-2b", title_en: "AI model for phones (Qwen3.5 2B)", title_sr: "x", size: 1e9, installed: true, recommended: true }],
      },
    }),
  );
  const bodies: { online: boolean }[] = [];
  await page.route("**/api/assistant/ask", (route) => {
    bodies.push(route.request().postDataJSON());
    return route.fulfill({ json: { id: `w${bodies.length}` } });
  });
  await page.route("**/api/assistant/answers/*", (route) =>
    route.fulfill({
      json: {
        id: route.request().url().split("/").pop(), question: "q", status: "done", grounded: true, language: "en", tokens_per_second: 5, error: null,
        text: "Boil it for a minute [1].", sources: [{ n: 1, title: "Boiling water", web: true, url: "https://example.org/boil", book_title_en: "example.org", book_title_sr: "example.org" }],
      },
    }),
  );
  await page.goto("/#assistant");
  const box = page.getByRole("textbox", { name: "Ask something" });
  await box.fill("How do I make water safe?");
  await page.getByRole("button", { name: "Ask the assistant" }).click();
  await expect(page.locator(".chat").getByText("Boil it for a minute")).toBeVisible();
  expect(bodies[0].online).toBe(false);
  await page.getByLabel(/Also search the internet/).check();
  await box.fill("And without a pot?");
  await page.getByRole("button", { name: "Ask the assistant" }).click();
  await expect.poll(() => bodies.length).toBe(2);
  expect(bodies[1].online).toBe(true);
  await expect(page.getByText("example.org (internet)").first()).toBeVisible();
  await page.getByRole("button", { name: "New conversation" }).click();
  await expect(page.getByLabel(/Also search the internet/)).not.toBeChecked();
});

test("settings: the laptop can offer its own Wi-Fi network (simulated, never really started)", async ({ page }) => {
  await ensureSetUp(page);
  const off = { supported: true, on: false, ssid: "PC 1234", passphrase: "", clients: 0, error: null, qr: null };
  const on = { supported: true, on: true, ssid: "Zaklon", passphrase: "abcd2345ef", clients: 1, error: null, qr: "WIFI:T:WPA;S:Zaklon;P:abcd2345ef;;" };
  await page.route("**/api/hotspot", (r) => r.fulfill({ json: off }));
  await page.route("**/api/hotspot/start", (r) => r.fulfill({ json: on }));
  await page.route("**/api/hotspot/stop", (r) => r.fulfill({ json: off }));
  // Reload so the panel asks again (and gets the simulated answer).
  await page.goto("/#settings/network");
  await page.reload();
  await expect(page.getByRole("heading", { name: "Network", level: 1 })).toBeVisible();
  await page.getByRole("button", { name: "Make the Wi-Fi network" }).click();
  await expect(page.getByText("abcd2345ef")).toBeVisible();
  await expect(page.getByRole("img", { name: "QR code to join the Wi-Fi network" })).toBeVisible();
  await page.getByRole("button", { name: "Turn the network off" }).click();
  await expect(page.getByRole("button", { name: "Make the Wi-Fi network" })).toBeVisible();
  await noHorizontalScroll(page);
});

test("add-ons: the starter set downloads the recommended packs in one go (requests intercepted)", async ({ page }) => {
  await ensureSetUp(page);
  // Whatever this computer's disk holds, the set fits.
  await page.route("**/api/catalog", async (r) => {
    const json = await (await r.fetch()).json();
    json.system.disk_free = 1e12;
    return r.fulfill({ json });
  });
  const asked: string[] = [];
  await page.route("**/api/packs/*/download", (r) => {
    asked.push(new URL(r.request().url()).pathname);
    return r.fulfill({ status: 202, body: "" });
  });
  await page.route("**/api/maps/*/download", (r) => {
    asked.push(new URL(r.request().url()).pathname);
    return r.fulfill({ status: 202, body: "" });
  });
  await page.goto("/#addons");
  const panel = page.locator(".starter");
  await expect(panel.getByRole("heading", { name: "English essentials" })).toBeVisible();
  await expect(panel.getByText(/Still to download/)).toBeVisible();
  // The set is complete, and can be downloaded, once the hub has said which AI model fits this computer.
  await expect(panel.getByRole("listitem").filter({ hasText: /AI model/ })).toHaveCount(1);
  await expect(panel.getByRole("button", { name: "Download all" })).toBeEnabled();
  const lines = await panel.getByRole("listitem").count();
  await panel.getByRole("button", { name: "Download all" }).click();
  // Every line asked for, one after the other (the AI model last).
  await expect.poll(() => asked.length).toBe(lines);
  expect(lines).toBeGreaterThanOrEqual(3);
  expect(asked).toContain("/api/packs/wikimed-en/download");
  expect(asked.some((p) => /\/api\/packs\/qwen35-/.test(p))).toBe(true);
  // The screen asks again after starting: stop the made-up answers before the page closes.
  await page.unrouteAll({ behavior: "ignoreErrors" });
});

test("settings: warns when Windows Firewall would keep phones out, and fixes it (simulated)", async ({ page }) => {
  await ensureSetUp(page);
  const bad = { checked: true, firewall_on: true, allowed: false, blocked: true, error: null, ok: false };
  const good = { ...bad, allowed: true, blocked: false, ok: true };
  let fixed = false;
  await page.route("**/api/firewall", (r) => r.fulfill({ json: fixed ? good : bad }));
  await page.route("**/api/firewall/allow", (r) => {
    fixed = true;
    return r.fulfill({ json: good });
  });
  // On Devices, where phones are added and it cannot be missed, and under Network.
  await page.goto("/#settings/devices");
  await page.reload();
  await expect(page.locator(".firewall").getByText(/Windows Firewall is blocking Zaklon/)).toBeVisible();
  await page.goto("/#settings/network");
  const panel = page.locator(".firewall");
  await expect(panel.getByText(/Windows Firewall is blocking Zaklon/)).toBeVisible();
  await expect(panel.getByRole("button", { name: "Make this network private" })).toHaveCount(0);
  await panel.getByRole("button", { name: "Let phones connect" }).click();
  await expect(panel).toHaveCount(0);
  await expect(page.locator(".firewall-ok")).toContainText("Phones on the Wi-Fi can reach Zaklon");
  await page.goto("/#settings/devices");
  await expect(page.getByRole("heading", { name: "Devices", level: 1 })).toBeVisible();
  await expect(page.locator(".firewall")).toHaveCount(0);
});

test("settings: a home network Windows treats as public is made private (simulated, Windows is never asked)", async ({ page }) => {
  await ensureSetUp(page);
  const pub = { checked: true, firewall_on: true, allowed: true, blocked: false, public_network: true, error: null, ok: false };
  const priv = { ...pub, public_network: false, ok: true };
  // The first time the person says no to Windows' question; then yes.
  const answers = [pub, priv];
  let asked = 0;
  await page.route("**/api/firewall", (r) => r.fulfill({ json: asked >= 2 ? priv : pub }));
  await page.route("**/api/firewall/private", (r) => r.fulfill({ json: answers[Math.min(asked++, 1)] }));
  await page.route("**/api/firewall/allow", (r) => r.abort());
  // On Home, with the warnings.
  await page.goto("/#home");
  await page.reload();
  const panel = page.locator(".firewall");
  await expect(panel.getByText(/treats the network this laptop is on as a public network/)).toBeVisible();
  // Before the button: that this is only for a home network.
  const why = panel.getByText("Do this only for your home network: other devices on a private network can find this laptop.");
  await expect(why).toBeVisible();
  // The rules are there already; asking Windows for them again would not help.
  await expect(panel.getByRole("button", { name: "Let phones connect" })).toHaveCount(0);
  const button = panel.getByRole("button", { name: "Make this network private" });
  const order = await panel.evaluate((el) => [...el.querySelectorAll("p, button")].map((x) => x.textContent));
  expect(order.findIndex((x) => x?.startsWith("Do this only"))).toBeLessThan(order.indexOf("Make this network private"));
  // Windows' question answered with no: the network is still public, and the way in Windows' own settings is shown.
  await button.click();
  await expect(panel.getByText(/Windows still treats this network as public/)).toBeVisible();
  await expect(panel).toContainText("Network & internet › this network › Network profile type › Private");
  // Answered with yes: the warning goes away.
  await button.click();
  await expect(panel).toHaveCount(0);
  expect(asked).toBe(2);
  // Settings › Network then says all is well.
  await page.goto("/#settings/network");
  await expect(page.locator(".firewall-ok")).toContainText("Phones on the Wi-Fi can reach Zaklon");
  await noHorizontalScroll(page);
});

test("settings: a laptop opens on Devices with the categories beside it; a phone lists them first", async ({ page }, info) => {
  await ensureSetUp(page);
  await page.goto("/#home");
  await page.locator("nav.nav").getByRole("button", { name: "Settings" }).click();
  await expect(page).toHaveURL(/#settings$/);
  const categories = page.getByRole("navigation", { name: "Categories" });
  const links = categories.getByRole("link");
  // Every category, and Help at the end.
  await expect(links).toHaveText([/^Devices/, /^Network/, /^Backups/, /^Privacy & security/, /^Appearance/, /^Language/, /^AI assistant/, /^Updates/, /^About/, /^Help/]);
  await expect(links.last()).toHaveAttribute("href", "#help");
  const search = page.getByRole("searchbox", { name: "Find a setting" });
  await expect(search).toBeVisible();
  // No tiles and no card about the hub: Home has that.
  await expect(page.locator(".set-tiles .set-tile-desc").first()).toBeVisible({ visible: info.project.name === "phone" });
  if (info.project.name === "laptop") {
    // Straight into the first category, the list on the left with the search at its top.
    const title = page.getByRole("heading", { name: "Devices", level: 1 });
    await expect(title).toBeVisible();
    await expect(title).toBeFocused();
    await expect(page.getByRole("heading", { level: 1 })).toHaveCount(1);
    await expect(categories.getByRole("link", { name: "Devices" })).toHaveAttribute("aria-current", "page");
    await expect(page.getByRole("button", { name: "Add a phone" })).toBeVisible();
    const [box, list, main] = [(await search.boundingBox())!, (await categories.boundingBox())!, (await page.locator(".set-main").boundingBox())!];
    expect(box.y + box.height).toBeLessThanOrEqual(list.y);
    expect(list.x + list.width).toBeLessThan(main.x);
    // The list is the way around: no way back and no breadcrumb.
    await expect(page.getByRole("link", { name: "Back to Settings" })).toBeHidden();
    await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toBeHidden();
    // A category opens beside the list, marked in it.
    await categories.getByRole("link", { name: "Backups" }).click();
    await expect(page).toHaveURL(/#settings\/backups$/);
    await expect(page.getByRole("heading", { name: "Backups", level: 1 })).toBeFocused();
    await expect(categories.getByRole("link", { name: "Backups" })).toHaveAttribute("aria-current", "page");
    await expect(page.getByRole("button", { name: "Make a backup now" })).toBeVisible();
    await page.goBack();
    await expect(page.getByRole("heading", { name: "Devices", level: 1 })).toBeVisible();
  } else {
    // A phone has no room for both: the list first, each category with what it holds, a fingertip tall.
    await expect(page.getByRole("heading", { name: "Settings", level: 1 })).toBeVisible();
    await expect(page.getByRole("heading", { name: "Devices", level: 1 })).toHaveCount(0);
    const backups = categories.getByRole("link", { name: /^Backups/ });
    await expect(backups).toContainText("Daily copies, saving to USB, restoring");
    for (const link of await links.all()) expect((await link.boundingBox())!.height).toBeGreaterThanOrEqual(44);
    expect((await search.boundingBox())!.height).toBeGreaterThanOrEqual(44);
    await noHorizontalScroll(page);
    // A category opens as a page of its own: "Settings › Backups", with the way back.
    await backups.click();
    await expect(page).toHaveURL(/#settings\/backups$/);
    await expect(page.getByRole("heading", { name: "Backups", level: 1 })).toBeFocused();
    await expect(page.getByRole("navigation", { name: "Categories" })).toHaveCount(0);
    await expect(page.getByRole("navigation", { name: "Breadcrumb" }).getByRole("link", { name: "Settings", exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "Make a backup now" })).toBeVisible();
    const back = page.getByRole("link", { name: "Back to Settings" });
    const size = (await back.boundingBox())!;
    expect(size.height).toBeGreaterThanOrEqual(44);
    expect(size.width).toBeGreaterThanOrEqual(44);
    await noHorizontalScroll(page);
    // The way back returns to the list, with the focus on the category just left.
    await back.click();
    await expect(page).toHaveURL(/#settings$/);
    await expect(backups).toBeFocused();
    // The browser's back button returns to the list too.
    await categories.getByRole("link", { name: /^Privacy & security/ }).click();
    await expect(page.getByRole("heading", { name: "Privacy & security", level: 1 })).toBeVisible();
    await page.goBack();
    await expect(page.getByRole("heading", { name: "Settings", level: 1 })).toBeVisible();
    // And so does Settings in the bar.
    await categories.getByRole("link", { name: /^About/ }).click();
    await expect(page.getByRole("heading", { name: "About", level: 1 })).toBeVisible();
    await page.locator("nav.nav").getByRole("button", { name: "Settings" }).click();
    await expect(page).toHaveURL(/#settings$/);
    await expect(page.getByRole("heading", { name: "Settings", level: 1 })).toBeVisible();
  }
  await noHorizontalScroll(page);
});

test("settings: Help is at the end of the list, and belongs to Settings in the bar", async ({ page }) => {
  await ensureSetUp(page);
  await page.goto("/#settings");
  await page.getByRole("navigation", { name: "Categories" }).getByRole("link", { name: /^Help/ }).click();
  await expect(page).toHaveURL(/#help$/);
  await expect(page.getByRole("heading", { name: "Help", level: 1 })).toBeVisible();
  await expect(page.locator("nav.nav").getByRole("button", { name: "Settings" })).toHaveAttribute("aria-current", "true");
  // "Settings › Help": the way back to Settings.
  await page.getByRole("navigation", { name: "Breadcrumb" }).getByRole("link", { name: "Settings", exact: true }).click();
  await expect(page).toHaveURL(/#settings$/);
  await expect(page.locator("nav.nav").getByRole("button", { name: "Settings" })).toHaveAttribute("aria-current", "page");
  // A help page opened from a screen's "How it works" belongs to Settings too.
  await page.goto("/#help/supplies");
  await expect(page.locator("nav.nav").getByRole("button", { name: "Settings" })).toHaveAttribute("aria-current", "true");
  await noHorizontalScroll(page);
});

test("settings: the old Household addresses lead to the same place", async ({ page }) => {
  await ensureSetUp(page);
  await page.goto("/#household/backups");
  await expect(page).toHaveURL(/#settings\/backups$/);
  await expect(page.getByRole("heading", { name: "Backups", level: 1 })).toBeVisible();
  // One setting on a page.
  await page.goto("/#household/privacy/password");
  await expect(page).toHaveURL(/#settings\/privacy\/password$/);
  await expect(page.locator("#set-password")).toBeInViewport();
  // Settings itself.
  await page.goto("/#household");
  await expect(page).toHaveURL(/#settings$/);
  await expect(page.locator("nav.nav").getByRole("button", { name: "Settings" })).toHaveAttribute("aria-current", "page");
  // A link to an old address inside the app.
  await page.evaluate(() => {
    location.hash = "household/language";
  });
  await expect(page).toHaveURL(/#settings\/language$/);
  await expect(page.getByRole("heading", { name: "Language", level: 1 })).toBeVisible();
  // The app opened fresh at an old address.
  await page.goto("about:blank");
  await page.goto("/#household/appearance");
  await expect(page).toHaveURL(/#settings\/appearance$/);
  await expect(page.getByRole("heading", { name: "Appearance", level: 1 })).toBeVisible();
  // The old address of a help page.
  await page.goto("/#help/household/backups");
  await expect(page).toHaveURL(/#help\/settings\/backups$/);
  await expect(page.getByRole("heading", { name: "Settings", level: 1 })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Backups", level: 2 })).toBeInViewport();
  // Back goes through the new addresses.
  await page.goBack();
  await expect(page).toHaveURL(/#settings\/appearance$/);
});

test("settings: the search finds a setting by its name or other words, in English or Serbian, and opens it", async ({ page }) => {
  await ensureSetUp(page);
  // Whatever Wi-Fi this computer has, the panel shows (simulated).
  await page.route("**/api/hotspot", (r) => r.fulfill({ json: { supported: true, on: false, ssid: "PC 1234", passphrase: "", clients: 0, error: null, qr: null } }));
  // At the top of the list of categories (beside the page on a laptop, the list itself on a phone).
  await page.goto("/#settings");
  const search = page.getByRole("searchbox", { name: "Find a setting" });
  const results = page.locator(".set-results");
  await search.fill("wifi");
  const hotspot = results.getByRole("link", { name: /Wi-Fi network from this laptop/ });
  await expect(hotspot).toContainText("Network");
  // The results take the place of the list.
  await expect(page.getByRole("navigation", { name: "Categories" })).toHaveCount(0);
  await hotspot.click();
  await expect(page).toHaveURL(/#settings\/network\/hotspot$/);
  await expect(page.getByRole("heading", { name: "Network", level: 1 })).toBeVisible();
  await expect(page.locator("#set-hotspot")).toBeFocused();
  await expect(page.locator("#set-hotspot")).toBeInViewport();

  // Serbian words find English settings (without the accents too); Enter opens the first.
  await page.goto("/#settings");
  await search.fill("sifrovanje");
  await expect(results.getByRole("link").first()).toContainText("Backup encryption");
  await search.press("Enter");
  await expect(page).toHaveURL(/#settings\/backups\/backup-encryption$/);
  await expect(page.locator("#set-backup-encryption")).toBeFocused();
  await expect(page.locator("#set-backup-encryption")).toBeInViewport();

  // The arrow keys go through the results.
  await page.goto("/#settings");
  await search.fill("lozinka");
  await expect(results.getByRole("link").first()).toContainText("Household password");
  await search.press("ArrowDown");
  await expect(results.getByRole("link").first()).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/#settings\/privacy\/password$/);
  await expect(page.getByRole("heading", { name: "Household password" })).toBeInViewport();

  // Words that match nothing are answered; Escape brings the list back.
  await page.goto("/#settings");
  await search.fill("zzqx");
  await expect(page.getByText("No setting matches that. Try another word.")).toBeVisible();
  await search.press("Escape");
  await expect(search).toHaveValue("");
  await expect(page.getByRole("navigation", { name: "Categories" })).toBeVisible();

  // In Serbian the results are in Serbian, and English words still find them.
  await setLanguage(page, "sr");
  await page.goto("/#settings");
  await page.getByRole("searchbox", { name: "Pronađi podešavanje" }).fill("accent");
  await expect(results.getByRole("link").first()).toContainText("Boja akcenta");
  await setLanguage(page, "en");
});

test("settings: an address opens a category, or one setting on it, directly", async ({ page }, info) => {
  await ensureSetUp(page);
  // A fresh start at the address of a category.
  await page.goto("/#settings/appearance");
  await page.reload();
  await expect(page.getByRole("heading", { name: "Appearance", level: 1 })).toBeVisible();
  await expect(page.getByRole("radio", { name: "Amber" })).toBeVisible();
  // ...and of one setting, which the page opens at.
  await page.goto("/#settings/about/licenses");
  await page.reload();
  await expect(page.getByRole("heading", { name: "About", level: 1 })).toBeVisible();
  await expect(page.locator("#set-licenses")).toBeInViewport();
  await expect(page.locator("#set-licenses")).toBeFocused();
  // A setting whose answer comes a moment later (Windows is slow to say): it says so, then shows it.
  await page.route("**/api/firewall", async (r) => {
    await new Promise((done) => setTimeout(done, 800));
    return r.fulfill({ json: { checked: true, firewall_on: true, allowed: true, blocked: false, public_network: false, error: null, ok: true } });
  });
  await page.goto("/#settings/network/firewall");
  await page.reload();
  await expect(page.locator("#set-firewall")).toContainText("Asking Windows…");
  await expect(page.locator("#set-firewall")).toContainText("Phones on the Wi-Fi can reach Zaklon");
  await expect(page.locator("#set-firewall")).toBeInViewport();
  await expect(page.locator("#set-firewall")).toBeFocused();
  // An address that is no category shows the first one on a laptop, the list on a phone.
  await page.goto("/#settings/nowhere");
  await expect(page.getByRole("heading", { name: info.project.name === "laptop" ? "Devices" : "Settings", level: 1 })).toBeVisible();
  await expect(page.locator("nav.nav").getByRole("button", { name: "Settings" })).toHaveAttribute("aria-current", "page");
  // Back goes through them in turn.
  await page.goBack();
  await expect(page.getByRole("heading", { name: "Network", level: 1 })).toBeVisible();
});
