import type { ReactNode } from "react";
import type { Key } from "./i18n";
import type { Glyph } from "./components/ExplorerIcons";
import type { TopicId } from "./topics";

// The packs: what /api/catalog says about the guides, maps, AI models and
// programs, shared by the Library (reading and getting guides), Tools ›
// Maps (the world map), Settings › AI assistant (the models), Storage &
// Downloads (what is on the hub) and Home.

/** How a list is shown, remembered per device: big tiles, or a table like Explorer's "Details". */
export type ViewMode = "tiles" | "details";

export type Localized = { en: string; sr: string };
export type PackStatus = "not_installed" | "queued" | "downloading" | "paused" | "verifying" | "installed" | "failed";
export type Pack = {
  id: string;
  title: Localized;
  description: Localized;
  category: "knowledge" | "maps" | "model" | "app";
  /** What a knowledge pack is about (see topics.ts): the Library lists it under each. */
  topics?: string[];
  version: string;
  size: number;
  license: string;
  attribution: string;
  /** Where it comes from (the publisher's site, or the library it is downloaded from). */
  source?: string;
  /**
   * How it may be offered: "auto" like any pack, "user" only as one people
   * download themselves after confirming its license (never preselected, never
   * in a starter set). Anything else is treated as "user".
   */
  offer?: string;
  /** Why it is "user": "noncommercial" or "mixed_licenses". */
  offer_reason?: string;
  /** On the hub, but no longer offered by the catalog: usable until deleted, never downloaded again. */
  withdrawn?: boolean;
  recommended_for: string[];
  state: { status: PackStatus; bytes_done: number; bytes_total: number; error?: string; speed: number; update_available?: boolean };
};
/** What a household starts with in one app language (the catalog's `starter_sets`). */
export type StarterSetDef = { lang: string; packs: string[]; map?: string | null };
export type CatalogReply = {
  packs: Pack[];
  starter_sets?: StarterSetDef[];
  system: { disk_free: number; disk_total: number; battery_percent: number | null; plugged_in: boolean };
  /** The root of the drive the library is on, like "D:\". */
  library_drive?: string;
  /** The world map: the build offered and the one on the hub. */
  world?: WorldInfo;
};

/** One installed book of the library (/api/library): a knowledge pack's file, read in the Reader. */
export type Book = {
  name: string;
  pack_id: string;
  title_en: string;
  title_sr: string;
  languages: string[];
  /** Its start page, relative to the hub. */
  home: string;
  /** Its pack's topics (an older hub does not say; then the catalog's are used). */
  topics?: string[];
};
export type Engine = "missing" | "idle" | "starting" | "running" | "failed";
export type LibraryReply = { engine: Engine; books: Book[] };

/** The world map's pack id. */
export const WORLD_MAP_ID = "world-map";

/** What the hub says about the world map (builds are dates, "20260811"). */
export type WorldInfo = {
  /** The build offered for download, or the one downloading. */
  offered: string;
  offered_size: number;
  /** The build on the hub, if any. */
  installed: string | null;
  installed_size: number;
  /** A newer build than the one on the hub is offered. */
  update: boolean;
  /** Free space the offered build still needs (with what is kept free). */
  needed: number;
  disk_free: number;
  /** The offered build fits next to the one on the hub. */
  room_for_both: boolean;
  /** Offered from Protomaps' list of builds (else the build that comes with the app). */
  listed: boolean;
  /** When the list was last read, if ever. */
  checked_at?: string | null;
};

/** A build's date, "20260811", as "2026-08-11" (for fmtDate); anything else as it is. */
export function buildDay(build: string): string {
  return /^\d{8}$/.test(build) ? `${build.slice(0, 4)}-${build.slice(4, 6)}-${build.slice(6, 8)}` : build;
}

/** A pack people download themselves: its license has conditions (see `offer`). */
export function downloadedByUser(p: { offer?: string }) {
  return (p.offer ?? "auto") !== "auto";
}

/** A download under way (the hub works on it now or next). */
export const BUSY: PackStatus[] = ["queued", "downloading", "verifying"];
/** States shown with a progress bar. */
export const WITH_BAR: PackStatus[] = ["queued", "downloading", "verifying", "paused"];

/** "E:\" -> "E:" */
export function rootName(path: string) {
  return path.replace(/[\\/]+$/, "");
}

export type KindId = "guides" | "maps" | "models" | "programs";

/**
 * What a pack is, as Storage & Downloads sorts what is on the hub into
 * folders: guides and books (what the Library reads), maps, AI models and
 * programs.
 */
export const KINDS: readonly { id: KindId; name: Key; glyph: Glyph }[] = [
  { id: "guides", name: "kindGuides", glyph: "book" },
  { id: "maps", name: "maps", glyph: "map" },
  { id: "models", name: "catModels", glyph: "chip" },
  { id: "programs", name: "catApps", glyph: "program" },
];

export function isKindId(id: unknown): id is KindId {
  return typeof id === "string" && KINDS.some((k) => k.id === id);
}

export function kindOf(id: KindId) {
  return KINDS.find((k) => k.id === id)!;
}

/** The kind of a catalog pack. */
export function packKind(p: { category: string }): KindId {
  if (p.category === "model") return "models";
  if (p.category === "app") return "programs";
  if (p.category === "maps") return "maps";
  return "guides";
}

/** One pack or one country's map, ready to show in any view. */
export type Entry = {
  key: string;
  kind: KindId;
  /** The Library's topics it is listed under (knowledge packs). */
  topics: TopicId[];
  name: string;
  desc: string;
  size: number;
  /** Version and license, for the tile. */
  meta: string;
  license: string;
  /** One line under it: why people download it themselves, or that it is no longer offered. */
  note: string | null;
  /** People download it themselves: listed apart, at the bottom of its list. */
  byUser: boolean;
  /** Its credit line and where it comes from, for its details. */
  credit: { attribution: string; source: string } | null;
  /** The question before a download (the license, named), while it is asked. */
  ask: ReactNode;
  recommended: boolean;
  status: string;
  tone: "ok" | "warn" | "muted";
  /** 0-100 while a bar shows (queued, downloading, verifying, paused). */
  progress: number | null;
  /** Under the bar: "1,1 GB / 2,3 GB · 3 MB/s". */
  detail: string | null;
  error: string | null;
  /** The buttons; smaller in the details view. */
  actions: (small: boolean) => ReactNode;
  /** Something of it is on the hub's disk (installed, or partly downloaded). */
  onDisk: boolean;
  installed: boolean;
  /** A newer version waits (installed, or downloading as an update). */
  update: boolean;
  /** Bytes it takes on the disk when installed. */
  installedBytes: number;
  busy: boolean;
  /** Paused or failed: a download that waits for a person. */
  stopped: boolean;
  bytesDone: number;
  bytesTotal: number;
};

export type KindStat = { id: KindId; count: number; size: number; busy: boolean; progress: number | null };

/** Sum up what of a kind is on the hub, for its folder's tile. */
export function kindStat(id: KindId, entries: Entry[]): KindStat {
  const mine = entries.filter((e) => e.kind === id && e.onDisk);
  const busy = mine.filter((e) => e.busy);
  const total = busy.reduce((s, e) => s + e.bytesTotal, 0);
  const done = busy.reduce((s, e) => s + Math.min(e.bytesDone, e.bytesTotal), 0);
  return {
    id,
    count: mine.length,
    size: mine.reduce((s, e) => s + (e.installed ? e.installedBytes : Math.min(e.bytesDone, e.bytesTotal)), 0),
    busy: busy.length > 0,
    progress: total > 0 ? Math.min(100, Math.round((done / total) * 100)) : null,
  };
}
