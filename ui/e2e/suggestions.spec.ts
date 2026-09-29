import { expect, test, type Page } from "@playwright/test";

// Tools and guides under an answer, against the real hub of the interface
// tests. That hub has no AI engine, so every answer fails at once; the hub
// reads the suggestions from the question's words, so they come all the
// same. The hub's assistant overview is answered by the test ("ready") so
// that the question box is there.

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

async function assistantReady(page: Page) {
  await page.route("**/api/assistant", (route) =>
    route.fulfill({
      json: {
        engine: "ready", engine_installed: true, selected: "qwen35-2b", recommended: "qwen35-2b", ram_total: 8e9, books: 1,
        models: [{ id: "qwen35-2b", title_en: "AI model for phones (Qwen3.5 2B)", title_sr: "x", size: 1e9, installed: true, recommended: true }],
      },
    }),
  );
}

async function noHorizontalScroll(page: Page) {
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow, "page must not be wider than the screen").toBeLessThanOrEqual(1);
}

/** Ask, and wait for the answer (here always "failed": there is no AI engine). */
async function ask(page: Page, question: string) {
  await page.getByRole("textbox", { name: "Ask something" }).fill(question);
  await page.getByRole("button", { name: "Ask the assistant" }).click();
  await expect(page.locator(".exchange", { hasText: question }).locator(".error")).toBeVisible();
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

test("suggestions: a sizing question opens the power calculator filled in, and the conversation stays", async ({ page }, info) => {
  await ensureSetUp(page);
  await assistantReady(page);
  const ref = `${info.project.name === "phone" ? "P" : "L"}${Date.now() % 1_000_000}`;
  const question = `A fridge, 6 LED bulbs and a laptop for 3 days: how big a battery? (ref ${ref})`;
  await page.goto("/#assistant");
  await ask(page, question);

  const answer = page.locator(".exchange", { hasText: question });
  const chips = answer.getByRole("group", { name: "Tools and guides for this" });
  const calc = chips.getByRole("link", { name: "Open the power calculator with this list" });
  await expect(calc).toHaveAttribute("href", "#power?items=fridge:1,lights-led:6,laptop:1&days=3");
  await expect(chips.getByRole("link", { name: "Guides: Power" })).toHaveAttribute("href", "#addons/power");
  await expect(chips.getByRole("link")).toHaveCount(2);
  await noHorizontalScroll(page);

  // The calculator opens with the list from the question, as a draft.
  await calc.click();
  await expect(page).toHaveURL(/#power$/);
  await expect(page.getByRole("heading", { name: "Power calculator", level: 1 })).toBeVisible();
  await expect(page.getByText("Opened from a link.")).toBeVisible();
  const rows = page.locator(".pw-table tbody tr");
  await expect(rows).toHaveCount(3);
  await expect(rows.nth(0)).toContainText("Refrigerator with freezer");
  await expect(rows.nth(1)).toContainText("LED bulb");
  await expect(page.getByRole("textbox", { name: "How many: LED bulb" })).toHaveValue("6");
  await expect(rows.nth(2)).toContainText("Laptop");
  await expect(page.getByRole("button", { name: "3 days" })).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByRole("link", { name: "Back to the conversation" })).toBeVisible();

  // Back returns to the conversation, with its suggestions (saved with the answer).
  await page.goBack();
  await expect(page).toHaveURL(/#assistant$/);
  await expect(page.locator(".exchange .q")).toHaveText([question]);
  await expect(calc).toBeVisible();

  // The guides open Add-ons at the topic's folder; the way back is on the screen too.
  await answer.getByRole("link", { name: "Guides: Power" }).click();
  await expect(page).toHaveURL(/#addons\/power$/);
  await expect(page.getByRole("heading", { name: "Power", level: 1 })).toBeVisible();
  await page.getByRole("link", { name: "Back to the conversation" }).click();
  await expect(page.locator(".exchange .q")).toHaveText([question]);
  await expect(page.getByRole("link", { name: "Back to the conversation" })).toHaveCount(0);
  await noHorizontalScroll(page);
});

test("suggestions: none for small talk or the supplies; a Serbian health question gets its guides", async ({ page }, info) => {
  await ensureSetUp(page);
  await assistantReady(page);
  const ref = `${info.project.name === "phone" ? "P" : "L"}${Date.now() % 1_000_000}`;
  await page.goto("/#assistant");
  const thanks = `Thanks! ${ref}`;
  await ask(page, thanks);
  await expect(page.locator(".exchange", { hasText: thanks }).locator(".ai-suggest")).toHaveCount(0);
  const supplies = `What do we have in the supplies? ${ref}`;
  await ask(page, supplies);
  await expect(page.locator(".exchange", { hasText: supplies }).locator(".ai-suggest")).toHaveCount(0);
  const burn = `šta da radim kod opekotine ${ref}`;
  await ask(page, burn);
  const chips = page.locator(".exchange", { hasText: burn }).getByRole("group", { name: "Tools and guides for this" });
  await expect(chips.getByRole("link")).toHaveCount(1);
  await expect(chips.getByRole("link", { name: "Guides: Health and first aid" })).toHaveAttribute("href", "#addons/health");
});

// Pictures to look at by eye, not a check: only with ZAKLON_SHOTS_DIR (SHOT_SIZE=1600x900
// sets the laptop window). The answers are made up (there is no AI here); the suggestions are the hub's.
test("suggestions: screenshots", async ({ page }, info) => {
  const dir = process.env.ZAKLON_SHOTS_DIR;
  test.skip(!dir, "set ZAKLON_SHOTS_DIR to take screenshots");
  const size = process.env.SHOT_SIZE?.match(/^(\d+)x(\d+)$/);
  if (size && info.project.name === "laptop") await page.setViewportSize({ width: Number(size[1]), height: Number(size[2]) });
  const file = (name: string) => `${dir}/en-${info.project.name}-assist-${name}.png`;
  await ensureSetUp(page);
  await assistantReady(page);
  const texts: Record<string, string> = {
    battery:
      "For a refrigerator, six LED bulbs and a laptop you need about 1.8 kWh a day, so roughly 5.4 kWh for 3 days [1]. " +
      "With a 12 V LiFePO4 battery that is about 560 Ah. The exact sizing, with solar panels and an inverter, is in the power calculator.",
    burn: "Cool the burn under cool running water for 20 minutes [1]. Do not put ice, butter or toothpaste on it [1].",
  };
  await page.route("**/api/assistant/answers/*", async (route) => {
    const json = await (await route.fetch()).json();
    const text = /battery/.test(json.question) ? texts.battery : texts.burn;
    const source = { n: 1, title: /battery/.test(json.question) ? "Battery (electricity)" : "Burn", url: "/kiwix/content/x/A/Battery", book_title_en: "Wikipedia", book_title_sr: "Vikipedija" };
    return route.fulfill({ json: { ...json, status: "done", error: null, grounded: true, cited: true, text, sources: [source], tokens_per_second: 9.4 } });
  });
  await page.goto("/#assistant");
  await ask2(page, "A fridge, 6 LED bulbs and a laptop for 3 days: how big a battery do I need?");
  await ask2(page, "šta da radim kod opekotine");
  await page.waitForTimeout(500);
  await page.screenshot({ path: file("chips") });
  await page.locator(".exchange").first().getByRole("link", { name: "Open the power calculator with this list" }).click();
  await expect(page.getByText("Opened from a link.")).toBeVisible();
  await page.waitForTimeout(500);
  await page.screenshot({ path: file("power") });
  await page.goBack();
  await page.locator(".exchange").last().getByRole("link", { name: /^Guides: / }).click();
  await page.waitForTimeout(800);
  await page.screenshot({ path: file("guides") });
});

async function ask2(page: Page, question: string) {
  await page.getByRole("textbox", { name: "Ask something" }).fill(question);
  await page.getByRole("button", { name: "Ask the assistant" }).click();
  await expect(page.locator(".exchange", { hasText: question }).locator(".ai-suggest")).toBeVisible();
}
