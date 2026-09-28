// Trim GeoNames' list of places (cities5000.txt, CC BY 4.0) to what the
// hub's "find a place" needs, one place per line, the largest first:
//   name \t ascii name \t Serbian name \t region \t country \t lat \t lon \t population
// The Serbian name (Latin script) is there only when it differs from the
// others: Beograd for Belgrade, Beč for Vienna. It comes from GeoNames'
// alternate names (alternateNamesV2.txt), read from the standard input when
// the fourth argument is "-".
//   unzip -p alternateNamesV2.zip alternateNamesV2.txt |
//     node scripts/map-cities.mjs cities5000.txt admin1CodesASCII.txt cities.tsv -
// Called by scripts/fetch-map-assets.sh.
import { createInterface } from "node:readline";
import { readFileSync, writeFileSync } from "node:fs";

const [citiesFile, admin1File, outFile, alternates] = process.argv.slice(2);
if (!outFile) {
  console.error("usage: node map-cities.mjs cities5000.txt admin1CodesASCII.txt cities.tsv [-]");
  process.exit(2);
}

const fold = (s) => s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
const clean = (s) => s.replace(/[\t\r\n]/g, " ").trim();

// Serbian Cyrillic to Latin, letter by letter (the language's own rules).
const CYR = {
  а: "a", б: "b", в: "v", г: "g", д: "d", ђ: "đ", е: "e", ж: "ž", з: "z", и: "i", ј: "j", к: "k", л: "l", љ: "lj", м: "m",
  н: "n", њ: "nj", о: "o", п: "p", р: "r", с: "s", т: "t", ћ: "ć", у: "u", ф: "f", х: "h", ц: "c", ч: "č", џ: "dž", ш: "š",
};
function toLatin(s) {
  let out = "";
  for (const ch of s) {
    const low = ch.toLowerCase();
    const lat = CYR[low];
    if (lat === undefined) out += ch;
    else if (ch === low) out += lat;
    else out += lat[0].toUpperCase() + lat.slice(1);
  }
  return out;
}
const latinOnly = /^[\p{Script=Latin}\p{M}\d '’.,()-]+$/u;

// "RS.00" -> "Central Serbia"
const regions = new Map();
for (const line of readFileSync(admin1File, "utf8").split("\n")) {
  const [code, name, ascii] = line.split("\t");
  if (code && (name || ascii)) regions.set(code, clean(name || ascii));
}

const places = new Map();
for (const line of readFileSync(citiesFile, "utf8").split("\n")) {
  const f = line.split("\t");
  if (f.length < 15) continue;
  const [id, name, ascii, , lat, lon, , , country, , admin1] = f;
  const la = Number(lat);
  const lo = Number(lon);
  if (!name || !Number.isFinite(la) || !Number.isFinite(lo)) continue;
  places.set(id, {
    name: clean(name),
    ascii: clean(ascii),
    region: regions.get(`${country}.${admin1}`) ?? "",
    country,
    lat: la.toFixed(4),
    lon: lo.toFixed(4),
    population: Number(f[14]) || 0,
    sr: null,
  });
}

// The Serbian name of each place: Latin first, preferred names first; then
// the Cyrillic one written in Latin; then Serbo-Croatian, Bosnian or Croatian.
const RANK = { "sr-Latn": 0, sr: 1, sh: 2, bs: 3, hr: 4 };
if (alternates === "-") {
  const best = new Map();
  for await (const line of createInterface({ input: process.stdin, crlfDelay: Infinity })) {
    const f = line.split("\t");
    const lang = f[2];
    const rank = RANK[lang];
    if (rank === undefined || !places.has(f[1])) continue;
    // Colloquial and historic names are not what people search for.
    if (f[6] === "1" || f[7] === "1") continue;
    const text = clean(lang === "sr" ? toLatin(f[3]) : f[3]);
    if (!text || text.length > 60 || !latinOnly.test(text)) continue;
    const score = rank * 2 + (f[4] === "1" ? 0 : 1);
    const had = best.get(f[1]);
    if (!had || score < had.score) best.set(f[1], { score, text });
  }
  for (const [id, { text }] of best) {
    const p = places.get(id);
    if (fold(text) !== fold(p.name) && fold(text) !== fold(p.ascii)) p.sr = text;
  }
}

const rows = [...places.values()].sort((a, b) => b.population - a.population);
writeFileSync(
  outFile,
  rows.map((p) => [p.name, p.ascii, p.sr ?? "", p.region, p.country, p.lat, p.lon, String(p.population)].join("\t")).join("\n") + "\n",
);
console.log(`${rows.length} places (${rows.filter((p) => p.sr).length} with a Serbian name of their own) written to ${outFile}`);
