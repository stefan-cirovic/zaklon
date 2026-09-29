import type { Key } from "./i18n";
import type { Glyph } from "./components/ExplorerIcons";

/**
 * The Library's topics, in this order: everything there is to read is
 * sorted into these plain categories, so that people who are not technical
 * find their way. The catalog gives each knowledge pack one or more of them
 * (its `topics`), and the pack is listed under each. A tool about a topic
 * names it in tools.ts, and the topic's page links to it. The same list,
 * with the maps after it, is TOPICS in crates/zaklon-core/src/catalog.rs;
 * the maps are in Tools › Maps, not in the Library.
 */
export const TOPICS = [
  { id: "health", name: "topicHealth", desc: "topicHealthDesc", glyph: "health" },
  { id: "water", name: "topicWater", desc: "topicWaterDesc", glyph: "water" },
  { id: "food", name: "topicFood", desc: "topicFoodDesc", glyph: "food" },
  { id: "garden", name: "topicGarden", desc: "topicGardenDesc", glyph: "garden" },
  { id: "power", name: "topicPower", desc: "topicPowerDesc", glyph: "power" },
  { id: "build", name: "topicBuild", desc: "topicBuildDesc", glyph: "build" },
  { id: "reference", name: "topicReference", desc: "topicReferenceDesc", glyph: "reference" },
] as const satisfies readonly { id: string; name: Key; desc: Key; glyph: Glyph }[];

export type TopicId = (typeof TOPICS)[number]["id"];

export function isTopicId(id: unknown): id is TopicId {
  return typeof id === "string" && TOPICS.some((x) => x.id === id);
}

export function topicOf(id: TopicId) {
  return TOPICS.find((x) => x.id === id)!;
}

/**
 * Topics named otherwise before: Encyclopedias and dictionaries was
 * "knowledge" (before the whole Library was about reading), and before the
 * folders of Add-ons became the topics, "reference" (Wikipedia and books)
 * and "skills" (repair and skills, now Build and install).
 */
const RENAMED = new Map<string, TopicId>([
  ["knowledge", "reference"],
  ["skills", "build"],
]);

/** A topic's id now, for the id in an old address, catalog or saved answer ("knowledge" -> "reference"). */
export function upgradedTopic(id: string): string {
  return RENAMED.get(id) ?? id;
}

/**
 * The topics a pack is listed under: each of its topics this version knows,
 * in the catalog's order, or Encyclopedias and dictionaries when it names
 * none of them.
 */
export function packTopics(topics: readonly string[] | undefined): TopicId[] {
  const known = [...new Set((topics ?? []).map(upgradedTopic))].filter(isTopicId);
  return known.length > 0 ? known : ["reference"];
}
