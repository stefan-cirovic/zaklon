// The Zaklon map's look: the Protomaps basemap layers (BSD-3-Clause, CC0
// design) in our own colors, the VS Code "Dark Modern" greys, with labels in
// the app's language. Everything the style needs comes from the hub: tiles,
// fonts and icons. Loaded with the map, not with the app.
import { layers, namedFlavor, type Flavor } from "@protomaps/basemaps";
import type { ExpressionSpecification, LayerSpecification, StyleSpecification } from "maplibre-gl";
import type { Lang } from "../i18n";
import type { MapInfo } from "./info";

/** The map's source name and the credit it carries (see ATTRIBUTION in ZaklonMap). */
const SOURCE = "zaklon";

/**
 * Calm and dark: land a shade above the page, water a shade below with a
 * hint of blue, roads and borders in quiet greys, labels readable against
 * both. No amber anywhere: that is for our own marker.
 */
export const FLAVOR: Flavor = {
  ...namedFlavor("dark"),
  background: "#1b1d20",
  earth: "#262626",
  water: "#15181c",
  park_a: "#222823",
  park_b: "#212923",
  wood_a: "#222723",
  wood_b: "#222723",
  scrub_a: "#242623",
  scrub_b: "#242623",
  glacier: "#2a2b2d",
  sand: "#282725",
  beach: "#2a2927",
  hospital: "#2a2626",
  industrial: "#262626",
  school: "#282626",
  pedestrian: "#282828",
  aerodrome: "#262626",
  runway: "#333333",
  zoo: "#242623",
  military: "#272626",
  pier: "#2e2e2e",
  buildings: "#2d2d2e",

  tunnel_other_casing: "#1c1c1c",
  tunnel_minor_casing: "#1c1c1c",
  tunnel_link_casing: "#1c1c1c",
  tunnel_major_casing: "#1c1c1c",
  tunnel_highway_casing: "#1c1c1c",
  tunnel_other: "#2c2c2c",
  tunnel_minor: "#2c2c2c",
  tunnel_link: "#303030",
  tunnel_major: "#303030",
  tunnel_highway: "#343434",

  minor_service_casing: "#242424",
  minor_casing: "#242424",
  link_casing: "#242424",
  major_casing_late: "#242424",
  highway_casing_late: "#242424",
  other: "#323232",
  minor_service: "#323232",
  minor_a: "#383838",
  minor_b: "#333333",
  link: "#3c3c3c",
  major_casing_early: "#242424",
  major: "#404040",
  highway_casing_early: "#242424",
  highway: "#4a4a4a",
  railway: "#3a3a3a",
  boundaries: "#565b64",

  bridges_other_casing: "#2a2a2a",
  bridges_minor_casing: "#242424",
  bridges_link_casing: "#242424",
  bridges_major_casing: "#242424",
  bridges_highway_casing: "#242424",
  bridges_other: "#333333",
  bridges_minor: "#383838",
  bridges_link: "#3c3c3c",
  bridges_major: "#404040",
  bridges_highway: "#4a4a4a",

  roads_label_minor: "#8a8a8a",
  roads_label_minor_halo: "#1f1f1f",
  roads_label_major: "#9a9a9a",
  roads_label_major_halo: "#1f1f1f",
  ocean_label: "#66717e",
  subplace_label: "#8f8f8f",
  subplace_label_halo: "#1f1f1f",
  city_label: "#c2c2c2",
  city_label_halo: "#1b1b1b",
  state_label: "#6c6c6c",
  state_label_halo: "#1f1f1f",
  country_label: "#9d9d9d",
  address_label: "#7a7a7a",
  address_label_halo: "#1f1f1f",

  pois: {
    blue: "#7c9cb3",
    green: "#7fa58a",
    lapis: "#8193bd",
    pink: "#b08aa5",
    red: "#c08888",
    slategray: "#9a9aa6",
    tangerine: "#a89a8c",
    turquoise: "#7ea8ad",
  },

  landcover: {
    grassland: "#232823",
    barren: "#272725",
    urban_area: "#272727",
    farmland: "#242823",
    glacier: "#2b2b2d",
    scrub: "#242722",
    forest: "#212722",
  },
};

/** The language the map's labels are in: English, or Serbian in Latin script. */
export function labelLanguage(lang: Lang): string {
  return lang === "sr" ? "sr-Latn" : "en";
}

/**
 * Serbian names of countries, by their English names ("Germany" ->
 * "Nemačka"), from the web view: the map's data has none in Serbian. Flat
 * pairs for a "match" expression; empty where the web view cannot say.
 */
function serbianCountryNames(): string[] {
  try {
    const en = new Intl.DisplayNames(["en"], { type: "region", fallback: "none" });
    const sr = new Intl.DisplayNames(["sr-Latn"], { type: "region", fallback: "none" });
    const pairs: string[] = [];
    const seen = new Set<string>();
    for (let a = 65; a <= 90; a++) {
      for (let b = 65; b <= 90; b++) {
        const code = String.fromCharCode(a, b);
        const [e, s] = [en.of(code), sr.of(code)];
        if (e && s && e !== code && s !== code && !seen.has(e)) {
          seen.add(e);
          pairs.push(e, s);
        }
      }
    }
    return pairs;
  } catch {
    return [];
  }
}

/**
 * A place's name on one line, in the app's language where the map has it,
 * else in English, else as it is written there. (The Protomaps style adds
 * the local name on a second line; one line is calmer.) The map's data has
 * no Serbian names: for Serbian, places take their Croatian ones, the same
 * Latin names for places (Beograd, Beč, Pariz), and countries their Serbian
 * names from the web view.
 */
function nameExpression(lang: Lang, country: boolean): ExpressionSpecification {
  if (lang !== "sr") return ["coalesce", ["get", "name:en"], ["get", "name"]];
  const local: ExpressionSpecification = ["coalesce", ["get", "name:hr"], ["get", "name:en"], ["get", "name"]];
  const pairs = country ? serbianCountryNames() : [];
  return pairs.length ? (["match", ["get", "name:en"], ...pairs, local] as unknown as ExpressionSpecification) : local;
}

/**
 * The style's labels of places, waters and streets on one line (not road
 * numbers, not house numbers), and the icons of shops and sights dimmed: the
 * sprite's own colors would compete with the home's mark, the one colored
 * thing on the map.
 */
function oneLineNames(layers: LayerSpecification[], lang: Lang): LayerSpecification[] {
  return layers.map((l) => {
    if (l.type === "symbol" && l.id === "pois") l = { ...l, paint: { ...l.paint, "icon-opacity": 0.5 } };
    if (l.type !== "symbol" || !l.layout || l.layout["text-field"] === undefined || l.id === "places_region") return l;
    if (!JSON.stringify(l.layout["text-field"]).includes('"name')) return l;
    return { ...l, layout: { ...l.layout, "text-field": nameExpression(lang, l.id === "places_country") } };
  });
}

/**
 * The whole style for MapLibre. `base` is where the hub is (the page's own
 * address on the laptop, the app's private proxy on a phone).
 */
export function mapStyle(base: string, info: MapInfo, lang: Lang): StyleSpecification {
  const withLabels = info.glyphs;
  return {
    version: 8,
    // Without the fonts on the hub there are no labels, rather than failing requests.
    ...(withLabels ? { glyphs: `${base}/map/fonts/{fontstack}/{range}.pbf` } : {}),
    ...(info.sprites ? { sprite: `${base}/map/sprites/dark` } : {}),
    sources: {
      [SOURCE]: {
        type: "vector",
        tiles: [`${base}/tiles/{z}/{x}/{y}.mvt?v=${encodeURIComponent(info.tiles.key)}`],
        minzoom: info.tiles.min_zoom,
        maxzoom: Math.max(info.tiles.max_zoom, 0),
      },
    },
    layers: oneLineNames(layers(SOURCE, FLAVOR, withLabels ? { lang: labelLanguage(lang) } : undefined), lang),
  };
}
