import { expect, test, type Page } from "@playwright/test";

// Saved conversations with the assistant, against the real hub of the
// interface tests. That hub has no AI engine, so every answer fails at once,
// which is enough to save a conversation. The hub's assistant overview is
// answered by the test ("ready") so that the question box is there. The
// laptop and phone projects both drive the hub as the laptop, so every test
// uses names of its own.

const PASSWORD = "correct horse";

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

/** The list of conversations: beside the chat on a laptop, a panel opened from the top on a phone. */
async function conversations(page: Page, phone: boolean) {
  if (phone) {
    await page.getByRole("button", { name: "Conversations" }).click();
    const panel = page.getByRole("dialog", { name: "Conversations" });
    await expect(panel).toBeVisible();
    return panel;
  }
  return page.locator(".convo-side");
}

async function noHorizontalScroll(page: Page) {
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow, "page must not be wider than the screen").toBeLessThanOrEqual(1);
}

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

test("conversations: a question starts one; it is in the list, opens again, is renamed and deleted", async ({ page }, info) => {
  await ensureSetUp(page);
  await assistantReady(page);
  const phone = info.project.name === "phone";
  const stamp = `${info.project.name} ${Date.now() % 1_000_000}`;
  const question = `Candles for ${stamp}`;
  const asked: Record<string, unknown>[] = [];
  page.on("request", (r) => {
    if (r.url().endsWith("/api/assistant/ask")) asked.push(r.postDataJSON());
  });

  await page.goto("/#assistant");
  await ask(page, question);
  expect(asked[0]).toMatchObject({ question, new_conversation: true });
  // The conversation is named after the question and is in the list, under Today.
  await expect(page.getByRole("heading", { name: question })).toBeVisible();
  let list = await conversations(page, phone);
  const today = list.locator(".convo-group", { has: page.getByRole("heading", { name: "Today" }) });
  await expect(today.getByRole("button", { name: question })).toBeVisible();
  await expect(today.getByRole("button", { name: question })).toHaveAttribute("aria-current", "true");

  // A new conversation, then back to the saved one: it comes from the hub.
  await list.getByRole("button", { name: "New conversation" }).click();
  await expect(page.locator(".exchange")).toHaveCount(0);
  list = await conversations(page, phone);
  await list.getByRole("button", { name: question }).click();
  await expect(page.locator(".exchange .q")).toHaveText([question]);
  // A second question continues it: the hub takes the history from the saved conversation.
  await ask(page, `And the matches? ${stamp}`);
  expect(asked[1]).toMatchObject({ conversation: expect.any(String) });
  expect(asked[1].new_conversation).toBeUndefined();
  await page.reload();
  await expect(page.locator(".exchange .q"), "the open conversation is opened again").toHaveText([question, `And the matches? ${stamp}`]);

  // Rename it.
  await page.getByRole("button", { name: "Conversation options" }).click();
  await page.getByRole("button", { name: "Rename" }).click();
  await page.getByRole("textbox", { name: "Conversation name" }).fill(`Light ${stamp}`);
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByRole("heading", { name: `Light ${stamp}` })).toBeVisible();
  list = await conversations(page, phone);
  await expect(list.getByRole("button", { name: `Light ${stamp}` })).toBeVisible();

  // Search finds it by a word of its questions, with or without diacritics.
  const search = list.getByRole("searchbox", { name: "Search conversations" });
  await search.fill(`matches ${stamp}`);
  await expect(list.getByRole("button", { name: `Light ${stamp}` })).toBeVisible();
  await search.fill(`zzz-nothing-${stamp}`);
  await expect(list.getByText("No conversation matches.")).toBeVisible();
  await search.fill("");
  if (phone) await page.keyboard.press("Escape");
  await noHorizontalScroll(page);

  // Delete it: it needs a second, deliberate tap.
  const before = await (await page.request.get("/api/conversations")).json();
  const id = before.find((c: { title: string }) => c.title === `Light ${stamp}`).id;
  await page.getByRole("button", { name: "Conversation options" }).click();
  await page.getByRole("button", { name: "Delete conversation" }).click();
  await page.getByRole("button", { name: "Yes, delete" }).click();
  await expect(page.locator(".exchange")).toHaveCount(0);
  const saved = await (await page.request.get("/api/conversations")).json();
  expect(saved.some((c: { title: string }) => c.title === `Light ${stamp}`)).toBe(false);
  list = await conversations(page, phone);
  await expect(list.getByRole("button", { name: `Light ${stamp}` })).toHaveCount(0);

  // One remembered as open that is gone (deleted elsewhere, or another hub) quietly gives way to a new one.
  await page.evaluate((gone) => localStorage.setItem("zaklon.chat.open", gone), id);
  await page.reload();
  await expect(page.getByRole("textbox", { name: "Ask something" })).toBeVisible();
  await expect(page.locator(".exchange")).toHaveCount(0);
  await expect(page.locator(".chat-note")).toHaveCount(0);
  expect(await page.evaluate(() => localStorage.getItem("zaklon.chat.open"))).toBeNull();
});

test("conversations: a copy is sent to another device of the household (device list simulated)", async ({ page }, info) => {
  await ensureSetUp(page);
  await assistantReady(page);
  const stamp = `${info.project.name} ${Date.now() % 1_000_000}`;
  let devices = [{ id: "phone-ana", name: "Ana's phone", platform: "android", created_at: "2026-09-01T10:00:00Z", last_seen: null }];
  await page.route("**/api/devices", (route) => route.fulfill({ json: devices }));
  const sent: { url: string; body: unknown }[] = [];
  await page.route("**/api/conversations/*/send", (route) => {
    sent.push({ url: route.request().url(), body: route.request().postDataJSON() });
    return route.fulfill({ status: 204, body: "" });
  });

  await page.goto("/#assistant");
  await ask(page, `Water for ${stamp}`);
  await page.getByRole("button", { name: "Conversation options" }).click();
  await page.getByRole("button", { name: "Send to…" }).click();
  await expect(page.getByText("Send a copy to")).toBeVisible();
  await page.getByRole("button", { name: "Ana's phone" }).click();
  await expect(page.getByRole("status").filter({ hasText: "Sent to Ana's phone." })).toBeVisible();
  expect(sent).toHaveLength(1);
  expect(sent[0].body).toEqual({ to: "phone-ana" });
  const list = await (await page.request.get("/api/conversations")).json();
  const id = list.find((c: { title: string }) => c.title === `Water for ${stamp}`).id;
  expect(sent[0].url).toContain(`/api/conversations/${id}/send`);

  // With no phone paired there is nowhere to send it, and the screen says so.
  devices = [];
  await page.getByRole("button", { name: "Conversation options" }).click();
  await page.getByRole("button", { name: "Send to…" }).click();
  await expect(page.getByText("There is no other device yet. Pair a phone under Household.")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByText("Send a copy to")).toHaveCount(0);
  await page.request.delete(`/api/conversations/${id}`);
});

test("conversations: a copy from another device says who sent it, and its answers keep their sources", async ({ page }, info) => {
  await ensureSetUp(page);
  await assistantReady(page);
  const phone = info.project.name === "phone";
  const now = new Date().toISOString();
  const lastWeek = new Date(Date.now() - 3 * 86_400_000).toISOString();
  const summary = { id: "from-ana", title: "Boiling water", from_owner: "phone-ana", from_name: "Ana's phone", created_at: now, updated_at: now, turns: 1 };
  const older = { id: "older", title: "Beans", from_owner: null, from_name: null, created_at: lastWeek, updated_at: lastWeek, turns: 1 };
  await page.route("**/api/conversations", (route) => route.fulfill({ json: [summary, older] }));
  await page.route("**/api/conversations/from-ana", (route) =>
    route.fulfill({
      json: {
        ...summary,
        turns: [{
          id: "t1", question: "How long should water boil?", status: "done", error: null, answer_id: null, outcome: null, created_at: now, answered_at: now,
          answer: "Bring it to a **rolling boil** for one minute [1].",
          sources: [{ n: 1, title: "Water purification", url: "/kiwix/content/test/A/Water", book_title_en: "Wikipedia", book_title_sr: "Vikipedija" }],
          details: { grounded: true, cited: true, language: "en", tokens_per_second: 9.5, searched: ["boil water"] },
        }],
      },
    }),
  );
  await page.goto("/#assistant");
  const list = await conversations(page, phone);
  await expect(list.locator(".convo-group", { has: page.getByRole("heading", { name: "Previous 7 days" }) }).getByRole("button", { name: "Beans" })).toBeVisible();
  await list.getByRole("button", { name: /Boiling water, Sent from Ana's phone/ }).click();
  await expect(page.locator(".chat-head").getByText("Sent from Ana's phone")).toBeVisible();
  await expect(page.locator(".answer-text strong")).toHaveText("rolling boil");
  await expect(page.getByRole("button", { name: /Water purification · Wikipedia/ })).toBeVisible();
  await expect(page.getByText("Looked up: boil water")).toBeVisible();
  await noHorizontalScroll(page);
});
