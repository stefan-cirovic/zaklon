/**
 * The built-in user guide: one page per topic, written in English (en.ts)
 * and Serbian (sr.ts). Both are a `HelpContent`, so a topic missing from one
 * language does not compile; e2e/unit/help.check.mts also checks that the two
 * have the same sections, the same kinds of blocks and links that lead
 * somewhere.
 *
 * Text may hold two marks: **bold** (the names of buttons and screens, as the
 * app shows them) and [a link](#supplies) to a screen or to another help page
 * ("#help/<topic>" or "#help/<topic>/<section>"). Nothing else is parsed.
 */

/** The topics, in the order of the pages (Previous / Next follow it). */
export const HELP_TOPICS = [
  "start",
  "pairing",
  "home",
  "assistant",
  "supplies",
  "library",
  "maps",
  "addons",
  "household",
  "offline",
  "troubleshooting",
] as const;

export type HelpTopicId = (typeof HELP_TOPICS)[number];

export type HelpBlock =
  /** A paragraph. */
  | { p: string }
  /** Numbered steps, done in order. */
  | { steps: string[] }
  /** A list without an order. */
  | { list: string[] }
  /** Good to know: a tip, or why something is the way it is. */
  | { note: string }
  /** Important: something that can go wrong or cannot be undone. */
  | { warn: string };

export type HelpSection = {
  /** Part of the address (#help/supplies/scan), the same in every language. */
  id: string;
  title: string;
  body: HelpBlock[];
};

export type HelpTopic = {
  title: string;
  /** One line: on the topic's card and under its title. */
  summary: string;
  /** The screen the topic is about, as a button at its end. */
  open?: { href: string; label: string };
  sections: HelpSection[];
};

export type HelpContent = { [K in HelpTopicId]: HelpTopic };
