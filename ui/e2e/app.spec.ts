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
  await expect(page.locator(".item.check", { hasText: name })).toBeVisible();

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
  await expect(page.getByRole("heading", { name: "Household" })).toBeVisible();
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
