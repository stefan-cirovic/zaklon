import type { Key } from "./i18n";
import type { Glyph } from "./components/ExplorerIcons";

/**
 * The categories everything in Zaklon is sorted into, so that people who are
 * not technical find their way: the sections of the Tools screen and the
 * folders of Add-ons, in this order. The catalog gives each knowledge pack
 * and map one or more of them (its `topics`), and each tool names its own in
 * tools.ts, so the tools and the guides for a topic are found in one place.
 * The same list is TOPICS in crates/zaklon-core/src/catalog.rs.
 */
export const TOPICS = [
  { id: "health", name: "topicHealth", desc: "topicHealthDesc", glyph: "health" },
  { id: "water", name: "topicWater", desc: "topicWaterDesc", glyph: "water" },
  { id: "food", name: "topicFood", desc: "topicFoodDesc", glyph: "food" },
  { id: "garden", name: "topicGarden", desc: "topicGardenDesc", glyph: "garden" },
  { id: "power", name: "topicPower", desc: "topicPowerDesc", glyph: "power" },
  { id: "build", name: "topicBuild", desc: "topicBuildDesc", glyph: "build" },
  { id: "knowledge", name: "topicKnowledge", desc: "topicKnowledgeDesc", glyph: "book" },
  { id: "maps", name: "topicMaps", desc: "topicMapsDesc", glyph: "map" },
] as const satisfies readonly { id: string; name: Key; desc: Key; glyph: Glyph }[];

export type TopicId = (typeof TOPICS)[number]["id"];

export function isTopicId(id: unknown): id is TopicId {
  return typeof id === "string" && TOPICS.some((x) => x.id === id);
}
