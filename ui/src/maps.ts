import type { Lang } from "./i18n";

// What /api/maps says about the world's maps, shared by the Maps and Add-ons screens.

export type MapStatus = "not_installed" | "queued" | "downloading" | "paused" | "verifying" | "installed" | "failed";
export type Region = { id: string; name: string; name_sr: string; size: number; status: MapStatus; bytes_done: number; error?: string; update?: boolean };
export type Country = { id: string; name: string; name_sr: string; size: number; regions: Region[] };
export type MapsReply = {
  version: number;
  server_urls: string[];
  app_urls: string[];
  app: { status: MapStatus; bytes_done: number; bytes_total: number };
  installed_bytes: number;
  countries: Country[];
};

/** Suggested first, by app language. */
export const SUGGESTED: Record<Lang, string[]> = {
  sr: ["Serbia", "Bosnia and Herzegovina", "Croatia", "Montenegro", "Macedonia", "Kosovo", "Slovenia", "Hungary", "Romania", "Bulgaria"],
  en: [],
};

/** How far a country's map is: its pieces, summed up. */
export function countryState(c: Country) {
  const installed = c.regions.filter((r) => r.status === "installed").length;
  const busy = c.regions.some((r) => ["queued", "downloading", "verifying"].includes(r.status));
  const failed = c.regions.some((r) => r.status === "failed");
  const paused = c.regions.some((r) => r.status === "paused");
  const done = c.regions.reduce((s, r) => s + (r.status === "installed" ? r.size : Math.min(r.bytes_done, r.size)), 0);
  // Pieces of an older map version: they keep working until updated.
  const update = c.regions.some((r) => r.status === "installed" && r.update);
  return { installed, all: installed === c.regions.length, some: installed > 0, busy, failed, paused, done, update };
}
