import type { ReactNode } from "react";
import type { Key } from "./i18n";
import type { Glyph } from "./components/ExplorerIcons";
import { isTopicId, TOPICS, type TopicId } from "./topics";

/** How a list is shown, remembered per device: big tiles, or a table like Explorer's "Details". */
export type ViewMode = "tiles" | "details";

// What /api/catalog says, shared by the Add-ons screen and Home.

export type Localized = { en: string; sr: string };
export type PackStatus = "not_installed" | "queued" | "downloading" | "paused" | "verifying" | "installed" | "failed";
export type Pack = {
  id: string;
  title: Localized;
  description: Localized;
  category: "knowledge" | "maps" | "model" | "app";
  /** What a knowledge pack is about (see topics.ts); it shows in the folder of each. */
  topics?: string[];
  version: string;
  size: number;
  license: string;
  attribution: string;
  recommended_for: string[];
  state: { status: PackStatus; bytes_done: number; bytes_total: number; error?: string; speed: number; update_available?: boolean };
};
export type CatalogReply = {
  packs: Pack[];
  system: { disk_free: number; disk_total: number; battery_percent: number | null; plugged_in: boolean };
  /** The root of the drive the library is on, like "D:\". */
  library_drive?: string;
};

/** A download under way (the hub works on it now or next). */
export const BUSY: PackStatus[] = ["queued", "downloading", "verifying"];
/** States shown with a progress bar. */
export const WITH_BAR: PackStatus[] = ["queued", "downloading", "verifying", "paused"];

/** "E:\" -> "E:" */
export function rootName(path: string) {
  return path.replace(/[\\/]+$/, "");
}

export type FolderId = TopicId | "models" | "programs";

/**
 * The folders of the Add-ons screen, in this order: the topics, the same
 * categories as the sections of the Tools screen, then AI models and
 * programs, which are not about a topic. A knowledge pack shows in the folder
 * of each of its topics; AI models, maps and programs by their category.
 */
export const FOLDERS: readonly { id: FolderId; name: Key; desc: Key; glyph: Glyph }[] = [
  ...TOPICS,
  { id: "models", name: "catModels", desc: "folderModelsDesc", glyph: "chip" },
  { id: "programs", name: "catApps", desc: "folderProgramsDesc", glyph: "program" },
];

export function isFolderId(id: unknown): id is FolderId {
  return typeof id === "string" && FOLDERS.some((f) => f.id === id);
}

export function folderOf(id: FolderId) {
  return FOLDERS.find((f) => f.id === id)!;
}

/**
 * Folders renamed when the folders became the topics: "Wikipedia and books"
 * is Knowledge now, "Repair and skills" Build and install. (Health and Garden
 * kept their addresses, and "Other knowledge" was already #addons/knowledge.)
 */
const RENAMED = new Map<string, FolderId>([
  ["reference", "knowledge"],
  ["skills", "build"],
]);

/** A folder's id now, for the id in an old address ("reference" -> "knowledge"). */
export function upgradedFolder(id: string): string {
  return RENAMED.get(id) ?? id;
}

/**
 * The folders a catalog pack shows in: each of its topics, in the catalog's
 * order (Knowledge when it has none this version knows), or the folder of its
 * category.
 */
export function packFolders(p: { category: string; topics?: string[] }): FolderId[] {
  if (p.category === "model") return ["models"];
  if (p.category === "app") return ["programs"];
  if (p.category === "maps") return ["maps"];
  const known = [...new Set(p.topics ?? [])].filter(isTopicId);
  return known.length > 0 ? known : ["knowledge"];
}

/** One pack or one country's map, ready to show in any view. */
export type Entry = {
  key: string;
  /** The folders it shows in; outside a folder, its icon is the first one's. */
  folders: FolderId[];
  name: string;
  desc: string;
  size: number;
  /** Version and license, for the tile. */
  meta: string;
  license: string;
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
  /** Bytes it takes on the disk when installed. */
  installedBytes: number;
  busy: boolean;
  bytesDone: number;
  bytesTotal: number;
  /** Folded text the search looks in. */
  search: string;
};

export type FolderStat = { id: FolderId; count: number; size: number; installed: number; installedBytes: number; busy: boolean; progress: number | null };

/** Sum up a folder's entries for its tile (a pack in two folders counts in both). */
export function folderStat(id: FolderId, entries: Entry[]): FolderStat {
  const mine = entries.filter((e) => e.folders.includes(id));
  const busy = mine.filter((e) => e.busy);
  const total = busy.reduce((s, e) => s + e.bytesTotal, 0);
  const done = busy.reduce((s, e) => s + Math.min(e.bytesDone, e.bytesTotal), 0);
  const installed = mine.filter((e) => e.installed);
  return {
    id,
    count: mine.length,
    size: mine.reduce((s, e) => s + e.size, 0),
    installed: installed.length,
    installedBytes: installed.reduce((s, e) => s + e.installedBytes, 0),
    busy: busy.length > 0,
    progress: total > 0 ? Math.min(100, Math.round((done / total) * 100)) : null,
  };
}
