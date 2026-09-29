// Run: node e2e/unit/water.check.mts  (Node 23.6+ strips the types)
//
// The water calculator's arithmetic (src/water.ts): drinking water to store,
// the Hargreaves-Samani reference evapotranspiration against worked examples,
// drip irrigation for beds, the household's saved document and the deep link.
import assert from "node:assert/strict";

const w = await import("../../src/water.ts");

const near = (actual: number | null, expected: number, tolerance: number, what: string) => {
  assert.ok(actual !== null, `${what}: a number`);
  assert.ok(Math.abs(actual - expected) <= tolerance, `${what}: ${actual} is not ${expected} ± ${tolerance}`);
};

// ---- drinking water ---------------------------------------------------------

{
  // 4 people for a week: 4 × 3.8 L × 7 = 106.4 L, rounded up; with hygiene 4 × 15 L × 7 = 420 L.
  const r = w.drinkingWater({ people: 4, children: 0, smallPets: 0, largePets: 0, days: 7 });
  assert.equal(r.drinking, 107);
  assert.equal(r.hygiene, 420);
  near(r.perDay, 15.2, 1e-9, "4 people a day");
  // Children count like adults (Ready.gov gives no smaller amount).
  assert.deepEqual(w.drinkingWater({ people: 2, children: 2, smallPets: 0, largePets: 0, days: 7 }), r);
  // In containers: 107 L is 22 × 5 L, 11 × 10 L, 6 × 20 L or one 200 L drum.
  assert.deepEqual(
    w.containersFor(107).map((c) => [c.size, c.count]),
    [[5, 22], [10, 11], [20, 6], [200, 1]],
  );
  // Exactly full containers are not one too many (floating-point dust).
  assert.deepEqual(w.containersFor(420).map((c) => c.count), [84, 42, 21, 3]);
  assert.deepEqual(w.containersFor(0).map((c) => c.count), [0, 0, 0, 0]);
}
{
  // Pets only: a cat and a large dog for 3 days, (0.65 + 1.95) × 3 = 7.8 L.
  const r = w.drinkingWater({ people: 0, children: 0, smallPets: 1, largePets: 1, days: 3 });
  assert.equal(r.drinking, 8);
  assert.equal(r.hygiene, 8, "pets add their drinking water to the hygiene line too");
  // One person, 3 days: 11.4 L -> 12; hygiene 45 L.
  const one = w.drinkingWater({ people: 1, children: 0, smallPets: 0, largePets: 0, days: 3 });
  assert.deepEqual([one.drinking, one.hygiene], [12, 45]);
}
// CDC: 8 drops (½ mL) of bleach per gallon -> 2 drops per liter; 40 drops in 20 L is 2.5 mL.
assert.equal(w.BLEACH_DROPS_PER_L * 20 * w.BLEACH_ML_PER_DROP, 2.5);

// ---- reference evapotranspiration ---------------------------------------------------

// FAO-56 Example 8: 20°S on 3 September (day 246) has Ra = 32.2 MJ m⁻² day⁻¹.
near(w.extraterrestrialRadiation(-20, 246), 32.2, 0.05, "Ra at 20°S on 3 September");
// Worked by hand: 45°N in the middle of July (day 197): dr = 0.9680, δ = 0.3717 rad,
// ωs = 1.9712 rad, so Ra = 37.586 × 0.9680 × 1.1129 = 40.49 MJ m⁻² day⁻¹ (16.52 mm/day).
assert.equal(w.midMonthDay(7), 197);
assert.equal(w.midMonthDay(1), 15);
assert.equal(w.midMonthDay(12), 349);
near(w.extraterrestrialRadiation(45, 197), 40.49, 0.05, "Ra at 45°N in July");
// Hargreaves-Samani for 45°N in July, high 30 °C and low 17 °C:
// 0.0023 × 16.52 × (23.5 + 17.8) × √13 = 5.66 mm/day.
const july = w.et0Hargreaves(45, 7, 30, 17);
near(july, 5.66, 0.02, "ET₀ at 45°N in July, 30/17 °C");
assert.ok(july! > 5 && july! < 6, "roughly 5-6 mm/day");
// Winter at the same place is far lower; the southern summer is in January.
assert.ok(w.et0Hargreaves(45, 1, 4, -3)! < 1);
assert.ok(w.extraterrestrialRadiation(-34, 15) > w.extraterrestrialRadiation(-34, 197) * 2);
// Nothing to go on, or the low above the high: no estimate.
assert.equal(w.et0Hargreaves(null, 7, 30, 17), null);
assert.equal(w.et0Hargreaves(45, 7, null, 17), null);
assert.equal(w.et0Hargreaves(45, 7, 17, 30), null);
// Polar night gives no radiation rather than NaN.
assert.equal(w.extraterrestrialRadiation(66.5, 355) >= 0, true);
assert.ok(Number.isFinite(w.extraterrestrialRadiation(66.5, 172)));

// ---- drip irrigation ---------------------------------------------------------------

const bed = (over: Partial<import("../../src/water.ts").Bed>) => ({ ...w.newBed("tomatoes"), ...over });
const garden = (over: Partial<import("../../src/water.ts").Garden>) => ({ ...w.defaultPlan().garden, beds: [], ...over });

{
  // A 3 × 1.2 m bed of tomatoes, ET₀ 5 mm/day, drippers every 30 cm giving 2 L/h:
  // 3 lines (one per 40 cm of width) × 10 drippers = 30; 5 × 1.05 / 0.9 × 3.6 m² = 21 L a day,
  // from 60 L/h in 21 minutes; 147 L a week.
  const tomatoes = bed({ length: 3, width: 1.2 });
  assert.equal(w.bedRows(tomatoes), 3);
  const r = w.dripPlan(garden({ beds: [tomatoes], et0: 5, spacing: 30, flow: 2 }));
  assert.equal(r.emitters, 30);
  assert.equal(r.flowLh, 60);
  near(r.area, 3.6, 1e-9, "area");
  near(r.litersPerDay, 21, 1e-9, "liters a day");
  near(r.litersPerWeek, 147, 1e-9, "liters a week");
  assert.deepEqual(r.runs, { count: 1, minutes: 21 });
  assert.deepEqual(r.tank, { kind: "drum", size: 200 });
  // The user's own ET₀ wins over the estimate, which is still shown.
  const own = w.dripPlan(garden({ beds: [tomatoes], et0: 5, lat: 45, tmax: 30, tmin: 17 }));
  near(own.et0Estimate, 5.66, 0.02, "the estimate beside the user's own");
  assert.equal(own.et0, 5);
}
{
  // 62 mm of rain in July is 2 mm a day: (5 × 1.05 − 2) / 0.9 × 3.6 = 13 L; a wet month needs nothing.
  const tomatoes = bed({ length: 3, width: 1.2 });
  near(w.dripPlan(garden({ beds: [tomatoes], et0: 5, rain: 62, month: 7 })).litersPerDay, 13, 1e-9, "with rain");
  const wet = w.dripPlan(garden({ beds: [tomatoes], et0: 5, rain: 300, month: 7 }));
  assert.equal(wet.litersPerDay, 0);
  assert.equal(wet.runs, null);
  assert.equal(wet.tank, null);
}
{
  // Floating point: 1.2 m at 40 cm is 3 drippers, not 2; 0.9 m at 30 cm is 3.
  assert.equal(w.bedEmitters(bed({ length: 1.2, width: 0.4 }), 40), 3);
  assert.equal(w.bedEmitters(bed({ length: 0.9, width: 0.4 }), 30), 3);
  // The lines chosen by hand win over the suggestion.
  assert.equal(w.bedEmitters(bed({ length: 2, width: 1, rows: 2 }), 20), 20);
  // A bed given by its area: lines 40 cm apart, so 6 m² at 30 cm is 6 / 0.4 / 0.3 = 50 drippers.
  const potatoes = bed({ by: "area", area: 6, crop: "potatoes" });
  assert.equal(w.bedEmitters(potatoes, 30), 50);
  assert.equal(w.bedRows(potatoes), null);
  // No size yet: no area and no drippers, and no need to show.
  const empty = w.dripPlan(garden({ beds: [bed({})], et0: 5 }));
  assert.deepEqual([empty.area, empty.emitters, empty.litersPerDay], [0, 0, null]);
}
{
  // A large garden runs twice a day: 100 m² of corn at ET₀ 6 is 6 × 1.1 / 0.9 × 100 = 733.3 L a day;
  // 10 × 1 m beds, one line each, drippers every 40 cm at 1 L/h: 10 × 25 = 250 L/h -> 176 min, 2 × 88.
  const beds = Array.from({ length: 10 }, () => bed({ length: 10, width: 1, rows: 1, crop: "corn" }));
  const r = w.dripPlan(garden({ beds, et0: 6, spacing: 40, flow: 1 }));
  near(r.litersPerDay, 733.33, 0.01, "corn");
  assert.equal(r.emitters, 250);
  assert.deepEqual(r.runs, { count: 2, minutes: 88 });
  assert.deepEqual(r.tank, { kind: "tank", size: 1000 });
  assert.deepEqual(w.tankFor(2500), { kind: "tanks", size: 1000, count: 3 });
  assert.deepEqual(w.tankFor(12), { kind: "bucket", size: 20 });
}
{
  // Rainwater: 50 m² × 60 mm × 0.8 = 2,400 L, and how many days of the garden that is.
  const r = w.dripPlan(garden({ beds: [bed({ length: 3, width: 1.2 })], et0: 5, roof: 50, rain: 60, month: 6 }));
  assert.equal(r.roofLiters, 2400);
  // June: 2 mm a day, (5.25 − 2) / 0.9 × 3.6 = 13 L a day -> 184 days.
  assert.equal(r.roofDays, 184);
  assert.equal(w.dripPlan(garden({ roof: 50 })).roofLiters, null, "no rain given");
}
// Every crop group has a coefficient from the handbook's range.
for (const c of w.CROPS) assert.ok(c.kc >= 0.7 && c.kc <= 1.2, c.id);

// ---- the saved document ---------------------------------------------------------------

{
  const d = w.defaultPlan();
  assert.equal(d.v, 1);
  assert.equal(d.garden.beds.length, 1);
  // Damaged or foreign documents read as the defaults (with a fresh bed id).
  for (const bad of [null, "text", 3, [], { drink: "x", garden: [] }]) {
    const p = w.parsePlan(bad);
    assert.deepEqual({ ...p, garden: { ...p.garden, beds: [] } }, { ...d, garden: { ...d.garden, beds: [] } });
  }
  // Out of range is brought into range; unknown choices become the default; ids are kept.
  const p = w.parsePlan({
    v: 1,
    drink: { people: 1000, children: -3, smallPets: 2.6, largePets: "two", days: 0 },
    garden: {
      beds: [{ id: "bed-1", by: "area", area: 12, crop: "bananas" }, { id: "../x", length: 5000, width: 2, rows: 0 }],
      lat: 99, month: 13, tmax: 31.5, tmin: null, et0: -1, rain: 40, spacing: 25, flow: 4, roof: 80,
    },
  });
  assert.deepEqual(p.drink, { people: 100, children: 0, smallPets: 3, largePets: 0, days: 1 });
  assert.equal(p.garden.beds[0].id, "bed-1");
  assert.equal(p.garden.beds[0].by, "area");
  assert.equal(p.garden.beds[0].crop, "tomatoes");
  assert.notEqual(p.garden.beds[1].id, "../x");
  assert.deepEqual([p.garden.beds[1].by, p.garden.beds[1].length, p.garden.beds[1].rows], ["size", 1000, 1]);
  assert.deepEqual([p.garden.lat, p.garden.month, p.garden.tmax, p.garden.tmin, p.garden.et0], [66.5, 12, 31.5, null, 0]);
  assert.deepEqual([p.garden.spacing, p.garden.flow, p.garden.roof, p.garden.rain], [30, 4, 80, 40]);
  // At most 30 beds; a saved plan reads back the same.
  assert.equal(w.parsePlan({ garden: { beds: Array.from({ length: 40 }, () => ({})) } }).garden.beds.length, 30);
  assert.deepEqual(w.parsePlan(JSON.parse(JSON.stringify(p))), p);
  // A plan of 30 beds stays far below the hub's 32 KiB.
  const big = w.parsePlan({ garden: { beds: Array.from({ length: 30 }, () => ({ length: 123.456, width: 12.345, rows: 12, crop: "strawberries" })) } });
  assert.ok(JSON.stringify({ plan: big }).length < 8 * 1024);
}

// ---- the deep link ------------------------------------------------------------------

{
  const base = w.defaultPlan();
  assert.equal(w.linkQuery("#tools/water?people=4&days=7"), "people=4&days=7");
  assert.equal(w.linkQuery("#water?part=drip"), "part=drip");
  assert.equal(w.linkQuery("#water/drip?lat=45"), "lat=45");
  for (const other of ["#water", "#water/drip", "#tools", "#power?items=fan", "#waterfall?x=1", "#tools/water"]) assert.equal(w.linkQuery(other), null, other);
  const a = w.applyLink(base, "people=4&days=7");
  assert.equal(a.changed, true);
  assert.equal(a.part, "drink");
  assert.deepEqual(a.plan.drink, { ...base.drink, people: 4, days: 7 });
  assert.deepEqual(a.plan.garden, base.garden, "the garden is left alone");
  assert.equal(base.drink.people, 2, "the plan given is not changed");

  const b = w.applyLink(base, "part=drip&beds=3x1.2:tomatoes,2x1:greens&lat=44.8");
  assert.equal(b.part, "drip");
  assert.equal(b.plan.garden.lat, 44.8);
  assert.deepEqual(
    b.plan.garden.beds.map((x) => [x.by, x.length, x.width, x.crop, x.rows]),
    [["size", 3, 1.2, "tomatoes", null], ["size", 2, 1, "greens", null]],
  );
  assert.deepEqual(b.plan.drink, base.drink);

  // Areas, lines, other words for crops, and the rest of the garden.
  const c = w.applyLink(base, "beds=6:potatoes,4x1:cucumbers:2,5m2:lettuce,1x1:rocks,x:greens&month=6&tmax=28&tmin=15&rain=55,5&spacing=20&flow=4&roof=70&et0=4.5");
  assert.equal(c.part, "drip", "garden values open the garden part");
  assert.deepEqual(
    c.plan.garden.beds.map((x) => [x.by, x.area ?? `${x.length}x${x.width}`, x.crop, x.rows]),
    [["area", 6, "potatoes", null], ["size", "4x1", "squash", 2], ["area", 5, "greens", null]],
  );
  assert.deepEqual(
    [c.plan.garden.month, c.plan.garden.tmax, c.plan.garden.tmin, c.plan.garden.rain, c.plan.garden.spacing, c.plan.garden.flow, c.plan.garden.roof, c.plan.garden.et0],
    [6, 28, 15, 55.5, 20, 4, 70, 4.5],
  );

  // Nonsense changes nothing; out of range is brought into range.
  const d = w.applyLink(base, "people=lots&days=&spacing=25&flow=3&beds=,,&month=13&lat=abc");
  assert.equal(d.changed, true, "month 13 is read as December");
  assert.equal(d.plan.garden.month, 12);
  assert.deepEqual(d.plan.drink, base.drink);
  assert.deepEqual([d.plan.garden.spacing, d.plan.garden.flow, d.plan.garden.lat], [30, 2, null]);
  const e = w.applyLink(base, "people=-4&children=500&smallpets=1&largepets=2");
  assert.deepEqual(e.plan.drink, { ...base.drink, people: 0, children: 100, smallPets: 1, largePets: 2 });
  const f = w.applyLink(base, "hello=1");
  assert.deepEqual([f.changed, f.part], [false, null]);
  assert.equal(w.applyLink(base, "part=drink&lat=45").part, "drink", "the part named wins");
  // At most 30 beds from a link too.
  assert.equal(w.applyLink(base, "beds=" + Array.from({ length: 40 }, () => "1x1:greens").join(",")).plan.garden.beds.length, 30);
}

console.log("water: drinking water, ET₀ (Hargreaves-Samani, FAO-56 Ra), drip beds, the saved plan and the deep link check out.");
