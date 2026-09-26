import type { Lang } from "./i18n";

// Numbers, sizes and dates follow the app's language (not the phone's), so a
// Serbian interface shows "1,5 kg" and "31.01.2027." everywhere.

let lang: Lang = "en";

export function setFormatLang(l: Lang) {
  lang = l;
}

function locale(): string {
  return lang === "sr" ? "sr-Latn-RS" : "en-GB";
}

export function fmtQty(n: number): string {
  return new Intl.NumberFormat(locale(), { maximumFractionDigits: 3 }).format(n);
}

/** Sizes the way Windows Explorer, Android and CoMaps show them (1 MB = 1024 × 1024 bytes). */
export function fmtBytes(n: number): string {
  const nf = (v: number, digits: number) => new Intl.NumberFormat(locale(), { maximumFractionDigits: digits }).format(v);
  const KB = 1024, MB = KB * 1024, GB = MB * 1024;
  if (n >= 1000 * MB) return `${nf(n / GB, n >= 10 * GB ? 0 : 1)} GB`;
  if (n >= 1000 * KB) return `${nf(n / MB, 0)} MB`;
  return `${nf(n / KB, 0)} kB`;
}

/** "YYYY-MM-DD" -> "31.01.2027." (Serbian) or "31 Jan 2027" (English). */
export function fmtDate(iso: string): string {
  const [y, m, d] = iso.split("-").map(Number);
  if (!y || !m || !d) return iso;
  if (lang === "sr") return `${String(d).padStart(2, "0")}.${String(m).padStart(2, "0")}.${y}.`;
  return new Date(Date.UTC(y, m - 1, d)).toLocaleDateString("en-GB", { day: "numeric", month: "short", year: "numeric", timeZone: "UTC" });
}

/** A timestamp (RFC 3339) in local time. */
export function fmtDateTime(ts: string): string {
  const d = new Date(ts);
  if (Number.isNaN(d.getTime())) return ts;
  return d.toLocaleString(locale(), { dateStyle: "medium", timeStyle: "short" });
}

/** Parse "1,5" or "1.5". Returns null for anything that is not a number. */
export function parseNumber(s: string): number | null {
  const v = parseFloat(s.trim().replace(",", "."));
  return Number.isFinite(v) ? v : null;
}

export function todayIso(): string {
  const d = new Date();
  const p = (x: number) => String(x).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

export function daysUntil(date: string): number {
  const [y, m, d] = date.split("-").map(Number);
  const target = Date.UTC(y, m - 1, d);
  const now = new Date();
  const today = Date.UTC(now.getFullYear(), now.getMonth(), now.getDate());
  return Math.round((target - today) / 86400000);
}

/** First 8 hex digits of a certificate fingerprint as "AB12 CD34", for people to compare. */
export function securityCode(fp: string): string {
  const s = fp.replace(/[^0-9a-fA-F]/g, "").slice(0, 8).toUpperCase();
  return `${s.slice(0, 4)} ${s.slice(4)}`;
}

/**
 * Unit name for a quantity. Most units stay abbreviated (kg, g, ml, pcs);
 * litres are written out with the right grammatical form:
 * Serbian 1 litar, 2 litra, 5 litara, 1,5 litra; English 1 liter, 2 liters.
 */
export function unitLabel(unit: string, qty: number, short: (u: string) => string): string {
  if (unit !== "l") return short(unit);
  if (lang === "sr") {
    if (!Number.isInteger(qty)) return "litra";
    const n = Math.abs(qty);
    const last = n % 10;
    const lastTwo = n % 100;
    if (last === 1 && lastTwo !== 11) return "litar";
    if (last >= 2 && last <= 4 && (lastTwo < 12 || lastTwo > 14)) return "litra";
    return "litara";
  }
  return qty === 1 ? "liter" : "liters";
}

// ---- Serbian Cyrillic to Latin (same rules as the hub) --------------------

const CYR: Record<string, string> = {
  а: "a", б: "b", в: "v", г: "g", д: "d", ђ: "đ", е: "e", ж: "ž", з: "z", и: "i", ј: "j", к: "k", л: "l", љ: "lj",
  м: "m", н: "n", њ: "nj", о: "o", п: "p", р: "r", с: "s", т: "t", ћ: "ć", у: "u", ф: "f", х: "h", ц: "c", ч: "č",
  џ: "dž", ш: "š", я: "ja", ю: "ju", ё: "jo", й: "j", ы: "y", э: "e", щ: "šč", ъ: "", ь: "", ѓ: "ǵ", ќ: "ḱ", ѕ: "dz",
  і: "i", ї: "ji", є: "je", ґ: "g",
};

export function cyrToLat(s: string): string {
  const chars = [...s];
  let out = "";
  chars.forEach((c, i) => {
    const lower = c.toLowerCase();
    const lat = CYR[lower];
    if (lat === undefined) {
      out += c;
      return;
    }
    if (c === lower) {
      out += lat;
      return;
    }
    const next = chars[i + 1];
    const prev = chars[i - 1];
    const isUpper = (x?: string) => !!x && x !== x.toLowerCase();
    const isLower = (x?: string) => !!x && x !== x.toUpperCase();
    if (isUpper(next) || (isUpper(prev) && !isLower(next))) out += lat.toUpperCase();
    else out += lat.charAt(0).toUpperCase() + lat.slice(1);
  });
  return out;
}

/** Show Serbian articles in Latin script: a per-device choice, on by default in Serbian. */
export function latinArticles(appLang: Lang): boolean {
  try {
    const v = localStorage.getItem("zaklon.latin");
    if (v === "1") return true;
    if (v === "0") return false;
  } catch {
    /* ignore */
  }
  return appLang === "sr";
}

export function setLatinArticles(on: boolean) {
  try {
    localStorage.setItem("zaklon.latin", on ? "1" : "0");
  } catch {
    /* ignore */
  }
}
