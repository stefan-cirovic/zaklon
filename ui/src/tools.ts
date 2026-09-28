import type { Key } from "./i18n";
import type { Glyph } from "./components/ExplorerIcons";
import { TOPICS, type TopicId } from "./topics";

/** Where a tool is listed on the Tools screen: a topic, or Downloads (Add-ons, which serves every topic). */
export type ToolCategory = TopicId | "downloads";

/**
 * The sections of the Tools screen, in this order: the topics (the same
 * categories as the folders of Add-ons, each with a link to its folder),
 * then Downloads.
 */
export const TOOL_SECTIONS = [
  ...TOPICS,
  { id: "downloads", name: "topicDownloads", desc: "topicDownloadsDesc", glyph: "download" },
] as const satisfies readonly { id: ToolCategory; name: Key; desc: Key; glyph: Glyph }[];

/**
 * Every tool of the app, as listed on the Tools screen. The id is also the
 * tool's address (#supplies) and what the hub keeps as the pinned tool. The
 * category, one or a list, is the section (or sections) of the Tools screen
 * the tool is listed in. A new screen that is a tool is added here, with its
 * icon in components/Icon.tsx.
 */
export const TOOLS = [
  { id: "supplies", title: "supplies", desc: "toolSuppliesDesc", category: ["food", "health"] },
  { id: "library", title: "library", desc: "toolLibraryDesc", category: "knowledge" },
  { id: "maps", title: "maps", desc: "toolMapsDesc", category: "maps" },
  { id: "addons", title: "addons", desc: "toolAddonsDesc", category: "downloads" },
] as const satisfies readonly { id: string; title: Key; desc: Key; category: ToolCategory | readonly ToolCategory[] }[];

export type ToolId = (typeof TOOLS)[number]["id"];

export function isToolId(id: unknown): id is ToolId {
  return typeof id === "string" && TOOLS.some((x) => x.id === id);
}

export function toolOf(id: ToolId) {
  return TOOLS.find((x) => x.id === id)!;
}

/** The sections a tool is listed in. */
export function toolCategories(tool: { category: ToolCategory | readonly ToolCategory[] }): readonly ToolCategory[] {
  return typeof tool.category === "string" ? [tool.category] : tool.category;
}
