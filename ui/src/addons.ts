import type { ReactNode } from "react";
import type { Key } from "./i18n";
import type { Glyph } from "./components/ExplorerIcons";

/** How a list is shown, remembered per device: big tiles, or a table like Explorer's "Details". */
export type ViewMode = "tiles" | "details";

export type FolderId = "reference" | "health" | "garden" | "skills" | "knowledge" | "models" | "maps" | "programs";

/**
 * The folders of the Add-ons screen, in this order. Knowledge packs go by the
 * topic the catalog gives them ("Other knowledge" when it has none we know),
 * AI models, maps and programs by their category.
 */
export const FOLDERS: { id: FolderId; name: Key; desc: Key; glyph: Glyph }[] = [
  { id: "reference", name: "folderReference", desc: "folderReferenceDesc", glyph: "book" },
  { id: "health", name: "folderHealth", desc: "folderHealthDesc", glyph: "health" },
  { id: "garden", name: "folderGarden", desc: "folderGardenDesc", glyph: "garden" },
  { id: "skills", name: "folderSkills", desc: "folderSkillsDesc", glyph: "skills" },
  { id: "knowledge", name: "folderKnowledge", desc: "folderKnowledgeDesc", glyph: "layers" },
  { id: "models", name: "catModels", desc: "folderModelsDesc", glyph: "chip" },
  { id: "maps", name: "catMaps", desc: "folderMapsDesc", glyph: "map" },
  { id: "programs", name: "catApps", desc: "folderProgramsDesc", glyph: "program" },
];

/** Catalog topics that have a folder of their own. */
const TOPICS: FolderId[] = ["reference", "health", "garden", "skills"];

export function isFolderId(id: unknown): id is FolderId {
  return typeof id === "string" && FOLDERS.some((f) => f.id === id);
}

export function folderOf(id: FolderId) {
  return FOLDERS.find((f) => f.id === id)!;
}

/** The folder a catalog pack is shown in. */
export function packFolder(p: { category: string; topic?: string }): FolderId {
  if (p.category === "model") return "models";
  if (p.category === "app") return "programs";
  if (p.category === "maps") return "maps";
  return TOPICS.find((x) => x === p.topic) ?? "knowledge";
}

/** One pack or one country's map, ready to show in any view. */
export type Entry = {
  key: string;
  folder: FolderId;
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

/** Sum up a folder's entries for its tile. */
export function folderStat(id: FolderId, entries: Entry[]): FolderStat {
  const mine = entries.filter((e) => e.folder === id);
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
