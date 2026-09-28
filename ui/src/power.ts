import type { Key } from "./i18n";

/**
 * The power calculator (Tools › Power calculator): how much battery, solar
 * and inverter a SMALL backup system needs for the appliances a household
 * wants to keep running in a power cut. Pure functions without React or the
 * network: screens/Power.tsx shows them, e2e/unit/power.check.mts checks them.
 *
 * The method is the usual one for stand-alone solar systems (after Sandia
 * National Laboratories, "Stand-Alone Photovoltaic Systems: A Handbook of
 * Recommended Design Practices", SAND87-7023, https://www.osti.gov/biblio/6959528):
 *
 *   energy a day at the appliances   E  = Σ number × watts × hours
 *                                          (fridges and freezers: number × their energy a day)
 *   from the battery a day           B  = E(AC) ÷ inverter efficiency + E(DC)
 *   battery, nominal                 C  = B × days ÷ usable part;   Ah = C ÷ volts
 *   panels to keep up every day      P  = B ÷ (sun hours × PANEL_DERATE × charging efficiency)
 *   panels to refill in one day      P1 = B × (days + 1) ÷ (the same)
 *   inverter                         continuous ≥ 1.25 × Σ AC watts;
 *                                    peak ≥ Σ AC watts + the largest motor's start-up extra
 *
 * Typical wattages (conservative: rounded up, or the upper end of a range):
 *  - Source: U.S. Department of Energy, Energy Saver, "Estimating Appliance
 *    and Home Electronic Energy Use" (public domain; the page was taken down
 *    in July 2026, archived at https://web.archive.org/web/2025/https://www.energy.gov/energysaver/estimating-appliance-and-home-electronic-energy-use):
 *    typical wattages list (laptop 50 W, clock radio 10 W, TV 19" 65–110 W,
 *    window fan 55–250 W / ceiling fan 65–175 W, microwave 750–1100 W,
 *    electric blanket 60/100 W, deep-well water pump 250–1100 W). The same
 *    list is in Virginia Cooperative Extension publication 2901-9014 (2020).
 *  - Refrigerators: U.S. DOE Federal Energy Management Program, "Purchasing
 *    Energy-Efficient Residential Refrigerators": an 18.1 cu ft top-freezer
 *    uses 363.5 kWh a year (ENERGY STAR) to 404 kWh (less efficient), about
 *    1.1 kWh a day; ENERGY STAR (U.S. EPA): an old refrigerator uses about
 *    20% more. We use 1.2 kWh a day. Small (compact) refrigerators: ENERGY
 *    STAR certified ones use about 100–125 kWh a year; we use 0.6 kWh a day.
 *  - Freezers: ENERGY STAR (U.S. EPA), "Freezers": a certified chest freezer
 *    uses about 215 kWh a year and an upright one about 395 kWh (0.6 and 1.1
 *    kWh a day); we use 0.7 and 1.2 kWh a day.
 *  - Wi-Fi router and modem: NRDC, "Small Network Equipment Energy
 *    Consumption in U.S. Homes" (2013): 5.7 W for a modem and 5.7 W for a
 *    router on average; we use 12 W for both, all day.
 *  - LED bulb: U.S. DOE Energy Saver, "Lighting Choices" (2021): a 1100 lumen
 *    LED (as bright as a 75 W incandescent) uses as little as 9 W; we use 10 W.
 *  - CPAP: ResMed, "Battery Guide" (198103/7, 2018): an AirSense 10 draws
 *    0.7–1.7 A at 12 V without a humidifier (about 8–20 W) and up to 5.6 A
 *    (about 67 W) with a heated humidifier and tube; we use 25 W and 65 W.
 *  - Phone: a phone battery holds about 12–20 Wh; with charger losses one
 *    charge a day is about 20 Wh (10 W for 2 hours).
 *  - Start-up surge of motors: generator sizing guides (e.g. Honda Power
 *    Equipment) list a refrigerator's starting watts at about 3–4 times its
 *    running watts, and pumps at about 3 times; we use 5× for refrigerators
 *    and freezers, 3× for pumps, 2× for fans.
 *
 * Battery: the usable part (depth of discharge) is the practical rule of
 * energypedia.info, "Battery in a Photovoltaic Power Supply System –
 * Standards" and "Batteries" (CC BY-SA 4.0, https://energypedia.info/wiki/Batteries):
 * lead-acid about 50%, LiFePO4 about 80% (3,000–5,000 cycles).
 *
 * Sun: PVGIS 5.2 (European Commission, Joint Research Centre,
 * https://re.jrc.ec.europa.eu/pvg_tools/; "PVGIS © European Communities,
 * 2001-2026", free to use), monthly irradiation on a panel facing south at
 * the best fixed angle, averaged over 2005–2020 (PVGIS-SARAH2) and rounded
 * down to 0.1 h. kWh/m² a day equals peak sun hours. For places not in the
 * table, NASA POWER (https://power.larc.nasa.gov/) has the same for the
 * whole world.
 *
 * Safety lines on the screen: U.S. Fire Administration (charge lithium-ion
 * batteries only between 0 °C and 40 °C), OSHA (a generator is never
 * connected to house wiring without a transfer switch: it feeds power back
 * into the grid) and Ready.gov (generators only outdoors, at least 20 ft /
 * 6 m from windows and doors: carbon monoxide); all U.S. government, public
 * domain.
 *
 * Deep link (so the assistant can open the calculator with a list):
 *
 *   #power?items=fridge:1,lights-led:6:5&days=3&battery=lifepo4&volts=12
 *
 *   items    comma-separated id:number[:hours[:watts]], ids from APPLIANCES;
 *            unknown ids are skipped; hours are ignored for appliances that
 *            switch on and off by themselves (refrigerators, freezers)
 *   days     days without grid (0.5–30)      battery  lifepo4 | lead
 *   volts    12 | 24 | 48                    region   an id from SUN_REGIONS
 *   month    1–12                            sun      peak sun hours (0.1–12)
 *
 * "#tools/power?..." works too. What the link does not say stays as saved.
 * The screen shows such a list as a draft: it replaces the household's
 * saved list only when the person chooses to save it.
 */

export type BatteryType = "lifepo4" | "lead";
export type Volts = 12 | 24 | 48;

/** One line of the list: an appliance from APPLIANCES, or the household's own ("custom"). */
export type Line = {
  /** A stable key for the screen (not shown). */
  k: string;
  /** An id from APPLIANCES, or "custom". */
  id: string;
  /** The name of a custom line; appliances from the list are named in the app's language. */
  name?: string;
  /** How many. */
  qty: number;
  /** Running power of one, in watts. */
  watts: number;
  /** Hours a day it runs (not used when whDay is set). */
  hours: number;
  /** Energy a day of one, in Wh, for appliances that switch on and off by themselves. */
  whDay?: number;
  /** Runs straight from the battery (DC), not through the inverter. */
  dc?: boolean;
};

export type Plan = {
  v: 1;
  lines: Line[];
  /** Days the battery alone must cover (no grid and no sun). */
  days: number;
  battery: BatteryType;
  /** The part of the battery's capacity that may be used, 0.1–1. */
  usable: number;
  volts: Volts;
  /** Inverter efficiency for AC appliances, 0.5–1. */
  inverterEff: number;
  /** An id from SUN_REGIONS; the month's sun hours come from its table. */
  region: string;
  month: number;
  /** Peak sun hours a day used for the panels (the table's, or the person's own). */
  sunHours: number;
};

export type ApplianceGroup = "kitchen" | "devices" | "health" | "water";

export type Appliance = {
  id: string;
  name: Key;
  group: ApplianceGroup;
  /** Running power of one, W. */
  watts: number;
  /** Hours a day, by default. */
  hours: number;
  /** Energy a day of one, Wh, for appliances that switch on and off by themselves. */
  whDay?: number;
  /** Start-up power as a multiple of the running power (motors). */
  surge?: number;
};

/** Built-in appliances with typical (conservative) wattages; see the sources above. */
export const APPLIANCES: readonly Appliance[] = [
  { id: "fridge", name: "pwFridge", group: "kitchen", watts: 200, hours: 24, whDay: 1200, surge: 5 },
  { id: "fridge-small", name: "pwFridgeSmall", group: "kitchen", watts: 100, hours: 24, whDay: 600, surge: 5 },
  { id: "freezer", name: "pwFreezer", group: "kitchen", watts: 150, hours: 24, whDay: 700, surge: 5 },
  { id: "freezer-upright", name: "pwFreezerUpright", group: "kitchen", watts: 150, hours: 24, whDay: 1200, surge: 5 },
  { id: "microwave", name: "pwMicrowave", group: "kitchen", watts: 1100, hours: 0.25 },
  { id: "lights-led", name: "pwLightsLed", group: "devices", watts: 10, hours: 5 },
  { id: "phone", name: "pwPhone", group: "devices", watts: 10, hours: 2 },
  { id: "laptop", name: "pwLaptop", group: "devices", watts: 50, hours: 4 },
  { id: "router", name: "pwRouter", group: "devices", watts: 12, hours: 24 },
  { id: "tv", name: "pwTv", group: "devices", watts: 65, hours: 3 },
  { id: "radio", name: "pwRadio", group: "devices", watts: 10, hours: 4 },
  { id: "cpap", name: "pwCpap", group: "health", watts: 25, hours: 8 },
  { id: "cpap-humid", name: "pwCpapHumid", group: "health", watts: 65, hours: 8 },
  { id: "fan", name: "pwFan", group: "health", watts: 65, hours: 6, surge: 2 },
  { id: "blanket", name: "pwBlanket", group: "health", watts: 100, hours: 2 },
  { id: "pump", name: "pwPump", group: "water", watts: 800, hours: 1, surge: 3 },
];

export function applianceOf(id: string): Appliance | undefined {
  return APPLIANCES.find((a) => a.id === id);
}

/**
 * Peak sun hours a day, January to December (PVGIS, see above), for places
 * in Serbia first and a few others. The first is the default. Names are in
 * i18n; anywhere else, the person types the sun hours of their own place.
 */
export const SUN_REGIONS: readonly { id: string; name: Key; serbia: boolean; hours: readonly number[] }[] = [
  { id: "belgrade", name: "pwRegionBelgrade", serbia: true, hours: [2.0, 2.7, 4.1, 5.3, 5.5, 5.9, 6.3, 6.1, 5.0, 4.0, 2.8, 1.8] },
  { id: "novi-sad", name: "pwRegionNoviSad", serbia: true, hours: [2.0, 2.7, 4.1, 5.2, 5.5, 5.8, 6.4, 6.2, 5.0, 4.0, 2.8, 1.8] },
  { id: "nis", name: "pwRegionNis", serbia: true, hours: [2.1, 2.9, 4.1, 5.2, 5.3, 6.0, 6.5, 6.4, 5.1, 4.1, 2.9, 1.9] },
  { id: "kragujevac", name: "pwRegionKragujevac", serbia: true, hours: [2.1, 2.7, 4.0, 5.0, 5.2, 5.7, 6.2, 6.1, 4.9, 3.9, 2.9, 2.0] },
  { id: "sarajevo", name: "pwRegionSarajevo", serbia: false, hours: [2.1, 2.6, 3.5, 4.5, 4.7, 5.4, 5.9, 5.8, 4.7, 3.8, 2.6, 1.9] },
  { id: "zagreb", name: "pwRegionZagreb", serbia: false, hours: [2.1, 2.7, 4.1, 5.3, 5.5, 6.0, 6.2, 5.9, 4.7, 3.6, 2.1, 1.7] },
  { id: "vienna", name: "pwRegionVienna", serbia: false, hours: [1.8, 2.8, 4.1, 5.5, 5.3, 5.6, 5.8, 5.5, 4.7, 3.2, 1.9, 1.6] },
  { id: "berlin", name: "pwRegionBerlin", serbia: false, hours: [1.2, 2.3, 3.4, 5.1, 5.2, 5.5, 5.3, 5.0, 4.3, 2.8, 1.5, 1.1] },
  { id: "london", name: "pwRegionLondon", serbia: false, hours: [1.5, 2.2, 3.4, 4.7, 4.8, 5.0, 5.0, 4.4, 4.0, 2.7, 1.9, 1.4] },
  { id: "athens", name: "pwRegionAthens", serbia: false, hours: [3.7, 4.4, 5.6, 6.4, 6.6, 7.1, 7.4, 7.4, 6.4, 5.1, 4.2, 3.4] },
];

/** The sources as the screen names them (see the comment at the top), and what each is for. */
export const SOURCES: readonly { name: string; url?: string; what: Key }[] = [
  { name: "U.S. Department of Energy", url: "https://web.archive.org/web/2025/https://www.energy.gov/energysaver/estimating-appliance-and-home-electronic-energy-use", what: "pwSrcWatts" },
  { name: "ENERGY STAR", url: "https://www.energystar.gov/products/freezers", what: "pwSrcFridges" },
  { name: "U.S. DOE FEMP", url: "https://www.energy.gov/cmei/femp/purchasing-energy-efficient-residential-refrigerators", what: "pwSrcFridges" },
  { name: "NRDC", url: "https://www.nrdc.org/sites/default/files/residential-network-IP.pdf", what: "pwSrcRouters" },
  { name: "ResMed", url: "https://document.resmed.com/documents/articles/198103_battery-guide_glo_eng.pdf", what: "pwSrcCpap" },
  { name: "energypedia.info (CC BY-SA 4.0)", url: "https://energypedia.info/wiki/Battery_in_a_Photovoltaic_Power_Supply_System_-_Standards", what: "pwSrcBattery" },
  { name: "PVGIS © European Communities, 2001-2026", url: "https://re.jrc.ec.europa.eu/pvg_tools/", what: "pwSrcSun" },
  { name: "Sandia National Laboratories", url: "https://www.osti.gov/biblio/6959528", what: "pwSrcMethod" },
  { name: "USFA", url: "https://www.usfa.fema.gov/prevention/home-fires/prevent-fires/batteries/", what: "pwSrcSafety" },
  { name: "OSHA", what: "pwSrcSafety" },
  { name: "Ready.gov", url: "https://www.ready.gov/power-outages", what: "pwSrcSafety" },
];

export function regionOf(id: string) {
  return SUN_REGIONS.find((r) => r.id === id) ?? SUN_REGIONS[0];
}

/** The month (1–12) with the least sun in a region: the safe default for panels. */
export function worstMonth(regionId: string): number {
  const h = regionOf(regionId).hours;
  return h.indexOf(Math.min(...h)) + 1;
}

export function sunOf(regionId: string, month: number): number {
  return regionOf(regionId).hours[month - 1] ?? regionOf(regionId).hours[0];
}

/** Real-world output of panels against their rating: heat, dust, wiring and the charge controller take about a quarter. */
export const PANEL_DERATE = 0.75;
/** Energy that ends up stored when charging (the rest is lost as heat). */
export const CHARGE_EFF: Record<BatteryType, number> = { lifepo4: 0.95, lead: 0.85 };
/** The usable part of a battery by default: LiFePO4 about 80–90% (the safe end), lead-acid and AGM about 50%. */
export const USABLE: Record<BatteryType, number> = { lifepo4: 0.8, lead: 0.5 };
export const VOLTS: readonly Volts[] = [12, 24, 48];
export const DAY_CHOICES = [1, 3, 7] as const;
export const DEFAULT_INVERTER_EFF = 0.85;
/** Common inverter sizes, W. */
export const INVERTER_SIZES = [300, 500, 600, 800, 1000, 1200, 1500, 2000, 2500, 3000, 4000, 5000] as const;
/** The inverter's continuous rating, above what runs at once. */
export const INVERTER_HEADROOM = 1.25;
/** Inverters give about twice their rating for a moment (a motor starting). */
export const INVERTER_PEAK = 2;
/** Above this current (A), a higher system voltage is worth it (thinner cables, smaller fuses). */
export const HIGH_CURRENT = 125;
/** Above this (Wh, nominal), it is not a small backup system any more. */
export const BIG_SYSTEM_WH = 20000;
export const MAX_LINES = 60;
/** Panels are suggested in steps of this many watts. */
export const PANEL_STEP = 50;

const LIMITS = {
  qty: [0, 100],
  watts: [0, 10000],
  hours: [0, 24],
  whDay: [0, 50000],
  days: [0.5, 30],
  usable: [0.1, 1],
  inverterEff: [0.5, 1],
  sunHours: [0.1, 12],
} as const;

function clamp(v: unknown, [lo, hi]: readonly [number, number], fallback: number): number {
  const n = typeof v === "number" ? v : typeof v === "string" ? Number(v) : NaN;
  if (!Number.isFinite(n)) return fallback;
  return Math.min(hi, Math.max(lo, n));
}

export function newKey(): string {
  return Math.random().toString(36).slice(2, 10);
}

/** A new line for an appliance from the list, with its typical values (or an empty custom line). */
export function lineFor(id: string, qty = 1): Line {
  const a = applianceOf(id);
  if (!a) return { k: newKey(), id: "custom", name: "", qty, watts: 0, hours: 1 };
  const line: Line = { k: newKey(), id: a.id, qty, watts: a.watts, hours: a.hours };
  if (a.whDay !== undefined) line.whDay = a.whDay;
  return line;
}

export function defaultPlan(): Plan {
  const region = SUN_REGIONS[0].id;
  const month = worstMonth(region);
  return {
    v: 1,
    lines: [],
    days: 1,
    battery: "lifepo4",
    usable: USABLE.lifepo4,
    volts: 12,
    inverterEff: DEFAULT_INVERTER_EFF,
    region,
    month,
    sunHours: sunOf(region, month),
  };
}

/** A usual list to start from: a refrigerator, four LED bulbs, three phones, the router and a laptop. */
export function typicalLines(): Line[] {
  return [lineFor("fridge"), lineFor("lights-led", 4), lineFor("phone", 3), lineFor("router"), lineFor("laptop")];
}

function normalizeLine(raw: unknown): Line | null {
  if (!raw || typeof raw !== "object") return null;
  const r = raw as Record<string, unknown>;
  const a = typeof r.id === "string" ? applianceOf(r.id) : undefined;
  const name = typeof r.name === "string" ? r.name.slice(0, 60) : "";
  if (!a && r.id !== "custom") return null;
  const line: Line = {
    k: typeof r.k === "string" && /^[\w-]{1,20}$/.test(r.k) ? r.k : newKey(),
    id: a ? a.id : "custom",
    qty: Math.round(clamp(r.qty, LIMITS.qty, 1)),
    watts: clamp(r.watts, LIMITS.watts, a?.watts ?? 0),
    hours: clamp(r.hours, LIMITS.hours, a?.hours ?? 1),
  };
  if (!a) line.name = name;
  // Only appliances that switch on and off by themselves are counted by their energy a day.
  if (a?.whDay !== undefined) line.whDay = clamp(r.whDay, LIMITS.whDay, a.whDay);
  if (r.dc === true) line.dc = true;
  return line;
}

/** A plan as saved on the hub (or anything else), made safe to calculate with. */
export function normalizePlan(raw: unknown): Plan {
  const d = defaultPlan();
  if (!raw || typeof raw !== "object") return d;
  const r = raw as Record<string, unknown>;
  const battery: BatteryType = r.battery === "lead" ? "lead" : "lifepo4";
  const region = typeof r.region === "string" && SUN_REGIONS.some((x) => x.id === r.region) ? r.region : d.region;
  const month = Math.round(clamp(r.month, [1, 12], worstMonth(region)));
  const volts = VOLTS.includes(r.volts as Volts) ? (r.volts as Volts) : d.volts;
  const lines = Array.isArray(r.lines) ? r.lines.map(normalizeLine).filter((l): l is Line => l !== null).slice(0, MAX_LINES) : [];
  return {
    v: 1,
    lines,
    days: clamp(r.days, LIMITS.days, d.days),
    battery,
    usable: clamp(r.usable, LIMITS.usable, USABLE[battery]),
    volts,
    inverterEff: clamp(r.inverterEff, LIMITS.inverterEff, d.inverterEff),
    region,
    month,
    sunHours: clamp(r.sunHours, LIMITS.sunHours, sunOf(region, month)),
  };
}

/** Energy a day of a line at the appliance, Wh. */
export function lineWh(l: Line): number {
  const each = l.whDay != null ? l.whDay : l.watts * l.hours;
  return l.qty * each;
}

export type Pack = {
  /** Batteries in all (series × parallel). */
  count: number;
  unitAh: number;
  unitVolts: number;
  /** Batteries in series for the system voltage (lead-acid: 12 V blocks). */
  series: number;
  /** Strings side by side. */
  parallel: number;
};

/**
 * Batteries that hold `ah` at the system voltage. LiFePO4 is sold at 12, 24
 * and 48 V; lead-acid and AGM in 12 V blocks, put in series for 24 and 48 V.
 * 50 Ah for small needs, else 100 Ah blocks (up to four side by side), else
 * 200 Ah blocks.
 */
export function batteryPack(ah: number, type: BatteryType, volts: Volts): Pack {
  const unitVolts = type === "lifepo4" ? volts : 12;
  const series = volts / unitVolts;
  const need = Math.max(ah, 0);
  const up = (x: number) => Math.max(1, Math.ceil(x - 1e-9));
  const unitAh = need <= 50 ? 50 : up(need / 100) <= 4 ? 100 : 200;
  const parallel = up(need / unitAh);
  return { count: series * parallel, unitAh, unitVolts, series, parallel };
}

/** The smallest common inverter for these loads, or null when none of the common sizes will do. */
export function inverterSize(continuousW: number, peakW: number): number | null {
  for (const s of INVERTER_SIZES) {
    if (s >= continuousW && s * INVERTER_PEAK >= peakW) return s;
  }
  return null;
}

export type Result = {
  /** Energy a day of each line (the plan's order), Wh. */
  lineWh: number[];
  /** At the appliances: all, those on the inverter (AC), those on the battery (DC). */
  loadWh: number;
  acWh: number;
  dcWh: number;
  /** Taken from the battery in a day, with the inverter's losses. */
  fromBatteryWh: number;
  /** Nominal battery capacity needed. */
  batteryWh: number;
  batteryAh: number;
  /** The part of batteryWh that is used. */
  usableWh: number;
  pack: Pack;
  /** Panels to cover a day's use with the plan's sun hours, W. */
  keepUpW: number;
  /** Panels to refill a battery emptied over `days` in one sunny day, while also covering that day. */
  refillW: number;
  /** keepUpW rounded up to the next PANEL_STEP. */
  panelsW: number;
  /** Stored in the battery a day per watt of panels, Wh. */
  whPerPanelW: number;
  /** AC appliances all on at once, W; null without AC appliances. */
  inverter: { loadW: number; continuousW: number; peakW: number; sizeW: number | null; motor: string | null } | null;
  /** Running power of the DC appliances all on at once, W. */
  dcW: number;
  /** The most the battery gives: the inverter at its rating and every DC appliance, A. */
  maxCurrentA: number;
  /** The lowest system voltage that keeps maxCurrentA under HIGH_CURRENT, when the chosen one does not. */
  betterVolts: Volts | null;
  big: boolean;
};

export function calculate(p: Plan): Result {
  let acWh = 0;
  let dcWh = 0;
  let acW = 0;
  let dcW = 0;
  let surgeExtra = 0;
  let motor: string | null = null;
  const perLine: number[] = [];
  for (const l of p.lines) {
    const wh = lineWh(l);
    perLine.push(wh);
    const w = l.qty * l.watts;
    if (l.dc) {
      dcWh += wh;
      dcW += w;
      continue;
    }
    acWh += wh;
    acW += w;
    // One motor starts at a time: the largest start-up extra counts.
    const surge = applianceOf(l.id)?.surge ?? 1;
    const extra = l.qty > 0 ? (surge - 1) * l.watts : 0;
    if (extra > surgeExtra) {
      surgeExtra = extra;
      motor = l.id;
    }
  }
  const fromBatteryWh = acWh / p.inverterEff + dcWh;
  const batteryWh = (fromBatteryWh * p.days) / p.usable;
  const batteryAh = batteryWh / p.volts;
  const whPerPanelW = p.sunHours * PANEL_DERATE * CHARGE_EFF[p.battery];
  const keepUpW = fromBatteryWh / whPerPanelW;
  const refillW = (fromBatteryWh * (p.days + 1)) / whPerPanelW;
  const inverter =
    acW > 0
      ? (() => {
          const continuousW = acW * INVERTER_HEADROOM;
          const peakW = acW + surgeExtra;
          return { loadW: acW, continuousW, peakW, sizeW: inverterSize(continuousW, peakW), motor };
        })()
      : null;
  const inverterW = inverter ? (inverter.sizeW ?? inverter.continuousW) : 0;
  const currentAt = (v: number) => (inverterW / p.inverterEff + dcW) / v;
  const maxCurrentA = currentAt(p.volts);
  const betterVolts = maxCurrentA > HIGH_CURRENT ? (VOLTS.find((v) => v > p.volts && currentAt(v) <= HIGH_CURRENT) ?? null) : null;
  return {
    lineWh: perLine,
    loadWh: acWh + dcWh,
    acWh,
    dcWh,
    fromBatteryWh,
    batteryWh,
    batteryAh,
    usableWh: batteryWh * p.usable,
    pack: batteryPack(batteryAh, p.battery, p.volts),
    keepUpW,
    refillW,
    panelsW: Math.max(PANEL_STEP, Math.ceil(keepUpW / PANEL_STEP - 1e-9) * PANEL_STEP),
    whPerPanelW,
    inverter,
    dcW,
    maxCurrentA,
    betterVolts,
    big: batteryWh > BIG_SYSTEM_WH || (inverter !== null && inverter.sizeW === null),
  };
}

/** Panels needed to keep up in each month of the plan's region, W (January first). */
export function panelsByMonth(p: Plan): number[] {
  const { fromBatteryWh } = calculate(p);
  return regionOf(p.region).hours.map((h) => fromBatteryWh / (h * PANEL_DERATE * CHARGE_EFF[p.battery]));
}

/** The part of an address after "?" for the calculator ("#power?items=..." or "#tools/power?..."); null for any other. */
export function linkQuery(hash: string): string | null {
  const m = /^#(?:tools\/)?power\?(.*)$/.exec(hash);
  return m ? m[1] : null;
}

/** A plan from a deep link's query, on top of `base` (what the link does not say stays). Null when it says nothing usable. */
export function planFromLink(query: string, base: Plan): Plan | null {
  const q = new URLSearchParams(query);
  const raw: Record<string, unknown> = { ...base };
  let said = false;
  const items = q.get("items");
  if (items !== null) {
    const lines: Line[] = [];
    for (const part of items.split(",")) {
      const [id, qty, hours, watts] = part.trim().split(":");
      const a = applianceOf(id);
      if (!a) continue;
      const line = lineFor(a.id, qty ? Math.round(clamp(qty, LIMITS.qty, 1)) : 1);
      if (hours && line.whDay == null) line.hours = clamp(hours, LIMITS.hours, a.hours);
      if (watts) line.watts = clamp(watts, LIMITS.watts, a.watts);
      lines.push(line);
    }
    raw.lines = lines;
    said = lines.length > 0;
  }
  const num = (name: string) => {
    const v = q.get(name);
    return v !== null && v.trim() !== "" && Number.isFinite(Number(v)) ? Number(v) : null;
  };
  const days = num("days");
  if (days !== null) {
    raw.days = days;
    said = true;
  }
  const battery = q.get("battery");
  if (battery === "lifepo4" || battery === "lead") {
    if (battery !== base.battery) raw.usable = USABLE[battery];
    raw.battery = battery;
    said = true;
  }
  const volts = num("volts");
  if (volts !== null && VOLTS.includes(volts as Volts)) {
    raw.volts = volts;
    said = true;
  }
  const region = q.get("region");
  const month = num("month");
  if (region !== null && SUN_REGIONS.some((r) => r.id === region)) {
    raw.region = region;
    raw.month = month ?? worstMonth(region);
    raw.sunHours = sunOf(region, Number(raw.month));
    said = true;
  } else if (month !== null && month >= 1 && month <= 12) {
    raw.month = Math.round(month);
    raw.sunHours = sunOf(base.region, Math.round(month));
    said = true;
  }
  const sun = num("sun");
  if (sun !== null) {
    raw.sunHours = sun;
    said = true;
  }
  return said ? normalizePlan(raw) : null;
}

/** The deep link for a plan (lines from the list only; custom lines cannot be said in a link). */
export function linkFor(p: Plan): string {
  const items = p.lines
    .filter((l) => l.id !== "custom")
    .map((l) => {
      const a = applianceOf(l.id)!;
      const hours = l.whDay == null ? l.hours : a.hours;
      return l.watts !== a.watts ? `${l.id}:${l.qty}:${hours}:${l.watts}` : l.whDay == null && l.hours !== a.hours ? `${l.id}:${l.qty}:${l.hours}` : `${l.id}:${l.qty}`;
    })
    .join(",");
  const q = [`items=${items}`, `days=${p.days}`, `battery=${p.battery}`, `volts=${p.volts}`, `region=${p.region}`, `month=${p.month}`];
  if (p.sunHours !== sunOf(p.region, p.month)) q.push(`sun=${p.sunHours}`);
  return `#power?${q.join("&")}`;
}
