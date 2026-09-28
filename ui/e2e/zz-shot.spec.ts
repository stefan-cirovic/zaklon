import { test } from "@playwright/test";
import { join } from "node:path";

// Screenshots of every screen, to look at by eye; not a check. Runs only when
// ZAKLON_SHOTS_DIR names the folder to put them in (SHOT_LANG=en|sr, default sr;
// SHOT_SIZE=1600x900 sets the laptop window, default the project's):
//   ZAKLON_SHOTS_DIR=C:/temp/shots pnpm e2e zz-shot
const DIR = process.env.ZAKLON_SHOTS_DIR;
const LANG = process.env.SHOT_LANG ?? "sr";
const SIZE = process.env.SHOT_SIZE?.match(/^(\d+)x(\d+)$/);
test.skip(!DIR, "set ZAKLON_SHOTS_DIR to take screenshots");

test("shots", async ({ page }, info) => {
  test.setTimeout(120000);
  if (SIZE && info.project.name === "laptop") await page.setViewportSize({ width: Number(SIZE[1]), height: Number(SIZE[2]) });
  const file = (name: string) => join(DIR ?? "", `${LANG}-${info.project.name}-${name}.png`);
  await page.addInitScript((l) => localStorage.setItem("zaklon.lang", l), LANG);
  await page.goto("/#household");
  const setup = page.getByText(/Set up your household|Podesi domaćinstvo/);
  if (await setup.isVisible({ timeout: 3000 }).catch(() => false)) {
    await page.locator('input[type="password"]').nth(0).fill("correct horse");
    await page.locator('input[type="password"]').nth(1).fill("correct horse");
    await page.locator("form button.btn").click();
    await page.waitForTimeout(800);
    for (const it of [
      { name: "Mleko", quantity: 1, unit: "l", category: "drink", expiry: "2026-09-29", min_quantity: 2 },
      { name: "Brašno tip 500", quantity: 5, unit: "kg", category: "food", expiry: "2027-03-01", place: "Ostava" },
      { name: "Pasulj", quantity: 4, unit: "pcs", category: "food", expiry: "2026-09-20" },
      { name: "Paracetamol 500 mg", quantity: 2, unit: "pack", category: "medicine", expiry: "2027-12-01" },
    ]) await page.request.post("/api/items", { data: it });
    await page.request.post("/api/shopping", { data: { text: "Šećer", quantity: 1, unit: "kg" } });
  }
  const shots: [string, string][] = [
    ["home", "/#home"], ["tools", "/#tools"], ["library", "/#library"], ["maps", "/#maps"], ["supplies", "/#supplies"],
    ["assistant", "/#assistant"], ["addons", "/#addons"], ["household", "/#household"],
    ...["devices", "network", "backups", "privacy", "appearance", "language", "assistant", "updates", "about"].map(
      (c) => [`household-${c}`, `/#household/${c}`] as [string, string],
    ),
  ];
  for (const [name, url] of shots) {
    await page.goto(url);
    await page.waitForTimeout(1200);
    await page.screenshot({ path: file(name), fullPage: true });
  }
  // Household's search, and a setting it opened.
  await page.goto("/#household");
  await page.locator(".set-search input").fill(LANG === "sr" ? "lozinka" : "password");
  await page.waitForTimeout(400);
  await page.screenshot({ path: file("household-search"), fullPage: false });
  await page.keyboard.press("Enter");
  await page.waitForTimeout(600);
  await page.screenshot({ path: file("household-search-opened"), fullPage: false });
  // Add-ons as a file explorer, with some packs on the hub, one on its way and one
  // that failed (states made up for the picture; nothing is downloaded).
  await page.route("**/api/catalog", async (r) => {
    const json = await (await r.fetch()).json();
    for (const p of json.packs) {
      if (["wikipedia-sr-maxi", "wikimed-en", "kiwix-tools", "llama-cpp", "qwen35-4b"].includes(p.id)) p.state = { ...p.state, status: "installed", bytes_done: p.size, bytes_total: p.size };
      if (p.id === "ifixit-en") p.state = { ...p.state, status: "downloading", bytes_done: Math.round(p.size * 0.45), bytes_total: p.size, speed: 3_400_000 };
      if (p.id === "zimgit-water-en") p.state = { ...p.state, status: "failed", error: "checksum mismatch" };
      if (p.id === "qwen35-9b") p.state = { ...p.state, status: "paused", bytes_done: Math.round(p.size * 0.2), bytes_total: p.size };
    }
    return r.fulfill({ json });
  });
  await page.route("**/api/maps", async (r) => {
    const json = await (await r.fetch()).json();
    const serbia = json.countries.find((c: { id: string }) => c.id === "Serbia");
    for (const reg of serbia.regions) reg.status = "installed";
    json.installed_bytes = serbia.size;
    return r.fulfill({ json });
  });
  for (const [name, url, view] of [
    ["addons-x", "/#addons", "tiles"],
    ["addons-x-models", "/#addons/models", "tiles"],
    ["addons-x-skills", "/#addons/skills", "tiles"],
    ["addons-x-details", "/#addons", "details"],
    ["addons-x-reference-details", "/#addons/reference", "details"],
    ["addons-x-maps-details", "/#addons/maps", "details"],
    ["addons-x-library", "/#addons/library", "details"],
  ]) {
    await page.evaluate((v) => localStorage.setItem("zaklon.addonsView", v), view);
    await page.goto(url);
    await page.reload();
    await page.waitForTimeout(1200);
    await page.screenshot({ path: file(name), fullPage: name !== "addons-x-maps-details" });
  }
  await page.getByRole("searchbox").fill("wiki");
  await page.waitForTimeout(400);
  await page.screenshot({ path: file("addons-x-search"), fullPage: true });
  await page.getByRole("searchbox").fill("");
  if (info.project.name === "laptop") {
    // A USB drive, when the machine has another drive.
    await page.goto("/#addons");
    await page.reload();
    await page.waitForTimeout(1200);
    const other = page.locator(".drive-grid button.drive-tile").nth(1);
    if (await other.count()) {
      await other.click();
      await page.waitForTimeout(800);
      await page.screenshot({ path: file("addons-x-drive"), fullPage: true });
    }
  }
  await page.evaluate(() => localStorage.setItem("zaklon.addonsView", "tiles"));
  await page.unroute("**/api/catalog");
  await page.unroute("**/api/maps");

  // The bar with a pinned tool, and the logo while pointed at (laptop).
  await page.request.post("/api/pinned-tool", { data: { tool: "supplies" } });
  await page.goto("/#tools");
  await page.reload();
  await page.waitForTimeout(1200);
  if (info.project.name === "laptop") await page.locator(".nav-brand").hover();
  await page.waitForTimeout(400);
  await page.screenshot({ path: file("tools-pinned"), fullPage: false });
  await page.request.post("/api/pinned-tool", { data: { tool: null } });
  await page.goto("/#supplies");
  await page.waitForTimeout(800);
  for (const [i, n] of [[1, "history"], [2, "shopping"], [3, "putaway"]] as const) {
    await page.locator(".segmented.tabs-4 button").nth(i).click();
    await page.waitForTimeout(600);
    await page.screenshot({ path: file(`supplies-${n}`), fullPage: true });
  }
  await page.locator(".segmented.tabs-4 button").nth(0).click();
  await page.locator(".item").first().click();
  await page.waitForTimeout(600);
  await page.screenshot({ path: file("supplies-edit"), fullPage: true });

  // The assistant with saved conversations. The test hub has no AI, so the
  // answers and the list are served by the test.
  const phone = info.project.name === "phone";
  const sr = LANG === "sr";
  const ago = (days: number) => new Date(Date.now() - days * 86_400_000).toISOString();
  const conv = (id: string, en: string, srTitle: string, days: number, from?: [string, string]) => ({
    id, title: sr ? srTitle : en, from_owner: from?.[0] ?? null, from_name: from?.[1] ?? null, created_at: ago(days), updated_at: ago(days), turns: 2,
  });
  const list = [
    conv("c1", "How do I purify water without a filter?", "Kako da prečistim vodu bez filtera?", 0),
    conv("c2", "How long do dry beans keep?", "Koliko dugo traje suvi pasulj?", 0.05),
    conv("c3", "First aid for a burn", "Prva pomoć kod opekotine", 1, ["p1", sr ? "Anin telefon" : "Ana's phone"]),
    conv("c4", "Add 2 liters of milk", "Dodaj 2 litra mleka", 3),
    conv("c5", "Bread without yeast", "Hleb bez kvasca", 20),
  ];
  const source = (n: number, title: string) => ({ n, title, url: `/kiwix/content/test/A/${n}`, book_title_en: "Wikipedia", book_title_sr: "Vikipedija" });
  const turn = (q: string, a: string) => ({
    id: q, question: q, answer: a, status: "done", error: null, answer_id: null, outcome: null, created_at: ago(0), answered_at: ago(0),
    sources: [source(1, sr ? "Prečišćavanje vode" : "Water purification"), source(2, sr ? "Ključanje" : "Boiling")],
    details: { grounded: true, cited: true, language: LANG, tokens_per_second: 11.4, searched: [sr ? "prečišćavanje vode" : "water purification"] },
  });
  const turns = sr
    ? [
        turn("Kako da prečistim vodu bez filtera?", "Najsigurnije je da je **prokuvaš**: neka ključa bar jedan minut, a na planini tri [1].\n\nAko ne možeš da je prokuvaš:\n* ostavi je da se slegne, pa je procedi kroz čistu tkaninu\n* dodaj dve kapi varikine bez mirisa na litar i sačekaj 30 minuta [2]"),
        turn("A ako imam samo jedan lonac?", "Dovoljan je jedan lonac: prokuvaj vodu, ostavi je poklopljenu da se ohladi i tek onda je presipaj u čiste flaše [1]."),
      ]
    : [
        turn("How do I purify water without a filter?", "The safest way is to **boil** it: keep it at a rolling boil for one minute, three at high altitude [1].\n\nIf you cannot boil it:\n* let it settle, then pour it through a clean cloth\n* add two drops of unscented bleach per liter and wait 30 minutes [2]"),
        turn("And if I only have one pot?", "One pot is enough: boil the water, let it cool with the lid on, and only then pour it into clean bottles [1]."),
      ];
  await page.route("**/api/assistant", (route) =>
    route.fulfill({
      json: {
        engine: "ready", engine_installed: true, selected: "qwen35-4b", recommended: "qwen35-4b", ram_total: 16e9, books: 3,
        models: [{ id: "qwen35-4b", title_en: "AI model (Qwen3.5 4B)", title_sr: "AI model (Qwen3.5 4B)", size: 3e9, installed: true, recommended: true }],
      },
    }),
  );
  await page.route("**/api/conversations", (route) => route.fulfill({ json: list }));
  await page.route("**/api/conversations/c1", (route) => route.fulfill({ json: { ...list[0], turns } }));
  await page.route("**/api/devices", (route) =>
    route.fulfill({ json: [{ id: "p1", name: sr ? "Anin telefon" : "Ana's phone", platform: "android", created_at: ago(30), last_seen: ago(0) }, { id: "p2", name: sr ? "Markov telefon" : "Marko's phone", platform: "android", created_at: ago(30), last_seen: null }] }),
  );
  await page.addInitScript(() => localStorage.removeItem("zaklon.chat.open"));
  await page.goto("/#assistant");
  await page.waitForTimeout(1000);
  await page.screenshot({ path: file("assistant-new"), fullPage: false });
  if (phone) {
    await page.locator(".chat-top .ghost-icon").first().click();
    await page.waitForTimeout(500);
    await page.screenshot({ path: file("assistant-list"), fullPage: false });
  }
  await page.locator(".convo-item").first().click();
  await page.waitForTimeout(800);
  await page.screenshot({ path: file("assistant-chat"), fullPage: false });
  await page.locator(".menu-wrap .ghost-icon").click();
  await page.waitForTimeout(300);
  await page.screenshot({ path: file("assistant-menu"), fullPage: false });
  await page.locator(".menu > button").nth(1).click();
  await page.waitForTimeout(600);
  await page.screenshot({ path: file("assistant-send"), fullPage: false });
  await page.keyboard.press("Escape");
  if (phone) await page.locator(".chat-top .ghost-icon").first().click();
  await page.locator(".convo-memory").click();
  await page.waitForTimeout(600);
  await page.screenshot({ path: file("assistant-memory"), fullPage: false });
});
