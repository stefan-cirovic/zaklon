import type { Key } from "./i18n";

/**
 * Every tool of the app, as listed on the Tools screen. The id is also the
 * tool's address (#supplies) and what the hub keeps as the pinned tool. A new
 * screen that is a tool is added here, with its icon in components/Icon.tsx.
 */
export const TOOLS = [
  { id: "supplies", title: "supplies", desc: "toolSuppliesDesc" },
  { id: "library", title: "library", desc: "toolLibraryDesc" },
  { id: "maps", title: "maps", desc: "toolMapsDesc" },
  { id: "addons", title: "addons", desc: "toolAddonsDesc" },
] as const satisfies readonly { id: string; title: Key; desc: Key }[];

export type ToolId = (typeof TOOLS)[number]["id"];

export function isToolId(id: unknown): id is ToolId {
  return typeof id === "string" && TOOLS.some((x) => x.id === id);
}

export function toolOf(id: ToolId) {
  return TOOLS.find((x) => x.id === id)!;
}
