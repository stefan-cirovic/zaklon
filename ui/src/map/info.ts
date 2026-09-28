import { invoke } from "@tauri-apps/api/core";
import { contentBase, getMode } from "../api";

// What /api/map says about the Zaklon map, shared by Home and the Maps screen.

export type MapPackStatus = "not_installed" | "queued" | "downloading" | "paused" | "verifying" | "installed" | "failed";

export type MapInfo = {
  tiles: {
    min_zoom: number;
    max_zoom: number;
    /** Changes with the set of map archives in use; part of the tiles' address. */
    key: string;
    /** A map pack (the world map) is in use, not only the overview. */
    detailed: boolean;
    overview: boolean;
  };
  glyphs: boolean;
  sprites: boolean;
  /** The world map pack and where it stands. */
  world: { id: string; size: number; status: MapPackStatus; bytes_done: number; bytes_total: number } | null;
  /** "Find a place" has its list of places on the hub. */
  places: boolean;
  /** The household's home, if it was set. */
  home: HomePlace | null;
  /** Drawn from what this phone carries (away from the hub), not from the hub. */
  phone?: boolean;
};

/** The household's home, as the hub keeps it. */
export type HomePlace = { lat: number; lon: number; label?: string | null; source?: "gps" | "place" | "map"; set_at?: string; set_by?: string };

/** A place from "find a place". */
export type FoundPlace = { name: string; name_sr?: string; region: string; country: string; lat: number; lon: number; population: number };

/** A point on the map. */
export type LatLon = { lat: number; lon: number };

/** Map data exists at all: the overview or a map pack. */
export function hasTiles(info: MapInfo | null): boolean {
  return !!info && (info.tiles.detailed || info.tiles.overview);
}

/** Where this device keeps the home it saw last, for the map away from home. It never leaves the device. */
const LAST_HOME = "zaklon.mapHome";

/** Remember the home the hub told about (a phone shows it away from home too). */
export function rememberHome(home: HomePlace | null) {
  try {
    if (home) localStorage.setItem(LAST_HOME, JSON.stringify({ lat: home.lat, lon: home.lon, label: home.label ?? null }));
    else localStorage.removeItem(LAST_HOME);
  } catch {
    /* private mode: not kept */
  }
}

function lastHome(): HomePlace | null {
  try {
    const h = JSON.parse(localStorage.getItem(LAST_HOME) ?? "null") as HomePlace | null;
    return h && Number.isFinite(h.lat) && Number.isFinite(h.lon) ? h : null;
  } catch {
    return null;
  }
}

/**
 * A phone away from the hub: the map from what the app carries (the world
 * overview), with the home it saw last. Null on the laptop, or when the app
 * carries no map.
 */
export async function phoneMapInfo(): Promise<MapInfo | null> {
  if ((await getMode()).mode !== "client") return null;
  const local = await invoke<{ overview: boolean; min_zoom: number; max_zoom: number; glyphs: boolean; sprites: boolean }>("client_map_local").catch(() => null);
  if (!local?.overview) return null;
  return {
    tiles: { min_zoom: local.min_zoom, max_zoom: local.max_zoom, key: "phone", detailed: false, overview: true },
    glyphs: local.glyphs,
    sprites: local.sprites,
    world: null,
    places: false,
    home: lastHome(),
    phone: true,
  };
}

/** Where the map's tiles, fonts and icons are asked for: the hub's own address on the laptop, the app's proxy to the hub on a phone. */
export async function mapBase(): Promise<string> {
  const base = await contentBase();
  return base || location.origin;
}

/** How close the map comes to a home: about a town, or a region when only the world overview is there. */
export function homeZoom(info: MapInfo | null): number {
  return info?.tiles.detailed ? 11.5 : 6;
}

/** A longitude from a map that repeats the world sideways, back between -180 and 180. */
export function wrapLon(lon: number): number {
  return ((((lon + 180) % 360) + 360) % 360) - 180;
}

/** Distance in kilometers between two points (good enough for "the same town"). */
export function distanceKm(a: LatLon, b: LatLon): number {
  const r = Math.PI / 180;
  const x = (b.lon - a.lon) * r * Math.cos(((a.lat + b.lat) / 2) * r);
  const y = (b.lat - a.lat) * r;
  return Math.sqrt(x * x + y * y) * 6371;
}

/** The country's name from its two-letter code, in the app's language (the code where the web view cannot say). */
export function countryName(code: string, lang: "en" | "sr"): string {
  try {
    return new Intl.DisplayNames([lang === "sr" ? "sr-Latn" : "en"], { type: "region" }).of(code) ?? code;
  } catch {
    return code;
  }
}

/** A place in a few words: "Novi Sad, Vojvodina, Serbia". */
export function placeLabel(p: FoundPlace, lang: "en" | "sr"): string {
  const name = lang === "sr" && p.name_sr ? p.name_sr : p.name;
  return [name, p.region && p.region !== p.name ? p.region : "", countryName(p.country, lang)].filter(Boolean).join(", ");
}

/** "44.8040° N, 20.4651° E". */
export function coords(p: LatLon): string {
  const lat = `${Math.abs(p.lat).toFixed(4)}° ${p.lat >= 0 ? "N" : "S"}`;
  const lon = `${Math.abs(p.lon).toFixed(4)}° ${p.lon >= 0 ? "E" : "W"}`;
  return `${lat}, ${lon}`;
}
