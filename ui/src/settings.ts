import { makeT, type Key } from "./i18n";

/**
 * Household is laid out like Windows Settings: a tile for each category, and
 * each category on its own page at #household/<category>. A single setting
 * has an address too, #household/<category>/<setting>: the page opens at it.
 * That is where the search sends you. A new panel in Household gets an entry
 * in SETTINGS, and an element with the id "set-<setting id>" on its page.
 */
export type CategoryId = "devices" | "network" | "backups" | "privacy" | "appearance" | "language" | "assistant" | "updates" | "about";

export type Category = {
  id: CategoryId;
  title: Key;
  desc: Key;
  /** What the tile says on a phone, when it differs. */
  descPhone?: Key;
  /** Only on the laptop (the hub itself). */
  hubOnly?: boolean;
};

/** In the order of the tiles. */
export const CATEGORIES: readonly Category[] = [
  { id: "devices", title: "devices", desc: "catDevicesDesc", descPhone: "catDevicesDescPhone" },
  { id: "network", title: "catNetwork", desc: "catNetworkDesc", hubOnly: true },
  { id: "backups", title: "backups", desc: "catBackupsDesc", hubOnly: true },
  { id: "privacy", title: "catPrivacy", desc: "catPrivacyDesc", descPhone: "catPrivacyDescPhone" },
  { id: "appearance", title: "appearance", desc: "catAppearanceDesc" },
  { id: "language", title: "language", desc: "catLanguageDesc" },
  { id: "assistant", title: "catAssistant", desc: "catAssistantDesc" },
  { id: "updates", title: "catUpdates", desc: "catUpdatesDesc" },
  { id: "about", title: "about", desc: "catAboutDesc", descPhone: "catAboutDescPhone" },
];

export type Setting = {
  id: string;
  cat: CategoryId;
  title: Key;
  /** More words the search finds it by. */
  kw: Key;
  /** Only on the laptop, or only on a phone. */
  only?: "hub" | "phone";
};

export const SETTINGS: readonly Setting[] = [
  { id: "add-phone", cat: "devices", title: "addDevice", kw: "kwAddPhone", only: "hub" },
  { id: "hub-link", cat: "devices", title: "forgetHub", kw: "kwHubLink", only: "phone" },
  { id: "paired", cat: "devices", title: "pairedDevices", kw: "kwPaired" },
  { id: "hotspot", cat: "network", title: "hotspotTitle", kw: "kwHotspot" },
  { id: "firewall", cat: "network", title: "firewallName", kw: "kwFirewall" },
  { id: "addresses", cat: "network", title: "networkAddresses", kw: "kwAddresses" },
  { id: "backup-encryption", cat: "backups", title: "backupEncryption", kw: "kwEncryption" },
  { id: "backup-now", cat: "backups", title: "backupNow", kw: "kwBackupNow" },
  { id: "backup-usb", cat: "backups", title: "backupToUsb", kw: "kwBackupUsb" },
  { id: "restore", cat: "backups", title: "restoreFromFile", kw: "kwRestore" },
  { id: "password", cat: "privacy", title: "householdPassword", kw: "kwPassword", only: "hub" },
  { id: "privacy", cat: "privacy", title: "privacy", kw: "kwPrivacy" },
  { id: "accent", cat: "appearance", title: "accentColor", kw: "kwAccent" },
  { id: "pure-black", cat: "appearance", title: "pureBlackShort", kw: "kwPureBlack" },
  { id: "language", cat: "language", title: "language", kw: "kwLanguage" },
  { id: "latin", cat: "language", title: "latinScript", kw: "kwLatin" },
  { id: "model", cat: "assistant", title: "catModels", kw: "kwModels" },
  { id: "memory", cat: "assistant", title: "memoryTitle", kw: "kwMemory" },
  { id: "updates", cat: "updates", title: "updates", kw: "kwUpdates" },
  { id: "version", cat: "about", title: "version", kw: "kwVersion" },
  { id: "this-hub", cat: "about", title: "thisHub", kw: "kwThisHub" },
  { id: "licenses", cat: "about", title: "licenses", kw: "kwLicenses" },
];

export function categoriesFor(isHub: boolean): Category[] {
  return CATEGORIES.filter((c) => isHub || !c.hubOnly);
}

export function categoryOf(id: CategoryId): Category {
  return CATEGORIES.find((c) => c.id === id)!;
}

function settingsFor(isHub: boolean): Setting[] {
  const cats = new Set(categoriesFor(isHub).map((c) => c.id));
  return SETTINGS.filter((s) => cats.has(s.cat) && (!s.only || (s.only === "hub") === isHub));
}

/** The address of a category's page, or of one setting on it. */
export function settingsHref(cat: CategoryId, setting?: string): string {
  return `#household/${cat}${setting ? `/${setting}` : ""}`;
}

/** The page and setting in an address like "#household/network/hotspot" (nothing for the tiles). */
export function householdRoute(hash: string): { cat: string | null; setting: string | null } {
  const [tab, cat, setting] = hash.replace(/^#/, "").split("/");
  if (tab !== "household") return { cat: null, setting: null };
  return { cat: cat || null, setting: setting || null };
}

/** Lowercase and without accents ("Šifrovanje" -> "sifrovanje", "đ" -> "dj", "Wi-Fi" -> "wifi"). */
export function fold(s: string): string {
  return s
    .toLowerCase()
    .replace(/đ/g, "dj")
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .replace(/[-'’]/g, "")
    .replace(/[^a-z0-9]+/g, " ")
    .trim();
}

const EN = makeT("en");
const SR = makeT("sr");

/** Folded, and once more with "dj" as "d", so "uredaji" finds "uređaji" too. */
function searchable(parts: string[]): string {
  const text = parts.map(fold).join(" ");
  return `${text} ${text.replace(/dj/g, "d")}`;
}

/**
 * The settings that have every word typed in their name, their category or
 * their keywords, in English or in Serbian (whatever the app's language), so
 * "wifi", "lozinka" and "sifrovanje" all find something. First the names
 * that start with the first word, then names that hold every word (in either
 * language), then the rest, each in the order of the pages.
 */
export function findSettings(query: string, isHub: boolean, t: (k: Key) => string): Setting[] {
  const words = fold(query).split(" ").filter(Boolean);
  if (words.length === 0) return [];
  const found: { s: Setting; rank: number }[] = [];
  for (const s of settingsFor(isHub)) {
    const cat = categoryOf(s.cat);
    const names = searchable([EN(s.title), SR(s.title)]);
    const all = `${names} ${searchable([EN(cat.title), SR(cat.title), EN(s.kw), SR(s.kw)])}`;
    if (!words.every((w) => all.includes(w))) continue;
    const rank = fold(t(s.title)).startsWith(words[0]) ? 0 : words.every((w) => names.includes(w)) ? 1 : 2;
    found.push({ s, rank });
  }
  return found.sort((a, b) => a.rank - b.rank).map((x) => x.s);
}
