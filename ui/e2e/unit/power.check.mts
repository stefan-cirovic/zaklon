// Run: node e2e/unit/power.check.mts  (Node 23.6+ strips the types)
//
// The power calculator's arithmetic (src/power.ts), checked against sums done
// by hand, and its deep links and saved lists read back safely.
import assert from "node:assert/strict";

const pw = await import("../../src/power.ts");
const { makeT } = await import("../../src/i18n.ts");
type Plan = ReturnType<typeof pw.defaultPlan>;

const near = (actual: number, expected: number, what: string, eps = 1e-6) =>
  assert.ok(Math.abs(actual - expected) <= eps, `${what}: ${actual} is not ${expected}`);

const plan = (over: Partial<Plan>): Plan => ({ ...pw.defaultPlan(), ...over });
const line = (id: string, over: Record<string, unknown> = {}) => ({ ...pw.lineFor(id), ...over });

// ---- the built-in list ---------------------------------------------------------
const ids = pw.APPLIANCES.map((a) => a.id);
assert.equal(new Set(ids).size, ids.length, "appliance ids are unique");
for (const id of ids) assert.match(id, /^[a-z0-9-]+$/, `${id} fits in a link`);
for (const lang of ["en", "sr"] as const) {
  const t = makeT(lang);
  for (const a of pw.APPLIANCES) assert.notEqual(t(a.name), a.name, `${lang}: a name for ${a.id}`);
  for (const r of pw.SUN_REGIONS) assert.notEqual(t(r.name), r.name, `${lang}: a name for the region ${r.id}`);
  for (const s of pw.SOURCES) assert.notEqual(t(s.what), s.what, `${lang}: what ${s.name} is for`);
  assert.equal(t("pwMonths").split(",").length, 12, `${lang}: twelve month names`);
  assert.equal(t("pwMonthsShort").split(",").length, 12, `${lang}: twelve short month names`);
}
for (const s of pw.SOURCES) if (s.url) assert.match(s.url, /^https:\/\//, `${s.name}: a web address`);
for (const r of pw.SUN_REGIONS) {
  assert.equal(r.hours.length, 12, `${r.id}: twelve months`);
  for (const h of r.hours) assert.ok(h > 0.5 && h < 9, `${r.id}: ${h} sun hours is plausible`);
}
// Serbia: about 1.8 hours of full sun in December, over 6 in July; the least sunny month is the default.
assert.ok(pw.SUN_REGIONS.slice(0, 4).every((r) => r.serbia), "Serbian places come first");
const d = pw.defaultPlan();
assert.equal(d.region, "belgrade");
assert.equal(d.month, 12);
assert.equal(d.sunHours, 1.8);
assert.equal(pw.sunOf("belgrade", 7), 6.3);
assert.equal(pw.worstMonth("kragujevac"), 12);
assert.equal(pw.worstMonth("athens"), 12);
assert.deepEqual([d.days, d.battery, d.usable, d.volts, d.inverterEff], [1, "lifepo4", 0.8, 12, 0.85]);

// ---- one line ------------------------------------------------------------------
assert.equal(pw.lineWh(line("fridge")), 1200, "a refrigerator counts by its energy a day, not watts × 24 h");
assert.equal(pw.lineWh(line("fridge", { qty: 2 })), 2400);
assert.equal(pw.lineWh(line("lights-led", { qty: 6, hours: 5 })), 300, "6 × 10 W × 5 h");
assert.equal(pw.lineWh(line("custom")), 0, "a new custom line uses nothing yet");
assert.equal(pw.lineWh({ ...line("custom"), watts: 40, hours: 3, qty: 2 }), 240);

// ---- a whole plan, by hand ------------------------------------------------------
// 6 LED bulbs 5 h (300 Wh) and 2 phones (40 Wh) on the inverter, the router on DC (12 W × 24 h = 288 Wh).
{
  const p = plan({
    lines: [line("lights-led", { qty: 6, hours: 5 }), line("phone", { qty: 2 }), line("router", { dc: true })],
    days: 3,
    sunHours: 4,
  });
  const r = pw.calculate(p);
  assert.deepEqual(r.lineWh, [300, 40, 288]);
  near(r.loadWh, 628, "at the appliances");
  near(r.acWh, 340, "on the inverter");
  near(r.dcWh, 288, "on DC");
  near(r.fromBatteryWh, 340 / 0.85 + 288, "from the battery: AC ÷ 0.85 + DC"); // 688
  near(r.batteryWh, (688 * 3) / 0.8, "battery: 688 Wh × 3 days ÷ 80 % usable"); // 2580
  near(r.batteryAh, 2580 / 12, "at 12 V"); // 215
  near(r.usableWh, 688 * 3, "the used part is three days' worth");
  assert.deepEqual(r.pack, { count: 3, unitAh: 100, unitVolts: 12, series: 1, parallel: 3 }, "3 × 100 Ah 12 V");
  near(r.whPerPanelW, 4 * 0.75 * 0.95, "a watt of panels stores 4 h × 0.75 × 0.95 Wh a day");
  near(r.keepUpW, 688 / 2.85, "panels to keep up"); // 241.4
  assert.equal(r.panelsW, 250, "rounded up to 50 W");
  near(r.refillW, (688 * 4) / 2.85, "refill 3 days in one day while covering that day too");
  assert.ok(r.inverter);
  near(r.inverter.loadW, 80, "AC appliances at once");
  near(r.inverter.continuousW, 100, "with 25 % headroom");
  near(r.inverter.peakW, 80, "no motors: the peak is the load");
  assert.equal(r.inverter.sizeW, 300, "the smallest common inverter");
  assert.equal(r.inverter.motor, null);
  near(r.dcW, 12, "DC appliances at once");
  near(r.maxCurrentA, (300 / 0.85 + 12) / 12, "battery current: the inverter at its rating and the DC loads");
  assert.equal(r.betterVolts, null);
  assert.equal(r.big, false);
}

// A refrigerator's motor: 200 W running, five times that for a moment when it starts.
{
  const r = pw.calculate(plan({ lines: [line("fridge"), line("lights-led", { qty: 4 })] }));
  assert.ok(r.inverter);
  near(r.inverter.loadW, 240, "fridge and bulbs");
  near(r.inverter.continuousW, 300, "240 W × 1.25");
  near(r.inverter.peakW, 240 + 800, "plus the fridge's start-up extra (4 × 200 W)");
  assert.equal(r.inverter.sizeW, 600, "600 W covers 300 W continuous and, at twice its rating, 1040 W peak");
  assert.equal(r.inverter.motor, "fridge");
  // Only one motor starts at a time: the largest extra counts, not their sum.
  const two = pw.calculate(plan({ lines: [line("fridge"), line("freezer"), line("fan")] }));
  near(two.inverter!.peakW, 200 + 150 + 65 + 800, "the refrigerator's start is the largest");
  // A motor on DC does not touch the inverter.
  assert.equal(pw.calculate(plan({ lines: [line("fan", { dc: true })] })).inverter, null, "no inverter without AC appliances");
}

// Lead-acid keeps half, charges less efficiently and comes in 12 V blocks in series.
{
  const p = plan({ lines: [line("fridge")], battery: "lead", usable: 0.5, volts: 24, days: 1, sunHours: 2 });
  const r = pw.calculate(p);
  near(r.fromBatteryWh, 1200 / 0.85, "fridge through the inverter");
  near(r.batteryWh, 1200 / 0.85 / 0.5, "half of a lead-acid battery is usable");
  near(r.batteryAh, 1200 / 0.85 / 0.5 / 24, "at 24 V"); // 117.6
  assert.deepEqual(r.pack, { count: 4, unitAh: 100, unitVolts: 12, series: 2, parallel: 2 }, "2 strings of 2 × 12 V");
  near(r.whPerPanelW, 2 * 0.75 * 0.85, "lead-acid stores 85 % of the charge");
}

// Battery sizes: 50 Ah for small needs, then 100 Ah blocks (up to four), then 200 Ah.
const pack = (ah: number, type: "lifepo4" | "lead" = "lifepo4", v: 12 | 24 | 48 = 12) => {
  const x = pw.batteryPack(ah, type, v);
  return `${x.count}×${x.unitAh}Ah/${x.unitVolts}V`;
};
assert.equal(pack(0), "1×50Ah/12V");
assert.equal(pack(50), "1×50Ah/12V");
assert.equal(pack(50.1), "1×100Ah/12V");
assert.equal(pack(200), "2×100Ah/12V", "exactly two");
assert.equal(pack(200 + 1e-10), "2×100Ah/12V", "a rounding error is not a third battery");
assert.equal(pack(400), "4×100Ah/12V");
assert.equal(pack(401), "3×200Ah/12V");
assert.equal(pack(90, "lifepo4", 48), "1×100Ah/48V", "LiFePO4 is sold at 48 V");
assert.equal(pack(90, "lead", 48), "4×100Ah/12V", "lead-acid: four 12 V blocks in series");

// Inverter sizes.
assert.equal(pw.inverterSize(100, 100), 300);
assert.equal(pw.inverterSize(300, 600), 300, "exactly at the limits");
assert.equal(pw.inverterSize(301, 100), 500);
assert.equal(pw.inverterSize(100, 1300), 800, "the peak decides: 800 W gives 1600 W for a moment");
assert.equal(pw.inverterSize(5001, 0), null, "not a small system");

// Big loads at 12 V: too much current, a higher voltage is suggested.
{
  const r = pw.calculate(plan({ lines: [line("microwave"), line("pump")] }));
  // 1900 W at once × 1.25 = 2375 W continuous; peak 1900 + 2 × 800 = 3500 W -> 2500 W inverter.
  assert.equal(r.inverter!.sizeW, 2500);
  near(r.maxCurrentA, 2500 / 0.85 / 12, "about 245 A at 12 V");
  assert.equal(r.betterVolts, 24, "at 24 V it is about 123 A, under 125 A");
  const huge = pw.calculate(plan({ lines: [line("pump", { qty: 10 })] }));
  assert.equal(huge.inverter!.sizeW, null);
  assert.equal(huge.big, true);
  assert.equal(pw.calculate(plan({ lines: [line("fridge", { qty: 10 })], days: 3 })).big, true, "over 20 kWh of battery");
}
// 24 V is enough for about 2400 W.
assert.equal(pw.calculate(plan({ lines: [line("custom", { watts: 1500, hours: 1 })] })).betterVolts, 24);

// Panels through the year: most in the darkest month.
{
  const p = plan({ lines: [line("fridge")] });
  const months = pw.panelsByMonth(p);
  assert.equal(months.length, 12);
  near(months[11], pw.calculate(p).keepUpW, "December is the plan's own month");
  assert.ok(months[6] < months[11] / 3, "July needs less than a third of December");
}

// ---- what comes from the hub, made safe ----------------------------------------
assert.deepEqual(pw.normalizePlan(null), { ...pw.defaultPlan() });
assert.deepEqual(pw.normalizePlan("nonsense").lines, []);
{
  const n = pw.normalizePlan({
    lines: [
      { k: "a1", id: "fridge", qty: 2, watts: 180, hours: 5, whDay: 900 },
      { id: "lights-led", qty: -3, watts: 1e9, hours: 99, whDay: 5, dc: true },
      { id: "custom", name: "Aquarium pump " + "x".repeat(100), qty: 1.6, watts: "15", hours: 24 },
      { id: "not-a-thing", qty: 1 },
      "junk",
    ],
    days: 100,
    battery: "lead",
    volts: 13,
    inverterEff: 2,
    region: "atlantis",
    month: 7,
    sunHours: -1,
  });
  assert.equal(n.lines.length, 3, "unknown appliances and junk are dropped");
  const [f, l, c] = n.lines;
  assert.deepEqual([f.k, f.qty, f.watts, f.whDay], ["a1", 2, 180, 900]);
  assert.deepEqual([l.qty, l.watts, l.hours, l.whDay, l.dc], [0, 10000, 24, undefined, true], "clamped; energy a day only for appliances that cycle");
  assert.equal(c.name!.length, 60);
  assert.deepEqual([c.qty, c.watts], [2, 15]);
  assert.match(l.k, /^[\w-]+$/, "a line gets a key");
  assert.deepEqual([n.days, n.battery, n.usable, n.volts, n.inverterEff, n.region, n.month, n.sunHours], [30, "lead", 0.5, 12, 1, "belgrade", 7, 0.1]);
}
assert.equal(pw.normalizePlan({ lines: Array.from({ length: 80 }, () => ({ id: "phone" })) }).lines.length, pw.MAX_LINES);
// What the screen saves reads back the same.
{
  const p = plan({ lines: [line("fridge"), line("custom", { name: "Pump", watts: 40, hours: 2 })], days: 3, battery: "lead", usable: 0.5, volts: 24 });
  assert.deepEqual(pw.normalizePlan(JSON.parse(JSON.stringify(p))), p);
}

// ---- deep links ----------------------------------------------------------------
assert.equal(pw.linkQuery("#power?items=fridge:1"), "items=fridge:1");
assert.equal(pw.linkQuery("#tools/power?days=3"), "days=3");
assert.equal(pw.linkQuery("#power"), null);
assert.equal(pw.linkQuery("#supplies?items=fridge:1"), null);
{
  const base = plan({ volts: 24, region: "nis", month: 12, sunHours: 1.9 });
  const p = pw.planFromLink("items=fridge:1,lights-led:6:5,nope:3,phone:2:3:18&days=3&battery=lead", base)!;
  assert.ok(p);
  assert.deepEqual(p.lines.map((l) => [l.id, l.qty, l.hours, l.watts]), [["fridge", 1, 24, 200], ["lights-led", 6, 5, 10], ["phone", 2, 3, 18]]);
  assert.equal(p.lines[0].whDay, 1200, "a refrigerator keeps its energy a day");
  assert.deepEqual([p.days, p.battery, p.usable], [3, "lead", 0.5], "lead-acid comes with its own usable part");
  assert.deepEqual([p.volts, p.region, p.sunHours], [24, "nis", 1.9], "what the link does not say stays");
  near(pw.calculate(p).loadWh, 1200 + 300 + 2 * 18 * 3, "the link's list adds up");
}
assert.equal(pw.planFromLink("items=nope:3", pw.defaultPlan()), null, "nothing usable");
assert.equal(pw.planFromLink("", pw.defaultPlan()), null);
assert.equal(pw.planFromLink("region=athens", pw.defaultPlan())!.sunHours, 3.4, "a region brings its darkest month");
assert.equal(pw.planFromLink("month=7", pw.defaultPlan())!.sunHours, 6.3, "a month in the saved region");
assert.equal(pw.planFromLink("region=vienna&month=6&sun=5", pw.defaultPlan())!.sunHours, 5, "sun hours of one's own win");
assert.equal(pw.planFromLink("days=0&volts=13", pw.defaultPlan())!.days, 0.5, "clamped");
// A plan's link opens the same plan.
{
  const p = plan({
    lines: [line("fridge", { qty: 2 }), line("lights-led", { qty: 6, hours: 4 }), line("tv", { watts: 40 }), line("custom", { name: "Mine" })],
    days: 7,
    battery: "lead",
    usable: 0.5,
    volts: 48,
    region: "zagreb",
    month: 3,
    sunHours: 3.5,
  });
  const link = pw.linkFor(p);
  assert.match(link, /^#power\?items=fridge:2,lights-led:6:4,tv:1:3:40&/);
  const back = pw.planFromLink(pw.linkQuery(link)!, pw.defaultPlan())!;
  const strip = (x: Plan) => ({ ...x, lines: x.lines.filter((l) => l.id !== "custom").map(({ k: _k, ...rest }) => rest) });
  assert.deepEqual(strip(back), strip(p), "custom lines are not in links; everything else comes back");
}

console.log(`power: ${pw.APPLIANCES.length} appliances, ${pw.SUN_REGIONS.length} regions; the arithmetic, saved lists and links check out.`);
