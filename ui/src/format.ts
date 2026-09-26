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

export function fmtBytes(n: number): string {
  const nf = (v: number, digits: number) => new Intl.NumberFormat(locale(), { maximumFractionDigits: digits }).format(v);
  if (n >= 1e9) return `${nf(n / 1e9, n >= 1e10 ? 0 : 1)} GB`;
  if (n >= 1e6) return `${nf(n / 1e6, 0)} MB`;
  return `${nf(n / 1e3, 0)} kB`;
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
