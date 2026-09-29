import { expect, test, type Page } from "@playwright/test";
import { join } from "node:path";

// Settings › About › System specification against the real hub of the
// interface tests: its groups and rows, "Copy all" putting all of it on the
// clipboard as plain text, a value the hub is still reading arriving a
// moment later, a phone's own part (the phone app simulated), and no
// horizontal scroll on a phone. With ZAKLON_SHOTS_DIR set, screenshots of it
// too (SHOT_LANG=en|sr, SHOT_SIZE=1600x900 for the laptop window);
// ZAKLON_PRINT_SPEC=1 prints what "Copy all" copied.

const PASSWORD = "correct horse";
const GROUPS = {
  en: ["Application", "Built with", "Database", "AI assistant", "Library", "Maps", "Network"],
  sr: ["Aplikacija", "Napravljeno sa", "Baza podataka", "AI asistent", "Biblioteka", "Mape", "Mreža"],
};

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
  // Setup hashes the password with Argon2, slow on purpose; a busy machine needs longer.
  await expect(ready).toBeVisible({ timeout: 20_000 });
}

async function noHorizontalScroll(page: Page) {
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow, "page must not be wider than the screen").toBeLessThanOrEqual(1);
}

const spec = (page: Page) => page.locator("#set-system-spec");

/** The value on the row with this label. */
const value = (page: Page, label: string) =>
  spec(page).locator("tr").filter({ has: page.getByRole("rowheader", { name: label, exact: true }) }).locator("td");

/** Open the specification and wait until the hub has read everything (the network profile takes Windows a moment). */
async function openSpec(page: Page) {
  await page.goto("/#settings/about/system-spec");
  await expect(spec(page).locator("h3").first()).toBeVisible();
  await expect(spec(page).locator("td.pending")).toHaveCount(0, { timeout: 30_000 });
  await expect(spec(page).getByText(/Asking the hub|Pitam hub/)).toHaveCount(0);
}

/**
 * The phone app, simulated: the page believes it runs in Zaklon's Android
 * app (Tauri), paired with this hub, and its requests to the hub reach the
 * test hub as they are. Takes effect at the next load of the page.
 */
async function asPhoneApp(page: Page) {
  await page.addInitScript(() => {
    const link = { linked: true, hub_id: "e2e-hub", hub_name: "E2E hub", device_id: "e2e-phone", hosts: ["127.0.0.1"], port: 8484, last_host: "127.0.0.1" };
    const w = window as unknown as Record<string, unknown>;
    w.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
      transformCallback: (callback: unknown) => {
        const id = Math.floor(Math.random() * 1e9);
        w[`_${id}`] = callback;
        return id;
      },
      invoke: async (cmd: string, args: Record<string, unknown> = {}) => {
        switch (cmd) {
          case "app_mode":
            return { mode: "client", api_base: null, platform: "android", version: "0.1.0", debug: false };
          case "client_state":
            return link;
          case "client_request": {
            const body = typeof args.body === "string" ? args.body : undefined;
            const res = await fetch(String(args.path), { method: String(args.method), body, headers: body ? { "content-type": "application/json" } : undefined });
            return { status: res.status, body: await res.text() };
          }
          default:
            return null;
        }
      },
    };
  });
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

test("about: the system specification shows its groups, and Copy all puts all of it on the clipboard", async ({ page, context }) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await ensureSetUp(page);
  await openSpec(page);
  await expect(page.getByRole("heading", { name: "About", level: 1 })).toBeVisible();
  await expect(spec(page).getByRole("heading", { name: "System specification", level: 2 })).toBeVisible();
  // The groups, in order, each with a table of rows.
  await expect(spec(page).locator("h3")).toHaveText(GROUPS.en);
  await expect(spec(page).locator("table")).toHaveCount(GROUPS.en.length);

  // Real values from the hub and from the build (lock files, the compiler).
  await expect(value(page, "Zaklon version")).toHaveText(/^\d+\.\d+\.\d+$/);
  await expect(value(page, "Build")).toHaveText(/^\d{4}-\d{2}-\d{2}( · commit [0-9a-f]{7,12})?$/);
  await expect(value(page, "Mode")).toHaveText(/^(release|development)$/);
  await expect(value(page, "Hub (server)")).toHaveText(/^Rust \d+\.\d+\.\d+$/);
  await expect(value(page, "Interface")).toHaveText(/^TypeScript \d+\.\d+\.\d+ \+ React \d+\.\d+\.\d+$/);
  await expect(value(page, "Desktop and phone app")).toHaveText(/^Tauri 2\.\d+\.\d+$/);
  await expect(value(page, "Database")).toHaveText(/^SQLite 3\.\d+\.\d+$/);
  await expect(value(page, "AI engine")).toHaveText(/^llama\.cpp b\d+ \(C\+\+\)/);
  await expect(value(page, "Library engine")).toHaveText(/kiwix-serve \d+\.\d+\.\d+ \(C\+\+\)/);
  await expect(value(page, "Zaklon map")).toHaveText(/^MapLibre \d+\.\d+\.\d+ \+ Protomaps basemaps \d+\.\d+\.\d+$/);
  await expect(value(page, "Navigation (phones)")).toHaveText(/^CoMaps 20\d\d\.\d\d\.\d\d-\d+ \(a separate app\)$/);
  await expect(value(page, "Schema")).toContainText("migrations: batches_v1");
  await expect(value(page, "Last backup")).toHaveText(/\d|^none yet$/);
  await expect(value(page, "Memory (RAM)")).toHaveText(/^\d/);
  await expect(value(page, "CoMaps app")).toHaveText(/^20\d\d\.\d\d\.\d\d-\d+ · /);
  await expect(value(page, "Port")).toHaveText(/^\d+$/);
  await expect(value(page, "Phones paired")).toHaveText(/^\d+$/);
  await expect(value(page, "Certificate")).toHaveText(/^([0-9A-F]{4} ){4}…$/);

  // Copy all: the same rows as plain text, each group under its name in capitals.
  const shown = await spec(page)
    .locator("tr")
    .evaluateAll((rows) => rows.map((r) => `${r.querySelector("th")?.textContent}: ${r.querySelector("td")?.textContent}`));
  await spec(page).getByRole("button", { name: "Copy all" }).click();
  await expect(spec(page).getByRole("status")).toHaveText("Copied. Paste it into the bug report.");
  // Windows keeps text on the clipboard with CRLF line ends.
  const text = (await page.evaluate(() => navigator.clipboard.readText())).replace(/\r\n/g, "\n");
  if (process.env.ZAKLON_PRINT_SPEC) console.log(`\n${text}`);
  expect(text).toMatch(/^Zaklon system specification \(.+\)\n\nAPPLICATION\nZaklon version: \d+\.\d+\.\d+\n/);
  for (const group of GROUPS.en) expect(text).toContain(`\n\n${group.toUpperCase()}\n`);
  for (const line of shown) expect(text).toContain(`\n${line}\n`);
  expect(text.trim().split("\n").length).toBe(1 + GROUPS.en.length * 2 + shown.length);
  // Nothing secret: the password never, the certificate only by its start.
  expect(text).not.toContain(PASSWORD);
  const { fingerprint } = await (await page.request.get("/api/status")).json();
  expect(text.replace(/\s/g, "").toLowerCase()).not.toContain(fingerprint.slice(0, 20));
  await noHorizontalScroll(page);
});

test("about: in Serbian the specification and its copy are in Serbian", async ({ page, context }) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await ensureSetUp(page);
  await page.goto("/#settings/language");
  await page.getByRole("combobox", { name: /^(Language|Jezik)$/ }).selectOption("sr");
  await openSpec(page);
  await expect(spec(page).getByRole("heading", { name: "Specifikacija sistema", level: 2 })).toBeVisible();
  await expect(spec(page).locator("h3")).toHaveText(GROUPS.sr);
  await expect(value(page, "Verzija Zaklona")).toHaveText(/^\d+\.\d+\.\d+$/);
  await expect(value(page, "Navigacija (telefoni)")).toHaveText(/\(posebna aplikacija\)$/);
  await spec(page).getByRole("button", { name: "Kopiraj sve" }).click();
  await expect(spec(page).getByRole("status")).toHaveText("Kopirano. Nalepi to u prijavu greške.");
  // Windows keeps text on the clipboard with CRLF line ends.
  const text = (await page.evaluate(() => navigator.clipboard.readText())).replace(/\r\n/g, "\n");
  expect(text).toMatch(/^Specifikacija sistema Zaklon \(/);
  expect(text).toContain("\n\nBAZA PODATAKA\n");
  expect(text).toMatch(/\nVerzija Zaklona: \d+\.\d+\.\d+\n/);
  await noHorizontalScroll(page);
  await page.goto("/#settings/language");
  await page.getByRole("combobox", { name: /^(Language|Jezik)$/ }).selectOption("en");
});

test("about: a value the hub is still reading shows as such, then arrives (simulated)", async ({ page }) => {
  await ensureSetUp(page);
  let asked = 0;
  await page.route("**/api/system/spec", async (route) => {
    const json = await (await route.fetch()).json();
    asked += 1;
    json.app.os = "Windows 11 Pro 24H2";
    json.app.os_build = "26100.4061";
    json.app.webview2 = "131.0.2903.70";
    json.network.profiles = asked === 1 ? null : [{ adapter: "Wi-Fi", category: "Private" }];
    json.pending = asked === 1 ? ["network_profile"] : [];
    await route.fulfill({ json });
  });
  await page.goto("/#settings/about/system-spec");
  await expect(value(page, "Network profile")).toHaveText("reading…");
  await expect(value(page, "Network profile")).toHaveClass("pending");
  // The page asks again by itself.
  await expect(value(page, "Network profile")).toHaveText("Private (Wi-Fi)", { timeout: 10_000 });
  await expect(value(page, "Windows version")).toHaveText("Windows 11 Pro 24H2 (build 26100.4061)");
  await expect(value(page, "WebView2")).toHaveText("131.0.2903.70");
  expect(asked).toBeGreaterThanOrEqual(2);
});

test("about on a phone (the app simulated): its app, Android and WebView first, then the hub it is connected to", async ({ page }, info) => {
  test.skip(info.project.name !== "phone", "the phone's browser says it is Android");
  await ensureSetUp(page);
  await asPhoneApp(page);
  await page.goto("/#settings/about/system-spec");
  await page.reload();
  await expect(spec(page).locator("h3")).toHaveText(["Application", "Hub", ...GROUPS.en.slice(1)], { timeout: 15_000 });
  await expect(value(page, "Phone app version")).toHaveText("0.1.0");
  await expect(value(page, "Mode")).toHaveText("release");
  await expect(value(page, "Android version")).toHaveText(/^\d+(\.\d+)*$/);
  await expect(value(page, "WebView (Chrome)")).toHaveText(/^\d+\.\d+\.\d+\.\d+$/);
  await expect(value(page, "Connected hub")).toHaveText("E2E hub");
  await expect(value(page, "Last reached")).not.toHaveText("n/a");
  await expect(value(page, "Hub version")).toHaveText(/^\d+\.\d+\.\d+ · .+ · (release|development)$/);
  await expect(value(page, "Hub computer")).not.toHaveText("");
  // The laptop's own rows are not a phone's.
  await expect(spec(page).getByRole("rowheader", { name: "Zaklon version", exact: true })).toHaveCount(0);
  await noHorizontalScroll(page);
});

const SHOTS = process.env.ZAKLON_SHOTS_DIR;
const SHOT_LANG = process.env.SHOT_LANG === "sr" ? "sr" : "en";
const SHOT_SIZE = process.env.SHOT_SIZE?.match(/^(\d+)x(\d+)$/);

test("about: screenshots of the system specification", async ({ page }, info) => {
  test.skip(!SHOTS, "set ZAKLON_SHOTS_DIR to take screenshots");
  test.setTimeout(90_000);
  if (SHOT_SIZE && info.project.name === "laptop") await page.setViewportSize({ width: Number(SHOT_SIZE[1]), height: Number(SHOT_SIZE[2]) });
  const file = (name: string) => join(SHOTS ?? "", `${SHOT_LANG}-${info.project.name}-${name}.png`);
  await ensureSetUp(page);
  await page.addInitScript((l) => localStorage.setItem("zaklon.lang", l), SHOT_LANG);
  await page.reload();
  await openSpec(page);
  await page.waitForTimeout(500);
  await page.screenshot({ path: file("about-spec-page"), fullPage: true });
  await spec(page).scrollIntoViewIfNeeded();
  await page.screenshot({ path: file("about-spec"), fullPage: false });
  await spec(page).screenshot({ path: file("about-spec-table") });
  if (info.project.name === "phone") {
    await asPhoneApp(page);
    await page.reload();
    await expect(spec(page).locator("h3")).toHaveCount(GROUPS.en.length + 1, { timeout: 15_000 });
    await expect(spec(page).locator("td.pending")).toHaveCount(0, { timeout: 30_000 });
    await page.waitForTimeout(500);
    await spec(page).screenshot({ path: file("about-spec-phone-app") });
  }
});
