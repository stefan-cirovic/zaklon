import { expect, test, type Page } from "@playwright/test";

// The hub starts empty for the whole run; tests run in order and build on
// each other (setup first). The "phone" project runs after "laptop" against
// the same hub, so every test must work whether or not setup already happened.

const PASSWORD = "correct horse";

async function ensureSetUp(page: Page) {
  await page.goto("/#household");
  const setup = page.getByRole("heading", { name: /Set up your household|Podesi domaćinstvo/ });
  if (await setup.isVisible({ timeout: 3000 }).catch(() => false)) {
    await page.getByLabel(/Hub name|Ime huba/).fill("E2E hub");
    await page.getByLabel(/^Household password$|^Lozinka domaćinstva$/).fill(PASSWORD);
    await page.getByLabel(/Repeat password|Ponovi lozinku/).fill(PASSWORD);
    await page.getByRole("button", { name: /Finish setup|Završi podešavanje/ }).click();
  }
  await expect(page.getByRole("button", { name: /Add a phone|Dodaj telefon/ })).toBeVisible();
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
  await page.getByRole("button", { name: "Add a phone" }).click();
  await expect(page.getByRole("heading", { name: "Pair a phone" })).toBeVisible();
  await expect(page.getByRole("img", { name: /QR code/ })).toHaveCount(2);
  await expect(page.locator(".code")).toHaveText(/^\d{6}$/);
  await expect(page.getByText(/http:\/\/.+:28480\/get/)).toBeVisible();
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
  await page.getByRole("button", { name: "Cancel" }).last().click();
  await expect(page.getByRole("heading", { name: "Edit item" })).toBeVisible();
  await page.getByRole("button", { name: "Delete" }).click();
  await page.getByRole("button", { name: "Yes, delete" }).click();
  await expect(page.locator(".item.supply", { hasText: name })).toHaveCount(0);
});

test("language switch to Serbian and back, with Serbian number format", async ({ page }, info) => {
  await ensureSetUp(page);
  await page.locator("select").first().selectOption("sr");
  await expect(page.getByRole("heading", { name: "Domaćinstvo" })).toBeVisible();
  await page.goto("/#supplies");
  await expect(page.getByRole("heading", { name: "Zalihe" })).toBeVisible();
  const name = `Šećer ${info.project.name}`;
  await page.getByRole("button", { name: "Dodaj stavku" }).click();
  await page.getByLabel("Naziv").fill(name);
  await page.getByLabel("Količina").fill("1,5");
  await page.getByLabel("Jedinica").selectOption("kg");
  await page.getByRole("button", { name: "Sačuvaj" }).click();
  await expect(page.locator(".item.supply", { hasText: name }).locator(".qty-val")).toContainText("1,5");
  await page.goto("/#household");
  await page.locator("select").first().selectOption("en");
  await expect(page.getByRole("heading", { name: "Household", exact: true })).toBeVisible();
});

test("library without packs points to add-ons, which lists the catalog", async ({ page }) => {
  await ensureSetUp(page);
  await page.goto("/#library");
  await expect(page.getByText(/No knowledge packs yet/)).toBeVisible();
  await page.getByRole("button", { name: "Open Add-ons" }).click();
  await expect(page.getByRole("heading", { name: "Add-ons" })).toBeVisible();
  await expect(page.getByText("Wikipedia in Serbian (with pictures)")).toBeVisible();
  await expect(page.getByText("Free disk space")).toBeVisible();
  await expect(page.getByText("Battery")).toBeVisible();
});

test("every screen fits the width of the device", async ({ page }) => {
  await ensureSetUp(page);
  for (const tab of ["home", "library", "maps", "supplies", "assistant", "addons", "household"]) {
    await page.goto(`/#${tab}`);
    await page.waitForTimeout(300);
    await noHorizontalScroll(page);
  }
});

test("the tab is remembered in the address", async ({ page }) => {
  await ensureSetUp(page);
  await page.goto("/#home");
  await page.locator("nav").getByRole("button", { name: /Supplies/ }).click();
  await expect(page).toHaveURL(/#supplies$/);
  await page.reload();
  await expect(page.getByRole("heading", { name: "Supplies" })).toBeVisible();
});

test("an idle screen does not flood the hub with requests", async ({ page }) => {
  test.setTimeout(90_000);
  await ensureSetUp(page);
  for (const tab of ["home", "supplies", "household", "library", "addons"]) {
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
  await page.locator("select").first().selectOption("sr");
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
  await page.goto("/#household");
  await page.locator("select").first().selectOption("en");
});

test("the Latin-script switch is remembered on this device", async ({ page }) => {
  await ensureSetUp(page);
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
  // Map pieces are not repeated in the add-ons list; there is a link instead.
  await page.goto("/#addons");
  await expect(page.getByText(/Maps of the world are chosen on their own screen/)).toBeVisible();
  await expect(page.getByText("Montenegro")).toHaveCount(0);
});

test("household: accent color is remembered, password can be changed, hub facts and privacy are shown", async ({ page }) => {
  await ensureSetUp(page);
  await page.goto("/#household");
  await page.getByRole("radio", { name: "Blue" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-accent", "blue");
  await page.getByLabel(/Pure black background/).check();
  await expect(page.locator("html")).toHaveAttribute("data-oled", "1");
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-accent", "blue");
  await expect(page.getByRole("radio", { name: "Blue" })).toHaveAttribute("aria-checked", "true");
  await page.getByRole("radio", { name: "Green" }).click();
  await page.getByLabel(/Pure black background/).uncheck();

  const form = page.locator("form").filter({ has: page.getByRole("heading", { name: "Household password" }) });
  await form.getByLabel("New password").fill("something else");
  await form.getByLabel("Repeat password").fill("something elsE");
  await form.getByRole("button", { name: "Change password" }).click();
  await expect(form.getByRole("alert")).toHaveText(/do not match/i);
  await form.getByLabel("New password").fill(PASSWORD);
  await form.getByLabel("Repeat password").fill(PASSWORD);
  await form.getByRole("button", { name: "Change password" }).click();
  await expect(form.getByText("Password changed.")).toBeVisible();

  await expect(page.getByRole("heading", { name: "This hub" })).toBeVisible();
  await expect(page.getByText("Memory")).toBeVisible();
  await page.getByText("Privacy", { exact: true }).click();
  await expect(page.getByText(/No tracking, no analytics/)).toBeVisible();
  await page.getByText("Licenses and credits").click();
  await expect(page.getByText("Kiwix (kiwix-serve)")).toBeVisible();
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
  await page.goto("/#household");
  await expect(page.getByRole("heading", { name: "Backups" })).toBeVisible();
  await page.getByRole("button", { name: "Make a backup now" }).click();
  await expect(page.getByText(/^Saved: .*zaklon-backup-.*\.zip$/)).toBeVisible();
  const row = page.locator(".backup-row").first();
  await expect(row).toContainText("saved by hand");
  await row.getByRole("button", { name: "Restore" }).click();
  await row.getByRole("button", { name: "Yes, restore" }).click();
  await expect(page.getByText(/The backup is checked and ready/)).toBeVisible();
  // Wait until that restore has finished (the notice may already be there from an earlier run).
  await expect(page.getByRole("button", { name: "Make a backup now" })).toBeEnabled();
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
  await page.goto("/#household");
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
  // Same address as after setup: reload so the panel asks again (and gets the simulated answer).
  await page.goto("/#household");
  await page.reload();
  await page.getByRole("button", { name: "Make the Wi-Fi network" }).click();
  await expect(page.getByText("abcd2345ef")).toBeVisible();
  await expect(page.getByRole("img", { name: "QR code to join the Wi-Fi network" })).toBeVisible();
  await page.getByRole("button", { name: "Turn the network off" }).click();
  await expect(page.getByRole("button", { name: "Make the Wi-Fi network" })).toBeVisible();
  await noHorizontalScroll(page);
});

test("add-ons: the starter set downloads the recommended packs in one go (requests intercepted)", async ({ page }) => {
  await ensureSetUp(page);
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
  await page.route("**/api/firewall", (r) => r.fulfill({ json: bad }));
  await page.route("**/api/firewall/allow", (r) => r.fulfill({ json: good }));
  await page.goto("/#household");
  await page.reload();
  const panel = page.locator(".firewall");
  await expect(panel.getByText(/Windows Firewall is blocking Zaklon/)).toBeVisible();
  await panel.getByRole("button", { name: "Let phones connect" }).click();
  await expect(panel).toHaveCount(0);
});
