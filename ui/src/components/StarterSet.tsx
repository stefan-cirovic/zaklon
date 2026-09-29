import { useEffect, useState } from "react";
import { api } from "../api";
import type { Key, Lang } from "../i18n";
import { errText } from "../errors";
import { fmtBytes } from "../format";
import { downloadedByUser, type StarterSetDef } from "../packs";

type T = (k: Key) => string;
type Pack = { id: string; title: { en: string; sr: string }; size: number; offer?: string; withdrawn?: boolean; state: { status: string } };
type MapCountry = { id: string; name: string; name_sr: string; size: number; regions: { status: string }[] };

/** done: installed or on its way (not started again); installed: ready to use; busy: downloading now. */
type Line = { key: string; label: string; size: number; done: boolean; installed: boolean; busy: boolean; start: () => Promise<unknown> };
const BUSY = ["queued", "downloading", "verifying"];

/**
 * One button for the recommended start ("Basic pack for Serbia" / "English
 * essentials"): the catalog's starter set for the app's language, the map of
 * its country and the AI model that fits. Only packs offered to everyone: a
 * pack people download themselves is never part of it, whatever the catalog says.
 */
export default function StarterSet({
  t,
  lang,
  packs,
  sets,
  freeBytes,
  onStarted,
  embedded = false,
}: {
  t: T;
  lang: Lang;
  packs: Pack[];
  sets: StarterSetDef[];
  freeBytes: number;
  onStarted: () => void;
  /** Inside another panel (the Library's start): no panel of its own. */
  embedded?: boolean;
}) {
  const [model, setModel] = useState<string | null>(null);
  const [map, setMap] = useState<MapCountry | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // Whether the hub has said which AI model fits, and the country the map was looked up for.
  const [modelKnown, setModelKnown] = useState(false);
  const [mapFor, setMapFor] = useState<string | null | undefined>(undefined);
  const set = sets.find((s) => s.lang === lang) ?? null;
  const country = set?.map ?? null;

  useEffect(() => {
    // No model when none fits this computer's memory (recommended is null).
    api<{ recommended: string | null }>("/api/assistant")
      .then((a) => setModel(a.recommended ?? null))
      .catch(() => {})
      .finally(() => setModelKnown(true));
  }, []);
  useEffect(() => {
    if (country) {
      api<{ countries: MapCountry[] }>("/api/maps")
        .then((m) => setMap(m.countries.find((c) => c.id === country) ?? null))
        .catch(() => {})
        .finally(() => setMapFor(country));
    } else {
      setMap(null);
      setMapFor(null);
    }
  }, [country]);
  // "Download all" waits until the set is complete: a tap before the model is
  // known would leave the AI model out.
  const complete = modelKnown && mapFor === country;

  const title = (p: Pack) => (lang === "sr" && p.title.sr ? p.title.sr : p.title.en);
  const lines: Line[] = [];
  for (const id of [...(set?.packs ?? []), ...(model ? [model] : [])]) {
    const p = packs.find((x) => x.id === id);
    if (!p || downloadedByUser(p) || p.withdrawn) continue;
    lines.push({
      key: id,
      label: title(p),
      size: p.size,
      done: p.state.status !== "not_installed" && p.state.status !== "failed",
      installed: p.state.status === "installed",
      busy: BUSY.includes(p.state.status),
      start: () => api(`/api/packs/${id}/download`, { method: "POST" }),
    });
  }
  if (map) {
    lines.push({
      key: `map:${map.id}`,
      label: `${t("mapsOf")}: ${lang === "sr" ? map.name_sr : map.name}`,
      size: map.size,
      done: map.regions.every((r) => r.status !== "not_installed" && r.status !== "failed"),
      installed: map.regions.every((r) => r.status === "installed"),
      busy: map.regions.some((r) => BUSY.includes(r.status)),
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
    <div className={(embedded ? "" : "panel ") + "stack left starter"}>
      {embedded ? <h3>{lang === "sr" ? t("starterTitleSr") : t("starterTitleEn")}</h3> : <h2>{lang === "sr" ? t("starterTitleSr") : t("starterTitleEn")}</h2>}
      <p className="muted" style={{ margin: 0, fontSize: 14 }}>{t("starterIntro")}</p>
      <ul className="plain">
        {lines.map((l) => (
          <li key={l.key} className={l.installed ? "installed" : undefined}>
            {l.label} <span className="muted">· {fmtBytes(l.size)}</span>
            {l.busy && <span className="muted"> · {t("aiDownloading")}</span>}
          </li>
        ))}
      </ul>
      <p style={{ margin: 0 }}>
        {t("starterTotal")}: <strong>{fmtBytes(total)}</strong> <span className="muted">· {fmtBytes(freeBytes)} {t("free")}</span>
      </p>
      {!fits && <p className="warn" style={{ margin: 0 }}>{t("errDisk")}</p>}
      {err && <p className="error" role="alert">{err}</p>}
      <div>
        <button className="btn" onClick={startAll} disabled={busy || !fits || !complete}>{t("starterDownload")}</button>
      </div>
    </div>
  );
}
