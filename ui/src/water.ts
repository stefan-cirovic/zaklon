/**
 * The water calculator's arithmetic (Tools › Water calculator), kept apart
 * from the screen so it can be checked on its own (e2e/unit/water.check.mts).
 * Two parts: how much drinking water a household should store, and how much
 * a vegetable garden needs with drip irrigation.
 *
 * The household's inputs are one JSON document (`WaterPlan`) that the hub
 * keeps in the setting "water_plan" (GET/PUT /api/water, see the hub's
 * api/water.rs), so every device shows the same numbers.
 *
 * Sources (numbers only; no text is copied):
 * - Drinking water: Ready.gov, "Water" (https://www.ready.gov/water): at
 *   least one gallon per person per day; children, nursing mothers and sick
 *   people may need more; in very hot weather the need can double.
 * - Basic hygiene: the Sphere Handbook (2018), water supply standard 2.1: at
 *   least 15 liters per person per day for drinking and domestic hygiene.
 * - Making water safe: CDC (https://www.cdc.gov/water-emergency/about/index.html)
 *   and EPA (https://www.epa.gov/ground-water-and-drinking-water/emergency-disinfection-drinking-water).
 * - Reference evapotranspiration: Hargreaves, G.H. and Samani, Z.A. (1985),
 *   "Reference crop evapotranspiration from temperature", Applied Engineering
 *   in Agriculture 1(2): 96-99. Extraterrestrial radiation from latitude and
 *   day of the year: Allen, R.G. et al. (1998), Crop evapotranspiration, FAO
 *   Irrigation and Drainage Paper 56, equations 21-25.
 * - Crop coefficients: USDA NRCS, National Engineering Handbook Part 623,
 *   Chapter 2, "Irrigation Water Requirements" (1993, public domain),
 *   https://www.wcc.nrcs.usda.gov/ftpref/wntsc/waterMgt/irrigation/NEH15/ch2.pdf
 * - Gravity drip kits: Palada M. et al. 2011. More Crop Per Drop. AVRDC – The
 *   World Vegetable Center (CC BY-SA 3.0),
 *   https://www.susana.org/_resources/documents/default/2-1094-robertholmereb0086.pdf
 *
 * Deep link (for the assistant and for links in Help): the calculator opens
 * filled in from the address, over the household's numbers, as a draft that
 * is saved for the household only when the person chooses to.
 *   #tools/water?people=4&days=7                 (also "#water?...")
 *   #tools/water?part=drip&beds=3x1.2:tomatoes,2x1:greens&lat=44.8
 * Drinking water: people (adults), children, smallpets (cats and small dogs),
 * largepets (large dogs), days. Garden: part=drip opens the garden part;
 * beds is a comma-separated list of "LENGTHxWIDTH:crop" (meters) or
 * "AREA:crop" (square meters), each optionally followed by ":rows" (drip
 * lines in the bed); crop is one of CROPS (or a word such as "cucumbers" or
 * "lettuce", see CROP_WORDS). Also lat (degrees, south negative), month
 * (1-12), tmax and tmin (°C), et0 (mm/day, the user's own), rain (mm in the
 * month), spacing (20, 30 or 40 cm), flow (1, 2 or 4 L/h) and roof (m²).
 * Anything missing keeps what the household had; anything invalid is ignored.
 */

// ---- drinking water ---------------------------------------------------------

/** Ready.gov: at least one gallon per person per day (3.785 L, rounded up). */
export const DRINK_L_PER_PERSON_DAY = 3.8;
/** Sphere: at least 15 L per person per day for drinking, cooking and basic hygiene. */
export const HYGIENE_L_PER_PERSON_DAY = 15;
/**
 * Pets: about 65 mL per kg of body weight a day (1 ounce per pound, a common
 * veterinary rule of thumb; Ready.gov gives no amount), for a small pet (a cat
 * or a small dog) of 10 kg and a large dog of 30 kg.
 */
export const PET_L_PER_DAY = { small: 0.65, large: 1.95 } as const;
/** The containers the result is counted in, in liters: canisters and a drum. */
export const CONTAINERS = [5, 10, 20, 200] as const;
/** The choices of days offered as buttons; any other number is "other". */
export const DAY_CHOICES = [3, 7, 14] as const;

export type Drink = { people: number; children: number; smallPets: number; largePets: number; days: number };

export type DrinkResult = {
  /** Liters a day for drinking and cooking, pets included. */
  perDay: number;
  /** Liters to store for drinking and cooking, rounded up. */
  drinking: number;
  /** Liters to store with basic hygiene too, rounded up. */
  hygiene: number;
};

export function drinkingWater(d: Drink): DrinkResult {
  const persons = d.people + d.children;
  const pets = d.smallPets * PET_L_PER_DAY.small + d.largePets * PET_L_PER_DAY.large;
  const perDay = persons * DRINK_L_PER_PERSON_DAY + pets;
  return {
    perDay,
    drinking: roundUp(perDay * d.days),
    hygiene: roundUp((persons * HYGIENE_L_PER_PERSON_DAY + pets) * d.days),
  };
}

/** How many of each container hold `liters` (the last one only partly filled). */
export function containersFor(liters: number): { size: number; count: number }[] {
  return CONTAINERS.map((size) => ({ size, count: liters > 0 ? roundUp(liters / size) : 0 }));
}

/**
 * CDC: 8 drops (about ½ mL) of 5-9% unscented household bleach per gallon of
 * clear water, twice as much for cloudy water; so 2 drops per liter, and a
 * drop is about 1/16 mL.
 */
export const BLEACH_DROPS_PER_L = 2;
export const BLEACH_ML_PER_DROP = 0.5 / 8;

// ---- the garden: reference evapotranspiration ---------------------------------

/** The solar constant, MJ m⁻² min⁻¹ (FAO-56). */
const GSC = 0.082;
/** 1 MJ m⁻² of radiation evaporates 0.408 mm of water (FAO-56, eq. 20). */
const MJ_TO_MM = 0.408;
export const DAYS_IN_MONTH = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31] as const;

/** The day of the year in the middle of a month (1-12), as FAO-56 suggests: J = INT(30.4 M - 15). */
export function midMonthDay(month: number): number {
  return Math.floor(30.4 * month - 15);
}

/**
 * Extraterrestrial radiation Ra (MJ m⁻² day⁻¹): the sun's radiation at the
 * top of the atmosphere, from the latitude (degrees, south negative) and the
 * day of the year (FAO-56, equations 21-25).
 */
export function extraterrestrialRadiation(latDeg: number, dayOfYear: number): number {
  const phi = (latDeg * Math.PI) / 180;
  const b = (2 * Math.PI * dayOfYear) / 365;
  const dr = 1 + 0.033 * Math.cos(b); // inverse relative distance Earth-Sun
  const decl = 0.409 * Math.sin(b - 1.39); // solar declination
  // Sunset hour angle; clamped for polar day and night.
  const ws = Math.acos(Math.min(1, Math.max(-1, -Math.tan(phi) * Math.tan(decl))));
  const ra = ((24 * 60) / Math.PI) * GSC * dr * (ws * Math.sin(phi) * Math.sin(decl) + Math.cos(phi) * Math.cos(decl) * Math.sin(ws));
  return Math.max(0, ra);
}

/**
 * Reference evapotranspiration ET₀ (mm/day) by Hargreaves and Samani (1985)
 * from the latitude, the month and its usual daily high and low temperature:
 *   ET₀ = 0.0023 × Ra × (Tmean + 17.8) × √(Tmax − Tmin), Ra in mm/day.
 * Null when something is missing or the low is above the high.
 */
export function et0Hargreaves(lat: number | null, month: number, tmax: number | null, tmin: number | null): number | null {
  if (lat === null || tmax === null || tmin === null || tmin > tmax) return null;
  const ra = extraterrestrialRadiation(lat, midMonthDay(month)) * MJ_TO_MM;
  const tmean = (tmax + tmin) / 2;
  return Math.max(0, 0.0023 * ra * (tmean + 17.8) * Math.sqrt(tmax - tmin));
}

// ---- the garden: crops, beds and drip lines ------------------------------------

/**
 * Crop groups and their mid-season crop coefficient Kc, from USDA NRCS NEH
 * Part 623, Chapter 2 (1993). Table 2-20 gives peak basal coefficients (Kcp,
 * moderate wind) for a humid and an arid climate; each value here is the
 * mean of the group's crops, taking each crop halfway between humid and arid,
 * rounded to 0.05. Basal (dry soil surface) values suit drip, which keeps
 * most of the surface dry.
 *   greens: lettuce 0.95/1.00, spinach 0.95/1.00, crucifers 0.95/1.05   -> 1.00
 *   tomatoes: tomato 1.05/1.20, peppers 0.95/1.05, eggplant 0.95/1.05  -> 1.05
 *   squash: cucumber (fresh market), zucchini, winter squash 0.90/0.95,
 *           melons 1.10/1.15                                           -> 1.00
 *   beans: green beans 0.95/1.00, peas 1.05/1.15                       -> 1.05
 *   roots: table beets 1.00/1.05, carrots 1.00/1.10, radishes 0.80/0.85 -> 0.95
 *   potatoes: potato 1.05/1.15                                         -> 1.10
 *   onions: dry onion 0.95/1.05 (garlic is not listed; taken as onion) -> 1.00
 *   corn: sweet corn 1.05/1.15                                         -> 1.10
 *   strawberries: strawberry 0.70/0.80                                 -> 0.75
 *   bushes: Table 2-27, raspberries in June and July                   -> 1.20
 */
export const CROPS = [
  { id: "greens", kc: 1.0 },
  { id: "tomatoes", kc: 1.05 },
  { id: "squash", kc: 1.0 },
  { id: "beans", kc: 1.05 },
  { id: "roots", kc: 0.95 },
  { id: "potatoes", kc: 1.1 },
  { id: "onions", kc: 1.0 },
  { id: "corn", kc: 1.1 },
  { id: "strawberries", kc: 0.75 },
  { id: "bushes", kc: 1.2 },
] as const;
export type CropId = (typeof CROPS)[number]["id"];

/** Other words a link may use for a crop group (singular, plural, a member). */
const CROP_WORDS: Record<string, CropId> = {
  green: "greens", leafy: "greens", lettuce: "greens", salad: "greens", spinach: "greens", cabbage: "greens", kale: "greens",
  tomato: "tomatoes", pepper: "tomatoes", peppers: "tomatoes", eggplant: "tomatoes", eggplants: "tomatoes",
  cucumber: "squash", cucumbers: "squash", zucchini: "squash", pumpkin: "squash", pumpkins: "squash", melon: "squash", melons: "squash", cucurbits: "squash",
  bean: "beans", pea: "beans", peas: "beans",
  root: "roots", carrot: "roots", carrots: "roots", beet: "roots", beets: "roots",
  potato: "potatoes",
  onion: "onions", garlic: "onions",
  maize: "corn", sweetcorn: "corn",
  strawberry: "strawberries",
  bush: "bushes", berries: "bushes", raspberries: "bushes", currants: "bushes",
};

export function isCropId(x: unknown): x is CropId {
  return typeof x === "string" && CROPS.some((c) => c.id === x);
}

export function kcOf(crop: CropId): number {
  return CROPS.find((c) => c.id === crop)!.kc;
}

/** How much of the water drip delivers reaches the roots. */
export const DRIP_EFFICIENCY = 0.9;
/** The share of the rain on a roof that reaches the tank (splash, evaporation, the first flush). */
export const ROOF_RUNOFF = 0.8;
/** Drip lines counted this far apart: for a bed's suggested number of lines, and in a bed given only by its area. */
export const LINE_SPACING_M = 0.4;
export const SPACINGS_CM = [20, 30, 40] as const;
export const FLOWS_LH = [1, 2, 4] as const;
/** A run longer than this is split into two (morning and evening). */
export const MAX_RUN_MIN = 60;
export const MAX_BEDS = 30;

export type Bed = {
  /** Stable while the household edits the list. */
  id: string;
  /** Given by its sides, or only by its area. */
  by: "size" | "area";
  length: number | null;
  width: number | null;
  area: number | null;
  crop: CropId;
  /** Drip lines along the bed; null: the suggestion for its width. */
  rows: number | null;
};

export type Garden = {
  beds: Bed[];
  lat: number | null;
  month: number;
  tmax: number | null;
  tmin: number | null;
  /** The household's own ET₀ (mm/day), in place of the estimate. */
  et0: number | null;
  /** Rain in the month (mm). */
  rain: number | null;
  spacing: (typeof SPACINGS_CM)[number];
  flow: (typeof FLOWS_LH)[number];
  /** Roof area for the rainwater line (m²). */
  roof: number | null;
};

/** Drip lines suggested for a bed this wide: about one every 40 cm. */
export function suggestedRows(width: number): number {
  return Math.max(1, Math.round(width / LINE_SPACING_M + 1e-9));
}

export function bedArea(b: Bed): number | null {
  if (b.by === "area") return b.area !== null && b.area > 0 ? b.area : null;
  return b.length !== null && b.width !== null && b.length > 0 && b.width > 0 ? b.length * b.width : null;
}

export function bedRows(b: Bed): number | null {
  if (b.by === "area") return null;
  if (b.rows !== null) return b.rows;
  return b.width !== null && b.width > 0 ? suggestedRows(b.width) : null;
}

/** Drippers in a bed: every line along its length, one every `spacingCm`; a bed given by area has lines 40 cm apart. */
export function bedEmitters(b: Bed, spacingCm: number): number {
  const s = spacingCm / 100;
  // The small nudge keeps 1.2 / 0.4 from coming out as 2.9999999999999996.
  if (b.by === "area") {
    const area = bedArea(b);
    return area === null ? 0 : Math.max(1, Math.floor(area / LINE_SPACING_M / s + 1e-9));
  }
  const rows = bedRows(b);
  if (rows === null || b.length === null || b.length <= 0) return 0;
  return rows * Math.max(1, Math.floor(b.length / s + 1e-9));
}

export type BedResult = { id: string; area: number | null; kc: number; litersPerDay: number | null; emitters: number };

export type Tank =
  | { kind: "bucket"; size: 20 }
  | { kind: "drum"; size: 200 }
  | { kind: "tank"; size: 1000 }
  | { kind: "tanks"; size: 1000; count: number };

export type DripResult = {
  /** The estimate from the weather, and the ET₀ used (the household's own if given). */
  et0Estimate: number | null;
  et0: number | null;
  /** Rain in mm a day, spread over the month. */
  rainPerDay: number;
  beds: BedResult[];
  area: number;
  litersPerDay: number | null;
  litersPerWeek: number | null;
  emitters: number;
  flowLh: number;
  /** Minutes a day in one or two runs; null without drippers or a need. */
  runs: { count: 1 | 2; minutes: number } | null;
  tank: Tank | null;
  /** Liters the roof collects in the month; null without a roof or rain. */
  roofLiters: number | null;
  /** How many days of the garden's water that is. */
  roofDays: number | null;
};

export function tankFor(liters: number): Tank {
  if (liters <= 20) return { kind: "bucket", size: 20 };
  if (liters <= 200) return { kind: "drum", size: 200 };
  if (liters <= 1000) return { kind: "tank", size: 1000 };
  return { kind: "tanks", size: 1000, count: roundUp(liters / 1000) };
}

/**
 * Water a day for a bed = (ET₀ × Kc − rain a day) / drip efficiency × area,
 * and never below zero (1 mm on 1 m² is 1 liter).
 */
export function dripPlan(g: Garden): DripResult {
  const et0Estimate = et0Hargreaves(g.lat, g.month, g.tmax, g.tmin);
  const et0 = g.et0 ?? et0Estimate;
  const rainPerDay = (g.rain ?? 0) / DAYS_IN_MONTH[g.month - 1];
  const beds = g.beds.map((b): BedResult => {
    const area = bedArea(b);
    const kc = kcOf(b.crop);
    const litersPerDay = area === null || et0 === null ? null : (Math.max(0, et0 * kc - rainPerDay) / DRIP_EFFICIENCY) * area;
    return { id: b.id, area, kc, litersPerDay, emitters: bedEmitters(b, g.spacing) };
  });
  const sized = beds.filter((b) => b.area !== null);
  const area = sized.reduce((s, b) => s + (b.area ?? 0), 0);
  const litersPerDay = et0 === null || sized.length === 0 ? null : sized.reduce((s, b) => s + (b.litersPerDay ?? 0), 0);
  const emitters = beds.reduce((s, b) => s + b.emitters, 0);
  const flowLh = emitters * g.flow;
  let runs: DripResult["runs"] = null;
  if (litersPerDay !== null && litersPerDay > 0 && flowLh > 0) {
    const minutes = (litersPerDay / flowLh) * 60;
    runs = minutes > MAX_RUN_MIN ? { count: 2, minutes: roundUp(minutes / 2) } : { count: 1, minutes: roundUp(minutes) };
  }
  const roofLiters = g.roof !== null && g.roof > 0 && g.rain !== null ? g.roof * g.rain * ROOF_RUNOFF : null;
  return {
    et0Estimate,
    et0,
    rainPerDay,
    beds,
    area,
    litersPerDay,
    litersPerWeek: litersPerDay === null ? null : litersPerDay * 7,
    emitters,
    flowLh,
    runs,
    tank: litersPerDay !== null && litersPerDay > 0 ? tankFor(litersPerDay) : null,
    roofLiters,
    roofDays: roofLiters !== null && litersPerDay !== null && litersPerDay > 0 ? Math.floor(roofLiters / litersPerDay) : null,
  };
}

// ---- the household's document ---------------------------------------------------

export type WaterPlan = { v: 1; drink: Drink; garden: Garden };

export function newBedId(): string {
  return Math.random().toString(36).slice(2, 10);
}

export function newBed(crop: CropId = "tomatoes"): Bed {
  return { id: newBedId(), by: "size", length: null, width: null, area: null, crop, rows: null };
}

export function defaultPlan(): WaterPlan {
  return {
    v: 1,
    drink: { people: 2, children: 0, smallPets: 0, largePets: 0, days: 3 },
    garden: { beds: [newBed()], lat: null, month: 7, tmax: null, tmin: null, et0: null, rain: null, spacing: 30, flow: 2, roof: null },
  };
}

/** Limits for every number the household can enter. */
export const LIMITS = {
  people: [0, 100],
  children: [0, 100],
  smallPets: [0, 50],
  largePets: [0, 50],
  days: [1, 365],
  length: [0, 1000],
  width: [0, 100],
  area: [0, 100000],
  rows: [1, 50],
  lat: [-66.5, 66.5],
  temp: [-40, 60],
  et0: [0, 20],
  rain: [0, 2000],
  roof: [0, 10000],
} as const;

function num(x: unknown, [lo, hi]: readonly [number, number], int = false): number | null {
  if (typeof x !== "number" || !Number.isFinite(x)) return null;
  const v = Math.min(hi, Math.max(lo, x));
  return int ? Math.round(v) : v;
}

function obj(x: unknown): Record<string, unknown> {
  return typeof x === "object" && x !== null && !Array.isArray(x) ? (x as Record<string, unknown>) : {};
}

/**
 * A plan as saved (on the hub or on this device), checked: anything missing,
 * damaged or out of range becomes the default, so the screen always works.
 */
export function parsePlan(x: unknown): WaterPlan {
  const base = defaultPlan();
  const o = obj(x);
  const d = obj(o.drink);
  const g = obj(o.garden);
  const drink: Drink = {
    people: num(d.people, LIMITS.people, true) ?? base.drink.people,
    children: num(d.children, LIMITS.children, true) ?? 0,
    smallPets: num(d.smallPets, LIMITS.smallPets, true) ?? 0,
    largePets: num(d.largePets, LIMITS.largePets, true) ?? 0,
    days: num(d.days, LIMITS.days, true) ?? base.drink.days,
  };
  const beds = Array.isArray(g.beds)
    ? g.beds.slice(0, MAX_BEDS).map((b): Bed => {
        const bo = obj(b);
        return {
          id: typeof bo.id === "string" && /^[\w-]{1,32}$/.test(bo.id) ? bo.id : newBedId(),
          by: bo.by === "area" ? "area" : "size",
          length: num(bo.length, LIMITS.length),
          width: num(bo.width, LIMITS.width),
          area: num(bo.area, LIMITS.area),
          crop: isCropId(bo.crop) ? bo.crop : "tomatoes",
          rows: num(bo.rows, LIMITS.rows, true),
        };
      })
    : base.garden.beds;
  const month = num(g.month, [1, 12], true) ?? base.garden.month;
  const garden: Garden = {
    beds,
    lat: num(g.lat, LIMITS.lat),
    month,
    tmax: num(g.tmax, LIMITS.temp),
    tmin: num(g.tmin, LIMITS.temp),
    et0: num(g.et0, LIMITS.et0),
    rain: num(g.rain, LIMITS.rain),
    spacing: (SPACINGS_CM as readonly unknown[]).includes(g.spacing) ? (g.spacing as Garden["spacing"]) : base.garden.spacing,
    flow: (FLOWS_LH as readonly unknown[]).includes(g.flow) ? (g.flow as Garden["flow"]) : base.garden.flow,
    roof: num(g.roof, LIMITS.roof),
  };
  return { v: 1, drink, garden };
}

// ---- the deep link ---------------------------------------------------------------

export type Part = "drink" | "drip";

/** The query of a link to the calculator ("#tools/water?people=4", "#water?..."), or null for any other address. */
export function linkQuery(hash: string): string | null {
  const m = /^#(?:tools\/)?water(?:\/drip)?\?(.*)$/.exec(hash);
  return m ? m[1] : null;
}

/** One bed from a link: "3x1.2:tomatoes", "6:potatoes" or "3x1.2:tomatoes:2". */
export function parseBed(text: string): Bed | null {
  const [size, cropWord, rowsText] = text.trim().toLowerCase().split(":");
  if (!size) return null;
  const word = (cropWord ?? "").trim();
  const crop: CropId | null = word === "" ? "tomatoes" : isCropId(word) ? word : (CROP_WORDS[word] ?? null);
  if (crop === null) return null;
  const n = (s: string) => {
    const v = Number(s.trim().replace(",", "."));
    return s.trim() !== "" && Number.isFinite(v) && v > 0 ? v : null;
  };
  const rows = rowsText === undefined ? null : n(rowsText);
  if (rowsText !== undefined && rows === null) return null;
  const sides = size.replace(/m2$|m²$/, "").split(/[x×*]/);
  const bed = newBed(crop);
  if (sides.length === 2) {
    const [l, w] = sides.map(n);
    if (l === null || w === null) return null;
    return { ...bed, length: Math.min(l, LIMITS.length[1]), width: Math.min(w, LIMITS.width[1]), rows: rows === null ? null : Math.min(Math.max(1, Math.round(rows)), LIMITS.rows[1]) };
  }
  const area = sides.length === 1 ? n(sides[0]) : null;
  return area === null ? null : { ...bed, by: "area", area: Math.min(area, LIMITS.area[1]) };
}

/**
 * The plan with what a link gives, and the part to show (null: the link did
 * not say). Only what the link names changes; beds replace the list.
 */
export function applyLink(plan: WaterPlan, query: string): { plan: WaterPlan; part: Part | null; changed: boolean } {
  const q = new URLSearchParams(query);
  const drink = { ...plan.drink };
  const garden = { ...plan.garden };
  let changed = false;
  const read = (key: string, limits: readonly [number, number], int = false): number | null => {
    const raw = q.get(key);
    if (raw === null || raw.trim() === "") return null;
    return num(Number(raw.replace(",", ".")), limits, int);
  };
  const setDrink = (field: keyof Drink, key: string) => {
    const v = read(key, LIMITS[field], true);
    if (v !== null) {
      drink[field] = v;
      changed = true;
    }
  };
  setDrink("people", "people");
  setDrink("children", "children");
  setDrink("smallPets", "smallpets");
  setDrink("largePets", "largepets");
  setDrink("days", "days");
  const setGarden = <K extends "lat" | "tmax" | "tmin" | "et0" | "rain" | "roof">(field: K, limits: readonly [number, number]) => {
    const v = read(field, limits);
    if (v !== null) {
      garden[field] = v;
      changed = true;
    }
  };
  setGarden("lat", LIMITS.lat);
  setGarden("tmax", LIMITS.temp);
  setGarden("tmin", LIMITS.temp);
  setGarden("et0", LIMITS.et0);
  setGarden("rain", LIMITS.rain);
  setGarden("roof", LIMITS.roof);
  const month = read("month", [1, 12], true);
  if (month !== null) {
    garden.month = month;
    changed = true;
  }
  const spacing = Number(q.get("spacing"));
  if ((SPACINGS_CM as readonly number[]).includes(spacing)) {
    garden.spacing = spacing as Garden["spacing"];
    changed = true;
  }
  const flow = Number(q.get("flow"));
  if ((FLOWS_LH as readonly number[]).includes(flow)) {
    garden.flow = flow as Garden["flow"];
    changed = true;
  }
  const bedsText = q.get("beds");
  if (bedsText) {
    const beds = bedsText.split(",").map(parseBed).filter((b): b is Bed => b !== null).slice(0, MAX_BEDS);
    if (beds.length > 0) {
      garden.beds = beds;
      changed = true;
    }
  }
  // The part the link names; without one, the part its values are for.
  const partText = q.get("part");
  const has = (keys: string[]) => keys.some((k) => q.has(k));
  const part: Part | null =
    partText === "drip" || partText === "garden"
      ? "drip"
      : partText === "drink"
        ? "drink"
        : has(["beds", "lat", "month", "tmax", "tmin", "et0", "rain", "spacing", "flow", "roof"])
          ? "drip"
          : has(["people", "children", "smallpets", "largepets", "days"])
            ? "drink"
            : null;
  return { plan: { v: 1, drink, garden }, part, changed };
}

// ---- small helpers ---------------------------------------------------------------

/** Up to the next whole number, ignoring floating-point dust (3.0000000001 is 3). */
export function roundUp(n: number): number {
  return Math.ceil(n - 1e-9);
}
