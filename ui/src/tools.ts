import type { Key } from "./i18n";
import type { TopicId } from "./topics";

/**
 * Every tool of the app, each once, in the order of the Tools screen. The id
 * is also the tool's address (#supplies) and what the hub keeps as the
 * pinned tool; `short` is its name in the bar. `topics` are the Library's
 * topics the tool helps with: the tool shows a quiet link to their guides,
 * and each topic's page links back to the tool. A new screen that is a tool
 * is added here, with its icon in components/Icon.tsx.
 */
export const TOOLS = [
  { id: "supplies", title: "supplies", short: "supplies", desc: "toolSuppliesDesc", topics: ["food", "health"] },
  { id: "maps", title: "maps", short: "maps", desc: "toolMapsDesc", topics: [] },
  { id: "power", title: "powerCalc", short: "powerShort", desc: "toolPowerDesc", topics: ["power"] },
  { id: "water", title: "waterCalc", short: "waterShort", desc: "toolWaterDesc", topics: ["water", "garden"] },
] as const satisfies readonly { id: string; title: Key; short: Key; desc: Key; topics: readonly TopicId[] }[];

export type ToolId = (typeof TOOLS)[number]["id"];

export function isToolId(id: unknown): id is ToolId {
  return typeof id === "string" && TOOLS.some((x) => x.id === id);
}

export function toolOf(id: ToolId) {
  return TOOLS.find((x) => x.id === id)!;
}

/** The tools that help with a topic of the Library (Water: the water calculator). */
export function toolsFor(topic: TopicId) {
  return TOOLS.filter((x) => (x.topics as readonly TopicId[]).includes(topic));
}
