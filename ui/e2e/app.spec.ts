import { expect, test, type Page } from "@playwright/test";

// The hub starts empty for the whole run; tests run in order and build on
// each other (setup first). The "phone" project runs after "laptop" against
// the same hub, so every test must work whether or not setup already happened.

const PASSWORD = "correct horse";
/** The hub's "install the app" port (see start-hub.mjs). */
const INSTALL_PORT = Number(process.env.ZAKLON_E2E_PORT_BASE || 28480);

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
  // Setup hashes the password with Argon2, slow on purpose; a busy machine needs longer.
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
  await page.goto("/#household/devices");
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

  await page.goto("/#home");
  await expect(page.locator(".panel-list", { hasText: "Running low" }).getByText(name)).toBeVisible();
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
  await expect(page.getByRole("navigation", { name: "Putanja" }).getByRole("link", { name: "Domaćinstvo" })).toBeVisible();
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
  await page.goto("/#household");
  await expect(page.getByRole("heading", { name: "Household", exact: true })).toBeVisible();
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
  await page.getByRole("list", { name: "Folders" }).getByRole("button", { name: /^Wikipedia and books/ }).click();
  await expect(page.getByText("Wikipedia in Serbian (with pictures)")).toBeVisible();
});

test("every screen fits the width of the device", async ({ page }, info) => {
  await ensureSetUp(page);
  // A long name, expiring soon: it shows on Supplies and on Home, and must be cut short there, not widen the page.
  const soon = new Date(Date.now() + 5 * 86400000).toISOString().slice(0, 10);
  const name = `Extra virgin olive oil from the cooperative in Istria, 750 ml (${info.project.name})`;
  const res = await page.request.post("/api/items", { data: { name, quantity: 2, unit: "pcs", category: "food", expiry: soon, min_quantity: 3 } });
  expect(res.ok()).toBe(true);
  const household = ["devices", "network", "backups", "privacy", "appearance", "language", "assistant", "updates", "about"].map((c) => `household/${c}`);
  for (const tab of ["home", "tools", "library", "maps", "supplies", "assistant", "addons", "household", ...household]) {
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
  // Home: every date badge stays inside its panel.
  await page.goto("/#home");
  await expect(page.locator(".mini-row", { hasText: name }).first()).toBeVisible();
  const outside = await page.evaluate(() =>
    [...document.querySelectorAll(".mini-row")].filter((row) => {
      const panel = row.closest(".panel")!.getBoundingClientRect();
      return [...row.children].some((c) => c.getBoundingClientRect().right > panel.right + 1);
    }).length,
  );
  expect(outside, "nothing sticks out of the Home panels").toBe(0);
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
  await page.locator(".tool-grid").getByRole("button", { name: /^Supplies/ }).click();
  await expect(page).toHaveURL(/#supplies$/);
  await page.reload();
  await expect(page.getByRole("heading", { name: "Supplies" })).toBeVisible();
  // A tool that is not pinned is found under Tools, and the bar says so.
  await expect(page.locator("nav").getByRole("button", { name: "Tools" })).toHaveAttribute("aria-current", "true");
  // Every screen keeps its own address.
  for (const [tab, heading] of [["library", "Library"], ["maps", "Maps"], ["addons", "Add-ons"], ["assistant", "Assistant"], ["tools", "Tools"], ["household", "Household"]]) {
    await page.goto(`/#${tab}`);
    await expect(page.getByRole("heading", { name: heading, exact: true })).toBeVisible();
  }
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
  for (const name of ["Supplies", "Library", "Maps", "Add-ons"]) {
    await expect(page.locator(".tool-card", { hasText: name }).first()).toBeVisible();
  }
  await expect(page.getByText(/expiry dates and a shopping list/)).toBeVisible();
  expect(await barItems(page)).toEqual(["Home", "Assistant", "Tools", "Household"]);

  // The bar is along the bottom of the window, on the laptop too.
  const nav = await page.locator("nav").boundingBox();
  const height = page.viewportSize()!.height;
  expect(nav!.y + nav!.height).toBeGreaterThan(height - 2);
  expect(nav!.y + nav!.height).toBeLessThanOrEqual(height + 1);

  await page.getByRole("button", { name: "Pin to the bar: Maps" }).click();
  await expect(page.locator("nav").getByRole("button", { name: "Maps" })).toBeVisible();
  expect(await barItems(page)).toEqual(["Home", "Assistant", "Maps", "Tools", "Household"]);
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
  expect(await barItems(page)).toEqual(["Home", "Assistant", "Library", "Tools", "Household"]);
  // Five items: none is cut short, in Serbian either (the longest is "Domaćinstvo").
  await setLanguage(page, "sr");
  await expect(page.locator("nav").getByRole("button", { name: "Domaćinstvo" })).toBeVisible();
  const cut = await page.evaluate(() =>
    [...document.querySelectorAll(".nav-label")].filter((l) => l.scrollWidth > l.clientWidth + 1).map((l) => l.textContent),
  );
  expect(cut, "labels cut short in the bar").toEqual([]);
  await setLanguage(page, "en");
  await page.goto("/#tools");

  await page.getByRole("button", { name: "Unpin: Library" }).click();
  await expect(page.locator("nav").getByRole("button", { name: "Library" })).toHaveCount(0);
  expect(await barItems(page)).toEqual(["Home", "Assistant", "Tools", "Household"]);
  expect((await (await page.request.get("/api/pinned-tool")).json()).tool).toBe(null);
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
  for (const tab of ["home", "supplies", "household", "library", "addons", "tools"]) {
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
  await page.goto("/#household/language");
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
  await page.goto("/#maps");
  await expect(page.getByRole("heading", { name: "Maps", exact: true })).toBeVisible();
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
  await expect(page).toHaveURL(/#maps$/);
});

test("household: accent color is remembered, password can be changed, hub facts and privacy are shown", async ({ page }) => {
  await ensureSetUp(page);
  await page.goto("/#household/appearance");
  await page.getByRole("radio", { name: "Blue" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-accent", "blue");
  await page.getByLabel(/Pure black background/).check();
  await expect(page.locator("html")).toHaveAttribute("data-oled", "1");
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-accent", "blue");
  await expect(page.getByRole("radio", { name: "Blue" })).toHaveAttribute("aria-checked", "true");
  await page.getByRole("radio", { name: "Amber" }).click();
  await page.getByLabel(/Pure black background/).uncheck();

  await page.goto("/#household/privacy");
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

  await page.goto("/#household/about");
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
  // Knowledge by topic, AI models, maps and programs.
  for (const name of ["Wikipedia and books", "Health and first aid", "Garden and food", "Repair and skills", "AI models", "Maps", "Programs"]) {
    await expect(folders.getByRole("button", { name: new RegExp(`^${name}`) })).toBeVisible();
  }
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
  await table.getByRole("button", { name: /^Wikipedia and books/ }).click();
  const row = page.getByRole("table", { name: "Wikipedia and books" }).getByRole("row").filter({ hasText: "Wikipedia in Serbian (with pictures)" });
  await expect(row).toContainText("CC BY-SA 4.0");
  await expect(row).toContainText("Not downloaded");
  await expect(row.getByRole("button", { name: /^Download/ })).toBeVisible();
  await noHorizontalScroll(page);
  // Still details after a reload; back to tiles.
  await page.reload();
  await expect(page.getByRole("table", { name: "Wikipedia and books" })).toBeVisible();
  await page.getByRole("group", { name: "View" }).getByRole("button", { name: "Tiles" }).click();
  await expect(page.getByRole("list", { name: "Wikipedia and books" })).toBeVisible();
  expect(await page.evaluate(() => localStorage.getItem("zaklon.addonsView"))).toBe("tiles");
});

test("a download that the hub accepts without a body is not reported as an error", async ({ page }) => {
  await ensureSetUp(page);
  // The hub answers 202 with an empty body; answer the same way without going online.
  let asked = false;
  await page.route("**/api/maps/Montenegro/download", (route) => {
    asked = true;
    return route.fulfill({ status: 202, body: "" });
  });
  await page.goto("/#maps");
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

test("household: a backup can be made and a restore is prepared for the next start", async ({ page }) => {
  await ensureSetUp(page);
  await page.goto("/#household/backups");
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

test("updates: Home tells about a newer version; Household has the switch", async ({ page }) => {
  await ensureSetUp(page);
  await page.route("**/api/updates", (route) =>
    route.fulfill({
      json: { enabled: true, current: "0.1.0", latest: "0.2.0", newer: true, url: "https://github.com/stefan-cirovic/zaklon/releases/tag/v0.2.0", checked_at: "2026-09-28T08:00:00Z", error: null },
    }),
  );
  await page.goto("/#home");
  await expect(page.getByText("A newer Zaklon is available:")).toBeVisible();
  await expect(page.locator(".update-banner strong")).toHaveText("0.2.0");
  // The top of Household says so too, and leads to the switch.
  await page.goto("/#household");
  await page.locator(".hub-card").getByRole("link", { name: "New version 0.2.0" }).click();
  await expect(page).toHaveURL(/#household\/updates\/updates$/);
  await expect(page.getByRole("heading", { name: "Updates", level: 1 })).toBeVisible();
  await expect(page.getByText(/Check once a day whether a newer Zaklon is out/)).toBeVisible();
  await expect(page.getByRole("button", { name: "Check now" })).toBeVisible();
  await noHorizontalScroll(page);
});

test("assistant memory: notes can be added and deleted by hand", async ({ page }) => {
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
  await page.getByRole("button", { name: /What the assistant remembers/ }).click();
  await page.getByRole("textbox", { name: "Add" }).fill(text);
  await page.getByRole("button", { name: "Add", exact: true }).click();
  const row = page.locator(".memory-row").filter({ hasText: text });
  await expect(row).toBeVisible();
  await row.getByRole("button", { name: "Delete" }).click();
  await row.getByRole("button", { name: /Yes, delete/ }).click();
  await expect(row).toHaveCount(0);
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

test("household: the laptop can offer its own Wi-Fi network (simulated, never really started)", async ({ page }) => {
  await ensureSetUp(page);
  const off = { supported: true, on: false, ssid: "PC 1234", passphrase: "", clients: 0, error: null, qr: null };
  const on = { supported: true, on: true, ssid: "Zaklon", passphrase: "abcd2345ef", clients: 1, error: null, qr: "WIFI:T:WPA;S:Zaklon;P:abcd2345ef;;" };
  await page.route("**/api/hotspot", (r) => r.fulfill({ json: off }));
  await page.route("**/api/hotspot/start", (r) => r.fulfill({ json: on }));
  await page.route("**/api/hotspot/stop", (r) => r.fulfill({ json: off }));
  // Reload so the panel asks again (and gets the simulated answer).
  await page.goto("/#household/network");
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
  await panel.getByRole("button", { name: "Download all" }).click();
  await expect.poll(() => asked.length).toBeGreaterThanOrEqual(3);
  expect(asked).toContain("/api/packs/wikimed-en/download");
  expect(asked.some((p) => /\/api\/packs\/qwen35-/.test(p))).toBe(true);
});

test("household: warns when Windows Firewall would keep phones out, and fixes it (simulated)", async ({ page }) => {
  await ensureSetUp(page);
  const bad = { checked: true, firewall_on: true, allowed: false, blocked: true, error: null, ok: false };
  const good = { ...bad, allowed: true, blocked: false, ok: true };
  let fixed = false;
  await page.route("**/api/firewall", (r) => r.fulfill({ json: fixed ? good : bad }));
  await page.route("**/api/firewall/allow", (r) => {
    fixed = true;
    return r.fulfill({ json: good });
  });
  // On the tiles, where it cannot be missed, and under Network.
  await page.goto("/#household");
  await page.reload();
  await expect(page.locator(".firewall").getByText(/Windows Firewall is blocking Zaklon/)).toBeVisible();
  await page.goto("/#household/network");
  const panel = page.locator(".firewall");
  await expect(panel.getByText(/Windows Firewall is blocking Zaklon/)).toBeVisible();
  await panel.getByRole("button", { name: "Let phones connect" }).click();
  await expect(panel).toHaveCount(0);
  await expect(page.locator(".firewall-ok")).toContainText("Phones on the Wi-Fi can reach Zaklon");
  await page.goto("/#household");
  await expect(page.getByRole("heading", { name: "Household", exact: true })).toBeVisible();
  await expect(page.locator(".firewall")).toHaveCount(0);
});

test("household: says how to open a network Windows treats as public (simulated)", async ({ page }) => {
  await ensureSetUp(page);
  const state = { checked: true, firewall_on: true, allowed: true, blocked: false, public_network: true, error: null, ok: false };
  await page.route("**/api/firewall", (r) => r.fulfill({ json: state }));
  await page.goto("/#household");
  await page.reload();
  const panel = page.locator(".firewall");
  await expect(panel.getByText(/treats the network this laptop is on as a public network/)).toBeVisible();
  // The rules are there already; asking Windows again would not help.
  await expect(panel.getByRole("button", { name: "Let phones connect" })).toHaveCount(0);
});

test("household: the hub on top, a tile per category; a tile opens its page, and back returns to the tiles", async ({ page }, info) => {
  await ensureSetUp(page);
  const hub = await (await page.request.get("/api/status")).json();
  await expect(page.getByRole("heading", { name: "Household", exact: true })).toBeVisible();
  // The hub's name, that it runs, and a few facts.
  const card = page.locator(".hub-card");
  await expect(card.locator(".hub-name")).toHaveText(hub.hub_name);
  await expect(card).toContainText("Running");
  await expect(card).toContainText(hub.version);
  await expect(card).toContainText("Last backup");
  // Every category, with what it holds.
  const tiles = page.locator(".set-tiles").getByRole("link");
  await expect(tiles).toHaveText([/^Devices/, /^Network/, /^Backups/, /^Privacy & security/, /^Appearance/, /^Language/, /^AI assistant/, /^Updates/, /^About/]);
  const backups = page.getByRole("link", { name: /^Backups/ });
  await expect(backups).toContainText("Daily copies, saving to USB, restoring");
  if (info.project.name === "phone") {
    for (const tile of await tiles.all()) expect((await tile.boundingBox())!.height).toBeGreaterThanOrEqual(44);
    expect((await page.getByRole("searchbox", { name: "Find a setting" }).boundingBox())!.height).toBeGreaterThanOrEqual(44);
  }
  await noHorizontalScroll(page);

  // A tile opens the category as a page: "Household › Backups".
  await backups.click();
  await expect(page).toHaveURL(/#household\/backups$/);
  await expect(page.getByRole("heading", { name: "Backups", level: 1 })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Backups", level: 1 })).toBeFocused();
  await expect(page.getByRole("navigation", { name: "Breadcrumb" }).getByRole("link", { name: "Household", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Make a backup now" })).toBeVisible();
  const categories = page.getByRole("navigation", { name: "Categories" });
  if (info.project.name === "laptop") {
    // The other categories are listed beside it, this one marked, as in Windows Settings.
    await expect(categories.getByRole("link", { name: "Backups" })).toHaveAttribute("aria-current", "page");
    await categories.getByRole("link", { name: "Appearance" }).click();
    await expect(page).toHaveURL(/#household\/appearance$/);
    await expect(page.getByRole("heading", { name: "Appearance", level: 1 })).toBeVisible();
    await page.goBack();
    await expect(page.getByRole("heading", { name: "Backups", level: 1 })).toBeVisible();
  } else {
    // A phone has the room for the page only; the way back is a fingertip wide.
    await expect(categories).toBeHidden();
    const back = await page.getByRole("link", { name: "Back to Household" }).boundingBox();
    expect(back!.height).toBeGreaterThanOrEqual(44);
    expect(back!.width).toBeGreaterThanOrEqual(44);
  }
  await noHorizontalScroll(page);

  // The back arrow returns to the tiles, with the focus on the tile just left.
  await page.getByRole("link", { name: "Back to Household" }).click();
  await expect(page).toHaveURL(/#household$/);
  await expect(backups).toBeFocused();
  // The browser's back button returns to the tiles too.
  await page.getByRole("link", { name: /^Privacy & security/ }).click();
  await expect(page.getByRole("heading", { name: "Privacy & security", level: 1 })).toBeVisible();
  await page.goBack();
  await expect(page).toHaveURL(/#household$/);
  await expect(page.locator(".set-tiles")).toBeVisible();
  // And so does Household in the bar.
  await page.getByRole("link", { name: /^About/ }).click();
  await expect(page.getByRole("heading", { name: "About", level: 1 })).toBeVisible();
  await page.locator("nav.nav").getByRole("button", { name: "Household" }).click();
  await expect(page).toHaveURL(/#household$/);
  await expect(page.locator(".set-tiles")).toBeVisible();
});

test("household: the search finds a setting by its name or other words, in English or Serbian, and opens it", async ({ page }) => {
  await ensureSetUp(page);
  // Whatever Wi-Fi this computer has, the panel shows (simulated).
  await page.route("**/api/hotspot", (r) => r.fulfill({ json: { supported: true, on: false, ssid: "PC 1234", passphrase: "", clients: 0, error: null, qr: null } }));
  const search = page.getByRole("searchbox", { name: "Find a setting" });
  const results = page.locator(".set-results");
  await search.fill("wifi");
  const hotspot = results.getByRole("link", { name: /Wi-Fi network from this laptop/ });
  await expect(hotspot).toContainText("Network");
  // The results take the place of the tiles.
  await expect(page.locator(".set-tiles")).toHaveCount(0);
  await hotspot.click();
  await expect(page).toHaveURL(/#household\/network\/hotspot$/);
  await expect(page.getByRole("heading", { name: "Network", level: 1 })).toBeVisible();
  await expect(page.locator("#set-hotspot")).toBeFocused();
  await expect(page.locator("#set-hotspot")).toBeInViewport();

  // Serbian words find English settings (without the accents too); Enter opens the first.
  await page.goto("/#household");
  await search.fill("sifrovanje");
  await expect(results.getByRole("link").first()).toContainText("Backup encryption");
  await search.press("Enter");
  await expect(page).toHaveURL(/#household\/backups\/backup-encryption$/);
  await expect(page.locator("#set-backup-encryption")).toBeFocused();
  await expect(page.locator("#set-backup-encryption")).toBeInViewport();

  // The arrow keys go through the results.
  await page.goto("/#household");
  await search.fill("lozinka");
  await expect(results.getByRole("link").first()).toContainText("Household password");
  await search.press("ArrowDown");
  await expect(results.getByRole("link").first()).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/#household\/privacy\/password$/);
  await expect(page.getByRole("heading", { name: "Household password" })).toBeInViewport();

  // Words that match nothing are answered; Escape brings the tiles back.
  await page.goto("/#household");
  await search.fill("zzqx");
  await expect(page.getByText("No setting matches that. Try another word.")).toBeVisible();
  await search.press("Escape");
  await expect(search).toHaveValue("");
  await expect(page.locator(".set-tiles")).toBeVisible();

  // In Serbian the results are in Serbian, and English words still find them.
  await setLanguage(page, "sr");
  await page.goto("/#household");
  await page.getByRole("searchbox", { name: "Pronađi podešavanje" }).fill("accent");
  await expect(results.getByRole("link").first()).toContainText("Boja akcenta");
  await setLanguage(page, "en");
});

test("household: an address opens a category, or one setting on it, directly", async ({ page }) => {
  await ensureSetUp(page);
  // A fresh start at the address of a category.
  await page.goto("/#household/appearance");
  await page.reload();
  await expect(page.getByRole("heading", { name: "Appearance", level: 1 })).toBeVisible();
  await expect(page.getByRole("radio", { name: "Amber" })).toBeVisible();
  // ...and of one setting, which the page opens at.
  await page.goto("/#household/about/licenses");
  await page.reload();
  await expect(page.getByRole("heading", { name: "About", level: 1 })).toBeVisible();
  await expect(page.locator("#set-licenses")).toBeInViewport();
  await expect(page.locator("#set-licenses")).toBeFocused();
  // A setting whose answer comes a moment later (Windows is slow to say): it says so, then shows it.
  await page.route("**/api/firewall", async (r) => {
    await new Promise((done) => setTimeout(done, 800));
    return r.fulfill({ json: { checked: true, firewall_on: true, allowed: true, blocked: false, public_network: false, error: null, ok: true } });
  });
  await page.goto("/#household/network/firewall");
  await page.reload();
  await expect(page.locator("#set-firewall")).toContainText("Asking Windows…");
  await expect(page.locator("#set-firewall")).toContainText("Phones on the Wi-Fi can reach Zaklon");
  await expect(page.locator("#set-firewall")).toBeInViewport();
  await expect(page.locator("#set-firewall")).toBeFocused();
  // An address that is no category shows the tiles.
  await page.goto("/#household/nowhere");
  await expect(page.locator(".set-tiles")).toBeVisible();
  await expect(page.locator("nav.nav").getByRole("button", { name: "Household" })).toHaveAttribute("aria-current", "page");
  // Back goes through them in turn.
  await page.goBack();
  await expect(page.getByRole("heading", { name: "Network", level: 1 })).toBeVisible();
});
