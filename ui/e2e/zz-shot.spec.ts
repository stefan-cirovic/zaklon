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

test("map shots", async ({ page }, info) => {
  // The Zaklon map, best with the real map assets (ZAKLON_MAP_ASSETS) and
  // the world map in the test hub's library.
  test.setTimeout(180000);
  if (SIZE && info.project.name === "laptop") await page.setViewportSize({ width: Number(SIZE[1]), height: Number(SIZE[2]) });
  const file = (name: string) => join(DIR ?? "", `${LANG}-${info.project.name}-${name}.png`);
  await page.addInitScript((l) => localStorage.setItem("zaklon.lang", l), LANG);
  await page.goto("/#settings");
  const setup = page.getByText(/Set up your household|Podesi domaćinstvo/);
  if (await setup.isVisible({ timeout: 3000 }).catch(() => false)) {
    await page.locator('input[type="password"]').nth(0).fill("correct horse");
    await page.locator('input[type="password"]').nth(1).fill("correct horse");
    await page.locator("form button.btn").click();
    await page.waitForTimeout(800);
  }
  const settle = (ms = 5000) => page.waitForTimeout(ms);
  await page.request.delete("/api/home-location");
  await page.goto("/#home");
  await settle();
  await page.screenshot({ path: file("map-home-nohome"), fullPage: true });
  await page.goto("/#maps");
  await settle();
  await page.screenshot({ path: file("map-world"), fullPage: false });
  await page.goto("/#maps/home");
  await page.locator(".home-loc input[type=search]").fill("novi sad");
  await page.waitForTimeout(1200);
  await page.screenshot({ path: file("map-find-place"), fullPage: false });
  await page.locator(".home-loc-result").first().click();
  await settle();
  await page.screenshot({ path: file("map-chosen"), fullPage: false });
  await page.locator(".home-loc .btn").first().click();
  await page.waitForTimeout(1000);
  await page.goto("/#home");
  await settle(7000);
  await page.screenshot({ path: file("map-home"), fullPage: true });
  await page.goto("/#maps");
  await settle(7000);
  await page.screenshot({ path: file("map-maps-home"), fullPage: false });
  // Closer in, and farther out.
  for (const [name, clicks, button] of [["map-streets", 4, "zmap-btn:nth-child(1)"], ["map-region", 7, "zmap-btn:nth-child(2)"]] as const) {
    for (let i = 0; i < clicks; i++) {
      await page.locator(`.map-part .${button}`).click();
      await page.waitForTimeout(400);
    }
    await settle(6000);
    await page.screenshot({ path: file(name), fullPage: false });
  }
  await page.goto("/#maps/navigation");
  await page.waitForTimeout(1500);
  await page.screenshot({ path: file("map-navigation"), fullPage: true });
});

test("shots", async ({ page }, info) => {
  test.setTimeout(120000);
  if (SIZE && info.project.name === "laptop") await page.setViewportSize({ width: Number(SIZE[1]), height: Number(SIZE[2]) });
  const file = (name: string) => join(DIR ?? "", `${LANG}-${info.project.name}-${name}.png`);
  await page.addInitScript((l) => localStorage.setItem("zaklon.lang", l), LANG);
  await page.goto("/#settings");
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
    ["water", "/#tools/water?people=2&children=2&smallpets=1&days=7"],
    ["water-drip", "/#tools/water?part=drip&beds=4x1.2:tomatoes,3x1:greens,6:potatoes&lat=44.8&month=7&tmax=29&tmin=17&rain=60&roof=80"],
    ["assistant", "/#assistant"], ["addons", "/#addons"], ["settings", "/#settings"], ["settings-help", "/#help"],
    ...["devices", "network", "backups", "privacy", "appearance", "language", "assistant", "updates", "about"].map(
      (c) => [`settings-${c}`, `/#settings/${c}`] as [string, string],
    ),
  ];
  for (const [name, url] of shots) {
    await page.goto(url);
    await page.waitForTimeout(1200);
    await page.screenshot({ path: file(name), fullPage: true });
  }
  // Settings' search, and a setting it opened.
  await page.goto("/#settings");
  await page.locator(".set-search input").fill(LANG === "sr" ? "lozinka" : "password");
  await page.waitForTimeout(400);
  await page.screenshot({ path: file("settings-search"), fullPage: false });
  await page.keyboard.press("Enter");
  await page.waitForTimeout(600);
  await page.screenshot({ path: file("settings-search-opened"), fullPage: false });
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
    // A pack the hub has that the catalog no longer offers.
    json.packs.push({
      id: "zimgit-medicine-en",
      title: { en: "First aid and medicine guides (English)", sr: "Vodiči za prvu pomoć i medicinu (engleski)" },
      description: { en: "Field manuals on first aid and medical care when no doctor is available.", sr: "Priručnici za prvu pomoć i lečenje kad lekar nije dostupan." },
      category: "knowledge", topics: ["health"], version: "2024-08", size: 70179585, license: "Various; see each document",
      attribution: "Various authors, collected by Kiwix", source: "https://library.kiwix.org", offer: "auto", languages: ["eng"], recommended_for: [],
      withdrawn: true, state: { status: "installed", bytes_done: 70179585, bytes_total: 70179585, speed: 0 },
    });
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
    ["addons-x-build", "/#addons/build", "tiles"],
    ["addons-x-water", "/#addons/water", "tiles"],
    ["addons-x-details", "/#addons", "details"],
    ["addons-x-knowledge-details", "/#addons/knowledge", "details"],
    ["addons-x-health-details", "/#addons/health", "details"],
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
  // The packs people download themselves, and the question before one downloads
  // (nothing is asked of the hub: the question is only opened).
  for (const [name, url, view] of [
    ["addons-x-health", "/#addons/health", "tiles"],
    ["addons-x-food", "/#addons/food", "tiles"],
    ["addons-x-build-details", "/#addons/build", "details"],
  ]) {
    await page.evaluate((v) => localStorage.setItem("zaklon.addonsView", v), view);
    await page.goto(url);
    await page.reload();
    await page.waitForTimeout(1200);
    await page.screenshot({ path: file(name), fullPage: true });
  }
  await page.evaluate(() => localStorage.setItem("zaklon.addonsView", "tiles"));
  await page.goto("/#addons/food");
  await page.reload();
  await page.waitForTimeout(1200);
  const grim = page.locator('[data-entry="grimgrains-en"]');
  await grim.getByRole("button").first().click();
  await page.waitForTimeout(400);
  await grim.scrollIntoViewIfNeeded();
  await page.screenshot({ path: file("addons-x-food-ask"), fullPage: false });
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

// Only Help (run alone with -g "help shots"): its home, a search, topics, and the links to it.
test("help shots", async ({ page }, info) => {
  test.setTimeout(60000);
  if (SIZE && info.project.name === "laptop") await page.setViewportSize({ width: Number(SIZE[1]), height: Number(SIZE[2]) });
  const file = (name: string) => join(DIR ?? "", `${LANG}-${info.project.name}-${name}.png`);
  await page.addInitScript((l) => localStorage.setItem("zaklon.lang", l), LANG);
  await page.goto("/#settings");
  const setup = page.getByText(/Set up your household|Podesi domaćinstvo/);
  if (await setup.isVisible({ timeout: 3000 }).catch(() => false)) {
    await page.locator('input[type="password"]').nth(0).fill("correct horse");
    await page.locator('input[type="password"]').nth(1).fill("correct horse");
    await page.locator("form button.btn").click();
    await page.waitForTimeout(800);
  }
  for (const [name, url, full] of [
    ["help-tools", "/#tools", true],
    ["help-settings", "/#settings", false],
    ["help-home", "/#help", true],
    ["help-start", "/#help/start", true],
    ["help-pairing", "/#help/pairing", true],
    ["help-assistant", "/#help/assistant", true],
    ["help-supplies", "/#help/supplies", true],
    ["help-settings-backups", "/#help/settings/backups", false],
    ["help-troubleshooting", "/#help/troubleshooting", false],
    ["help-link-supplies", "/#supplies", false],
    ["help-link-settings", "/#settings/network", false],
    ["help-link-assistant", "/#assistant", false],
  ] as const) {
    await page.goto(url);
    await page.waitForTimeout(900);
    await page.screenshot({ path: file(name), fullPage: full });
  }
  await page.goto("/#help");
  await page.locator(".help-search input").fill(LANG === "sr" ? "lozinka" : "password");
  await page.waitForTimeout(400);
  await page.screenshot({ path: file("help-search"), fullPage: true });
});

// Only Home (run alone with -g "home shots"): with supplies and conversations, then with
// downloads under way, a new version of a pack and the warnings (states made up for the picture).
test("home shots", async ({ page }, info) => {
  test.setTimeout(60000);
  if (SIZE && info.project.name === "laptop") await page.setViewportSize({ width: Number(SIZE[1]), height: Number(SIZE[2]) });
  const file = (name: string) => join(DIR ?? "", `${LANG}-${info.project.name}-${name}.png`);
  const sr = LANG === "sr";
  const day = (n: number) => new Date(Date.now() + n * 86_400_000).toISOString().slice(0, 10);
  const ago = (days: number) => new Date(Date.now() - days * 86_400_000).toISOString();
  await page.addInitScript((l) => localStorage.setItem("zaklon.lang", l), LANG);
  await page.goto("/#settings");
  const setup = page.getByText(/Set up your household|Podesi domaćinstvo/);
  if (await setup.isVisible({ timeout: 3000 }).catch(() => false)) {
    await page.locator('input[type="password"]').nth(0).fill("correct horse");
    await page.locator('input[type="password"]').nth(1).fill("correct horse");
    await page.locator("form button.btn").click();
    await page.waitForTimeout(800);
    for (const it of [
      { name: sr ? "Mleko" : "Milk", quantity: 1, unit: "l", category: "drink", expiry: day(1), min_quantity: 2 },
      { name: sr ? "Jogurt" : "Yogurt", quantity: 2, unit: "pcs", category: "food", expiry: day(3) },
      { name: sr ? "Tunjevina u konzervi" : "Canned tuna", quantity: 6, unit: "pcs", category: "food", expiry: day(20) },
      { name: sr ? "Pasulj" : "Beans", quantity: 4, unit: "pcs", category: "food", expiry: day(-8) },
      { name: sr ? "Brašno tip 500" : "Flour", quantity: 5, unit: "kg", category: "food", expiry: day(150) },
      { name: sr ? "Pirinač" : "Rice", quantity: 1, unit: "kg", category: "food", min_quantity: 3 },
      { name: sr ? "Baterije AA" : "AA batteries", quantity: 4, unit: "pcs", category: "equipment", min_quantity: 8 },
      { name: "Paracetamol 500 mg", quantity: 2, unit: "pack", category: "medicine", expiry: day(400) },
    ]) await page.request.post("/api/items", { data: it });
    await page.request.post("/api/shopping", { data: { text: sr ? "Šećer" : "Sugar", quantity: 1, unit: "kg" } });
    await page.request.post("/api/shopping", { data: { text: sr ? "Šibice" : "Matches", quantity: 2, unit: "pack" } });
  }
  // This device's conversations (the test hub has no AI, so they are simulated).
  const conv = (id: string, en: string, srTitle: string, days: number, from?: [string, string]) => ({
    id, title: sr ? srTitle : en, from_owner: from?.[0] ?? null, from_name: from?.[1] ?? null, created_at: ago(days), updated_at: ago(days), turns: 2,
  });
  await page.route("**/api/conversations", (route) =>
    route.fulfill({
      json: [
        conv("c1", "How do I purify water without a filter?", "Kako da prečistim vodu bez filtera?", 0.02),
        conv("c2", "First aid for a burn", "Prva pomoć kod opekotine", 1, ["p1", sr ? "Anin telefon" : "Ana's phone"]),
        conv("c3", "How long do dry beans keep?", "Koliko dugo traje suvi pasulj?", 3),
        conv("c4", "Bread without yeast", "Hleb bez kvasca", 20),
      ],
    }),
  );
  // The usual day: phones can connect (whatever this machine's firewall says).
  let firewall = { checked: true, firewall_on: true, allowed: true, blocked: false, public_network: false, error: null, ok: true };
  await page.route("**/api/firewall", (r) => r.fulfill({ json: firewall }));
  await page.goto("/#home");
  await page.reload();
  await page.waitForTimeout(1500);
  await page.screenshot({ path: file("home"), fullPage: true });
  // What the window shows (a full-page picture draws the bar, which stays at the bottom, in the middle).
  await page.screenshot({ path: file("home-screen"), fullPage: false });
  // The supplies card up close.
  await page.locator(".home-supplies").screenshot({ path: file("home-supplies") });

  // Downloads under way, a new version and a paused download; a newer Zaklon; the firewall (laptop).
  await page.route("**/api/catalog", async (r) => {
    const json = await (await r.fetch()).json();
    for (const p of json.packs) {
      if (["wikipedia-sr-maxi", "wikimed-en", "llama-cpp", "qwen35-4b"].includes(p.id)) p.state = { ...p.state, status: "installed", bytes_done: p.size, bytes_total: p.size };
      if (p.id === "kiwix-tools") p.state = { ...p.state, status: "installed", bytes_done: p.size, bytes_total: p.size, update_available: true };
      if (p.id === "ifixit-en") p.state = { ...p.state, status: "downloading", bytes_done: Math.round(p.size * 0.45), bytes_total: p.size, speed: 3_400_000 };
      if (p.id === "qwen35-9b") p.state = { ...p.state, status: "paused", bytes_done: Math.round(p.size * 0.2), bytes_total: p.size };
    }
    return r.fulfill({ json });
  });
  await page.route("**/api/maps", async (r) => {
    const json = await (await r.fetch()).json();
    const serbia = json.countries.find((c: { id: string }) => c.id === "Serbia");
    serbia.regions.forEach((reg: { status: string; bytes_done: number; size: number }, i: number) => {
      reg.status = i === 0 ? "downloading" : "installed";
      reg.bytes_done = i === 0 ? Math.round(reg.size * 0.6) : reg.size;
    });
    return r.fulfill({ json });
  });
  await page.route("**/api/updates", (route) =>
    route.fulfill({ json: { enabled: true, current: "0.1.0", latest: "0.2.0", newer: true, url: "https://github.com/stefan-cirovic/zaklon/releases/tag/v0.2.0", checked_at: ago(0), error: null } }),
  );
  firewall = { ...firewall, allowed: false, blocked: true, ok: false };
  await page.reload();
  await page.waitForTimeout(1500);
  await page.screenshot({ path: file("home-busy"), fullPage: true });
  await page.screenshot({ path: file("home-busy-screen"), fullPage: false });
  // A home Wi-Fi that Windows treats as public (the button is never pressed here), and supplies with nothing to do.
  firewall = { ...firewall, allowed: true, blocked: false, public_network: true, ok: false };
  await page.route("**/api/supplies/summary", (r) => r.fulfill({ json: { total_items: 8, expired: [], expiring_soon: [], running_low: [], to_put_away: 0 } }));
  await page.route("**/api/shopping", (r) => r.fulfill({ json: [] }));
  await page.reload();
  await page.waitForTimeout(1500);
  await page.screenshot({ path: file("home-public-allgood"), fullPage: false });
  await page.locator(".home-supplies").screenshot({ path: file("home-supplies-empty") });
  await page.goto("/#settings/network");
  await page.waitForTimeout(1200);
  await page.screenshot({ path: file("settings-network-public"), fullPage: false });
  // Home asks every few seconds while something downloads: stop the made-up answers before the page closes.
  await page.unrouteAll({ behavior: "ignoreErrors" });
});

// Only the power calculator (run alone with -g "power shots"): empty, with a household's list,
// its formulas open, a list opened from a link, and a system too big for 12 V.
test("power shots", async ({ page }, info) => {
  test.setTimeout(60000);
  if (SIZE && info.project.name === "laptop") await page.setViewportSize({ width: Number(SIZE[1]), height: Number(SIZE[2]) });
  const file = (name: string) => join(DIR ?? "", `${LANG}-${info.project.name}-${name}.png`);
  await page.addInitScript((l) => localStorage.setItem("zaklon.lang", l), LANG);
  await page.goto("/#settings");
  const setup = page.getByText(/Set up your household|Podesi domaćinstvo/);
  if (await setup.isVisible({ timeout: 3000 }).catch(() => false)) {
    await page.locator('input[type="password"]').nth(0).fill("correct horse");
    await page.locator('input[type="password"]').nth(1).fill("correct horse");
    await page.locator("form button.btn").click();
    await page.waitForTimeout(800);
  }
  const line = (k: string, id: string, qty: number, watts: number, hours: number, whDay?: number) => ({ k, id, qty, watts, hours, ...(whDay ? { whDay } : {}) });
  await page.request.put("/api/power", { data: { plan: null } });
  await page.goto("/#power");
  await page.waitForTimeout(1000);
  await page.screenshot({ path: file("power-empty"), fullPage: true });
  const plan = {
    v: 1,
    lines: [
      line("a", "fridge", 1, 200, 24, 1200),
      line("b", "lights-led", 6, 10, 5),
      line("c", "phone", 3, 10, 2),
      line("d", "router", 1, 12, 24),
      line("e", "laptop", 1, 50, 4),
      line("f", "cpap", 1, 25, 8),
      { ...line("g", "custom", 1, 8, 24), name: LANG === "sr" ? "Pumpa za akvarijum" : "Aquarium pump" },
    ],
    days: 3,
    battery: "lifepo4",
    volts: 12,
    region: "belgrade",
    month: 12,
  };
  await page.request.put("/api/power", { data: { plan } });
  await page.goto("/#home");
  await page.goto("/#power");
  await page.waitForTimeout(1200);
  await page.screenshot({ path: file("power"), fullPage: true });
  await page.screenshot({ path: file("power-screen"), fullPage: false });
  await page.locator(".pw-how summary").click();
  await page.waitForTimeout(300);
  await page.locator(".pw-results").screenshot({ path: file("power-results-how") });
  // From a link, with loads too big for 12 V.
  await page.goto("/#power?items=fridge:1,pump:1,microwave:1,tv:1&days=1");
  await page.waitForTimeout(1000);
  await page.screenshot({ path: file("power-link"), fullPage: true });
  await page.request.put("/api/power", { data: { plan: null } });
});

test("world map shots", async ({ page }, info) => {
  // The world map in Add-ons › Maps, with the hub's answers simulated: the
  // build offered, then an older build on the hub with a newer one offered.
  test.setTimeout(60000);
  if (SIZE && info.project.name === "laptop") await page.setViewportSize({ width: Number(SIZE[1]), height: Number(SIZE[2]) });
  const file = (name: string) => join(DIR ?? "", `${LANG}-${info.project.name}-${name}.png`);
  await page.addInitScript((l) => localStorage.setItem("zaklon.lang", l), LANG);
  await page.goto("/#settings");
  const setup = page.getByText(/Set up your household|Podesi domaćinstvo/);
  if (await setup.isVisible({ timeout: 3000 }).catch(() => false)) {
    await page.locator('input[type="password"]').nth(0).fill("correct horse");
    await page.locator('input[type="password"]').nth(1).fill("correct horse");
    await page.locator("form button.btn").click();
    await page.waitForTimeout(800);
  }
  const GB = 1024 ** 3;
  let world: Record<string, unknown> = { offered: "20260811", offered_size: 137295889397, installed: null, installed_size: 0, update: false, needed: 128 * GB, disk_free: 412 * GB, room_for_both: true, listed: true };
  let state: Record<string, unknown> = { status: "not_installed", bytes_done: 0, bytes_total: 137295889397, speed: 0 };
  await page.route("**/api/world-map/check", (r) => r.fulfill({ json: { checking: false } }));
  await page.route("**/api/catalog", async (r) => {
    const json = await (await r.fetch()).json();
    const pack = json.packs.find((p: { id: string }) => p.id === "world-map");
    Object.assign(pack, { version: world.offered, size: world.offered_size, state });
    json.world = world;
    return r.fulfill({ json });
  });
  const entry = page.locator('[data-entry="world-map"]');
  // The world map's entry in the middle of the screen, and the entry alone.
  const shoot = async (name: string) => {
    await entry.evaluate((el) => el.scrollIntoView({ block: "center" }));
    await page.waitForTimeout(500);
    await page.screenshot({ path: file(name), fullPage: false });
    await entry.screenshot({ path: file(`${name}-entry`) });
  };
  await page.goto("/#addons/maps");
  await entry.waitFor();
  await shoot("addons-world-map");
  world = { ...world, offered: "20261019", offered_size: 129 * GB, installed: "20260811", installed_size: 137295889397, update: true, needed: 130 * GB };
  state = { status: "installed", bytes_done: 137295889397, bytes_total: 137295889397, speed: 0, update_available: true };
  await page.reload();
  await entry.waitFor();
  await shoot("addons-world-map-update");
  // Without room for both maps: the question before the old one is removed.
  world = { ...world, disk_free: 96 * GB, room_for_both: false };
  await page.reload();
  await entry.locator("[data-asks-license]").click();
  await shoot("addons-world-map-noroom");
  await page.unrouteAll({ behavior: "ignoreErrors" });
});
