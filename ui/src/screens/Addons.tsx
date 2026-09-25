import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import type { Key, Lang } from "../i18n";

type T = (k: Key) => string;
type Props = { t: T; lang: Lang; isHub: boolean };

type Localized = { en: string; sr: string };
type PackStatus = "not_installed" | "queued" | "downloading" | "paused" | "verifying" | "installed" | "failed";
type Pack = {
  id: string;
  title: Localized;
  description: Localized;
  category: "knowledge" | "maps" | "model" | "app";
  version: string;
  size: number;
  license: string;
  attribution: string;
  recommended_for: string[];
  state: { status: PackStatus; bytes_done: number; bytes_total: number; error?: string; speed: number };
};
type CatalogReply = {
  packs: Pack[];
  system: { disk_free: number; disk_total: number; battery_percent: number | null; plugged_in: boolean };
};

export function fmtBytes(n: number): string {
  if (n >= 1e9) return `${(n / 1e9).toFixed(n >= 1e10 ? 0 : 1)} GB`;
  if (n >= 1e6) return `${Math.round(n / 1e6)} MB`;
  return `${Math.round(n / 1e3)} kB`;
}

const CATEGORY_KEY: Record<Pack["category"], Key> = { knowledge: "catKnowledge", maps: "catMaps", model: "catModels", app: "catApps" };

export default function Addons({ t, lang, isHub }: Props) {
  const [data, setData] = useState<CatalogReply | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [importDir, setImportDir] = useState("");
  const [note, setNote] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setData(await api<CatalogReply>("/api/catalog"));
      setErr(null);
    } catch (e) {
      setErr((e as Error).message);
    }
  }, []);

  const busy = data?.packs.some((p) => ["queued", "downloading", "verifying"].includes(p.state.status)) ?? false;
  useEffect(() => {
    load();
    const id = setInterval(load, busy ? 1500 : 10000);
    return () => clearInterval(id);
  }, [load, busy]);

  const act = async (path: string, method = "POST") => {
    try {
      await api(path, { method });
      load();
    } catch (e) {
      setErr((e as Error).message);
    }
  };

  const doImport = async () => {
    setNote(null);
    try {
      const r = await api<{ imported: string[] }>("/api/packs/import", { json: { dir: importDir } });
      setNote(r.imported.length ? `${t("imported")}: ${r.imported.join(", ")}` : t("nothingToImport"));
      load();
    } catch (e) {
      setErr((e as Error).message);
    }
  };

  const title = (l: Localized) => (lang === "sr" && l.sr ? l.sr : l.en);
  const groups: Pack["category"][] = ["knowledge", "maps", "model", "app"];

  return (
    <div className="stack">
      <div>
        <h1>{t("addons")}</h1>
        <p className="muted">{t("addonsIntro")}</p>
      </div>
      {err && <p className="error">{err}</p>}
      {data && (
        <div className="grid">
          <div className="panel">
            <div className="label">{t("diskFree")}</div>
            <div className="value">{fmtBytes(data.system.disk_free)}</div>
            <div className="muted" style={{ fontSize: 13 }}>{t("of")} {fmtBytes(data.system.disk_total)}</div>
          </div>
          <div className="panel">
            <div className="label">{t("battery")}</div>
            <div className="value">
              {data.system.battery_percent === null ? "–" : `${data.system.battery_percent}%`}
              {data.system.plugged_in && <span className="muted" style={{ fontSize: 14 }}> · {t("pluggedIn")}</span>}
            </div>
            {!data.system.plugged_in && (data.system.battery_percent ?? 100) < 50 && (
              <div className="warn" style={{ fontSize: 13 }}>{t("batteryRule")}</div>
            )}
          </div>
        </div>
      )}
      {groups.map((cat) => {
        const packs = data?.packs.filter((p) => p.category === cat) ?? [];
        if (packs.length === 0) return null;
        return (
          <div key={cat}>
            <h2>{t(CATEGORY_KEY[cat])}</h2>
            <div className="list">
              {packs.map((p) => (
                <PackRow key={p.id} p={p} t={t} lang={lang} title={title} act={act} />
              ))}
            </div>
          </div>
        );
      })}
      {isHub && (
        <div className="panel stack">
          <h2>{t("importTitle")}</h2>
          <p className="muted">{t("importIntro")}</p>
          <div className="row">
            <input type="text" value={importDir} onChange={(e) => setImportDir(e.target.value)} placeholder="E:\" />
            <button className="btn secondary" onClick={doImport} disabled={!importDir.trim()}>{t("importBtn")}</button>
          </div>
          {note && <p className="ok">{note}</p>}
        </div>
      )}
    </div>
  );
}

function PackRow({ p, t, lang, title, act }: { p: Pack; t: T; lang: Lang; title: (l: Localized) => string; act: (path: string, method?: string) => void }) {
  const s = p.state;
  const pct = s.bytes_total ? Math.min(100, Math.round((s.bytes_done / s.bytes_total) * 100)) : 0;
  const recommended = p.recommended_for.includes(lang);
  return (
    <div className="item" style={{ flexDirection: "column", alignItems: "stretch", gap: 8 }}>
      <div className="row between">
        <div>
          <div>
            {title(p.title)} {recommended && <span className="ok" style={{ fontSize: 12 }}>· {t("recommended")}</span>}
          </div>
          <div className="muted" style={{ fontSize: 13 }}>{title(p.description)}</div>
          <div className="muted" style={{ fontSize: 13 }}>{fmtBytes(p.size)} · {p.version} · {p.license}</div>
        </div>
        <div className="row" style={{ flexShrink: 0 }}>
          {s.status === "not_installed" && <button className="btn" onClick={() => act(`/api/packs/${p.id}/download`)}>{t("download")}</button>}
          {s.status === "queued" && <button className="btn secondary" onClick={() => act(`/api/packs/${p.id}/pause`)}>{t("queued")} · {t("cancel")}</button>}
          {(s.status === "downloading" || s.status === "verifying") && (
            <button className="btn secondary" onClick={() => act(`/api/packs/${p.id}/pause`)}>{t("pause")}</button>
          )}
          {(s.status === "paused" || s.status === "failed") && (
            <>
              <button className="btn" onClick={() => act(`/api/packs/${p.id}/download`)}>{s.status === "paused" ? t("resume") : t("retry")}</button>
              <button className="btn danger" onClick={() => act(`/api/packs/${p.id}`, "DELETE")}>{t("remove")}</button>
            </>
          )}
          {s.status === "installed" && (
            <>
              <span className="ok">{t("installed")}</span>
              <button className="btn danger" onClick={() => act(`/api/packs/${p.id}`, "DELETE")}>{t("remove")}</button>
            </>
          )}
        </div>
      </div>
      {(s.status === "downloading" || s.status === "paused" || s.status === "verifying" || s.status === "queued") && (
        <div>
          <div className="bar"><i style={{ width: `${pct}%` }} /></div>
          <div className="muted" style={{ fontSize: 13 }}>
            {s.status === "verifying" ? t("verifying") : `${fmtBytes(s.bytes_done)} / ${fmtBytes(s.bytes_total)} (${pct}%)`}
            {s.status === "downloading" && s.speed > 0 && ` · ${fmtBytes(s.speed)}/s`}
          </div>
        </div>
      )}
      {s.status === "failed" && s.error && <div className="warn" style={{ fontSize: 13 }}>{s.error}</div>}
    </div>
  );
}
