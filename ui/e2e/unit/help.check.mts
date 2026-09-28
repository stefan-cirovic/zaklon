// Run: node e2e/unit/help.check.mts  (Node 23.6+ strips the types)
//
// The user guide in English and Serbian must match: the same topics (the
// types already make sure of that), the same sections in the same order, the
// same kinds of blocks with the same number of steps, and the same links.
// Every link must lead somewhere: a screen, a Settings category or setting,
// an Add-ons folder, or a help page and section that exist.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const { HELP_TOPICS } = await import("../../src/help/types.ts");
const en = (await import("../../src/help/en.ts")).default;
const sr = (await import("../../src/help/sr.ts")).default;

const src = (file: string) => readFileSync(new URL(`../../src/${file}`, import.meta.url), "utf8");

// What the addresses may name, read from the app's own lists.
const SCREENS = ["home", "assistant", "tools", "settings", "help", ...[...src("tools.ts").matchAll(/\{ id: "(\w+)", title:/g)].map((m) => m[1])];
const CATEGORIES = [...src("settings.ts").matchAll(/\{ id: "([\w-]+)", title: "\w+", desc:/g)].map((m) => m[1]);
const SETTINGS = [...src("settings.ts").matchAll(/\{ id: "([\w-]+)", cat: "([\w-]+)"/g)].map((m) => `${m[2]}/${m[1]}`);
// The Add-ons folders: the topics (topics.ts), then AI models and programs (addons.ts).
const FOLDERS = ["topics.ts", "addons.ts"].flatMap((f) => [...src(f).matchAll(/\{ id: "(\w+)", name: "\w+", desc:/g)].map((m) => m[1]));
assert.ok(
  SCREENS.includes("supplies") && CATEGORIES.includes("backups") && SETTINGS.includes("network/hotspot") && FOLDERS.includes("health") && FOLDERS.includes("models"),
  "the app's lists were read",
);
const MAPS_PARTS = [...src("screens/Maps.tsx").matchAll(/sub === "(\w+)"/g)].map((m) => m[1]);
assert.deepEqual(MAPS_PARTS.sort(), ["home", "navigation"], "the parts of the Maps screen were read");

type Block = { p: string } | { steps: string[] } | { list: string[] } | { note: string } | { warn: string };
type Section = { id: string; title: string; body: Block[] };
type Topic = { title: string; summary: string; open?: { href: string; label: string }; sections: Section[] };
type Content = Record<string, Topic>;

const LINK = /\[([^\]]+)\]\((#[^)\s]*)\)/g;
const kind = (b: Block) => Object.keys(b)[0];
const texts = (b: Block): string[] => {
  const v = Object.values(b)[0];
  return Array.isArray(v) ? v : [v];
};

function checkHref(href: string, where: string) {
  const [tab, a, b, ...rest] = href.slice(1).split("/");
  assert.equal(rest.length, 0, `${where}: too many parts in ${href}`);
  assert.ok(SCREENS.includes(tab), `${where}: no screen "${tab}" (${href})`);
  if (tab === "help") {
    if (a === undefined) return;
    assert.ok((HELP_TOPICS as readonly string[]).includes(a), `${where}: no help topic "${a}" (${href})`);
    if (b !== undefined) assert.ok((en as Content)[a].sections.some((s) => s.id === b), `${where}: no section "${b}" in help topic "${a}"`);
  } else if (tab === "settings") {
    if (a === undefined) return;
    assert.ok(CATEGORIES.includes(a), `${where}: no Settings category "${a}" (${href})`);
    if (b !== undefined) assert.ok(SETTINGS.includes(`${a}/${b}`), `${where}: no setting "${b}" in Settings › ${a}`);
  } else if (tab === "addons") {
    assert.equal(b, undefined, `${where}: ${href}`);
    if (a !== undefined) assert.ok(FOLDERS.includes(a), `${where}: no Add-ons folder "${a}" (${href})`);
  } else if (tab === "maps") {
    // The parts of the Maps screen (routeOf in screens/Maps.tsx).
    assert.equal(b, undefined, `${where}: ${href}`);
    if (a !== undefined) assert.ok(MAPS_PARTS.includes(a), `${where}: no part "${a}" of Maps (${href})`);
  } else {
    assert.equal(a, undefined, `${where}: ${href} has no places of its own`);
  }
}

/** The links of a text, checked; and no mark left half-written. */
function linksOf(text: string, where: string): string[] {
  const hrefs = [...text.matchAll(LINK)].map((m) => m[2]);
  for (const h of hrefs) checkHref(h, where);
  const rest = text.replace(LINK, "$1").replace(/\*\*(.+?)\*\*/g, "$1");
  assert.ok(!rest.includes("**"), `${where}: an unclosed ** in "${text}"`);
  assert.ok(!/\]\(/.test(rest), `${where}: a link that is not to a screen (#...) in "${text}"`);
  assert.ok(text.trim() === text && text.length > 0, `${where}: empty text, or spaces around it`);
  return hrefs;
}

let links = 0;
let words = 0;
for (const [name, content] of [["en", en], ["sr", sr]] as const) {
  assert.deepEqual(Object.keys(content), [...HELP_TOPICS], `${name}: the topics, in the order of HELP_TOPICS`);
  for (const [id, topic] of Object.entries(content as Content)) {
    assert.ok(topic.title && topic.summary, `${name}/${id}: a title and a summary`);
    assert.ok(topic.sections.length > 0, `${name}/${id}: at least one section`);
    const ids = topic.sections.map((s) => s.id);
    assert.equal(new Set(ids).size, ids.length, `${name}/${id}: section ids are unique`);
    for (const s of ids) assert.match(s, /^[a-z0-9-]+$/, `${name}/${id}: section id "${s}" fits in an address`);
    if (topic.open) checkHref(topic.open.href, `${name}/${id} (open)`);
    for (const s of topic.sections) for (const b of s.body) for (const x of texts(b)) words += x.split(/\s+/).length;
  }
}

for (const id of HELP_TOPICS) {
  const a = (en as Content)[id];
  const b = (sr as Content)[id];
  assert.equal(a.open?.href, b.open?.href, `${id}: the same screen to open`);
  assert.deepEqual(b.sections.map((s) => s.id), a.sections.map((s) => s.id), `${id}: the same sections in the same order`);
  a.sections.forEach((sa, i) => {
    const sb = b.sections[i];
    const where = `${id}/${sa.id}`;
    assert.deepEqual(sb.body.map(kind), sa.body.map(kind), `${where}: the same kinds of blocks`);
    sa.body.forEach((ba, k) => {
      assert.equal(texts(sb.body[k]).length, texts(ba).length, `${where}: block ${k + 1} has as many steps or points in both languages`);
    });
    const la = [sa.title, ...sa.body.flatMap(texts)].flatMap((x) => linksOf(x, `en/${where}`));
    const lb = [sb.title, ...sb.body.flatMap(texts)].flatMap((x) => linksOf(x, `sr/${where}`));
    assert.deepEqual([...lb].sort(), [...la].sort(), `${where}: the same links in both languages`);
    links += la.length;
  });
}

// Each Settings category page links to its section of the Settings topic.
for (const cat of CATEGORIES) {
  assert.ok((en as Content).settings.sections.some((s) => s.id === cat), `the Settings topic has a section "${cat}" for its category page`);
}

console.log(`help: ${HELP_TOPICS.length} topics in English and Serbian match; ${links} links checked; about ${words} words.`);
