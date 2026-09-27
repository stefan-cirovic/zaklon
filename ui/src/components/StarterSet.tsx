import { useEffect, useState } from "react";
import { api } from "../api";
import type { Key, Lang } from "../i18n";
import { errText } from "../errors";
import { fmtBytes } from "../format";

type T = (k: Key) => string;
type Pack = { id: string; title: { en: string; sr: string }; size: number; state: { status: string } };
type MapCountry = { id: string; name: string; name_sr: string; size: number; regions: { status: string }[] };

/** The packs a household starts with, by language ("Srbija osnovni" / "English essentials"). */
const KNOWLEDGE: Record<Lang, string[]> = {
  sr: ["wikipedia-sr-maxi", "wiktionary-sr", "wikimed-en", "ifixit-en"],
  en: ["wikipedia-en-mini", "wikimed-en", "ifixit-en"],
};
const MAP: Record<Lang, string | null> = { sr: "Serbia", en: null };

type Line = { key: string; label: string; size: number; done: boolean; start: () => Promise<unknown> };

/** One button for the recommended start: knowledge, the region's map and the AI model that fits. */
export default function StarterSet({ t, lang, packs, freeBytes, onStarted }: { t: T; lang: Lang; packs: Pack[]; freeBytes: number; onStarted: () => void }) {
  const [model, setModel] = useState<string | null>(null);
  const [map, setMap] = useState<MapCountry | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api<{ recommended: string }>("/api/assistant").then((a) => setModel(a.recommended)).catch(() => {});
    const country = MAP[lang];
    if (country) {
      api<{ countries: MapCountry[] }>("/api/maps")
        .then((m) => setMap(m.countries.find((c) => c.id === country) ?? null))
        .catch(() => {});
    } else setMap(null);
  }, [lang]);

  const title = (p: Pack) => (lang === "sr" && p.title.sr ? p.title.sr : p.title.en);
  const lines: Line[] = [];
  for (const id of [...KNOWLEDGE[lang], ...(model ? [model] : [])]) {
    const p = packs.find((x) => x.id === id);
    if (!p) continue;
    lines.push({ key: id, label: title(p), size: p.size, done: p.state.status !== "not_installed" && p.state.status !== "failed", start: () => api(`/api/packs/${id}/download`, { method: "POST" }) });
  }
  if (map) {
    lines.push({
      key: `map:${map.id}`,
      label: `${t("mapsOf")}: ${lang === "sr" ? map.name_sr : map.name}`,
      size: map.size,
      done: map.regions.every((r) => r.status !== "not_installed" && r.status !== "failed"),
      start: () => api(`/api/maps/${encodeURIComponent(map.id)}/download`, { method: "POST" }),
    });
  }
  const missing = lines.filter((l) => !l.done);
  if (lines.length === 0 || missing.length === 0) return null;
  const total = missing.reduce((s, l) => s + l.size, 0);
  const fits = total < freeBytes;

  const startAll = async () => {
    setBusy(true);
    setErr(null);
    try {
      for (const l of missing) await l.start();
      onStarted();
    } catch (e) {
      setErr(errText(t, e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="panel stack left starter">
      <h2>{lang === "sr" ? "Srbija osnovni" : "English essentials"}</h2>
      <p className="muted" style={{ margin: 0, fontSize: 14 }}>{t("starterIntro")}</p>
      <ul className="plain">
        {lines.map((l) => (
          <li key={l.key}>
            {l.done ? "✓ " : ""}
            {l.label} <span className="muted">· {fmtBytes(l.size)}</span>
          </li>
        ))}
      </ul>
      <p style={{ margin: 0 }}>
        {t("starterTotal")}: <strong>{fmtBytes(total)}</strong> <span className="muted">· {fmtBytes(freeBytes)} {t("free")}</span>
      </p>
      {!fits && <p className="warn" style={{ margin: 0 }}>{t("errDisk")}</p>}
      {err && <p className="error" role="alert">{err}</p>}
      <div>
        <button className="btn" onClick={startAll} disabled={busy || !fits}>{t("starterDownload")}</button>
      </div>
    </div>
  );
}
