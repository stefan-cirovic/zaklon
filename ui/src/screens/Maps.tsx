import { useCallback, useEffect, useMemo, useState } from "react";
import { api } from "../api";
import type { Key, Lang } from "../i18n";
import { errText } from "../errors";
import { fmtBytes } from "../format";
import ConfirmButton from "../components/ConfirmButton";
import Qr from "../components/Qr";

type T = (k: Key) => string;
type Status = "not_installed" | "queued" | "downloading" | "paused" | "verifying" | "installed" | "failed";
type Region = { id: string; name: string; name_sr: string; size: number; status: Status; bytes_done: number; error?: string };
type Country = { id: string; name: string; name_sr: string; size: number; regions: Region[] };
type MapsReply = {
  version: number;
  server_urls: string[];
  app_urls: string[];
  app: { status: Status; bytes_done: number; bytes_total: number };
  installed_bytes: number;
  countries: Country[];
};

/** Suggested first, by app language. */
const SUGGESTED: Record<Lang, string[]> = {
  sr: ["Serbia", "Bosnia and Herzegovina", "Croatia", "Montenegro", "Macedonia", "Kosovo", "Slovenia", "Hungary", "Romania", "Bulgaria"],
  en: [],
};

/** Lower case without diacritics, so "srbija" finds "Srbija" and "cesko" finds "Češko". */
function fold(s: string) {
  return s.toLowerCase().replace(/đ/g, "dj").normalize("NFD").replace(/[\u0300-\u036f]/g, "");
}

function countryState(c: Country) {
  const installed = c.regions.filter((r) => r.status === "installed").length;
  const busy = c.regions.some((r) => ["queued", "downloading", "verifying"].includes(r.status));
  const failed = c.regions.some((r) => r.status === "failed");
  const done = c.regions.reduce((s, r) => s + (r.status === "installed" ? r.size : Math.min(r.bytes_done, r.size)), 0);
  return { installed, all: installed === c.regions.length, some: installed > 0, busy, failed, done };
}

export default function Maps({ t, lang }: { t: T; lang: Lang }) {
  const [data, setData] = useState<MapsReply | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const [open, setOpen] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setData(await api<MapsReply>("/api/maps"));
      setErr(null);
    } catch (e) {
      setErr(errText(t, e));
    }
  }, [t]);

  const busy = !!data && (data.countries.some((c) => countryState(c).busy) || ["queued", "downloading", "verifying"].includes(data.app.status));
  useEffect(() => {
    load();
    const id = setInterval(load, busy ? 2000 : 15000);
    return () => clearInterval(id);
  }, [load, busy]);

  const act = async (p: Promise<unknown>) => {
    try {
      await p;
      load();
    } catch (e) {
      setErr(errText(t, e));
    }
  };

  const nameOf = useCallback((x: { name: string; name_sr: string }) => (lang === "sr" ? x.name_sr : x.name), [lang]);
  const shown = useMemo(() => {
    if (!data) return [];
    const needle = fold(q.trim());
    const list = needle
      ? data.countries.filter((c) => [c, ...c.regions].some((x) => fold(x.name).includes(needle) || fold(x.name_sr).includes(needle)))
      : data.countries.filter((c) => SUGGESTED[lang].includes(c.id) || countryState(c).some || countryState(c).busy);
    const order = (c: Country) => {
      const i = SUGGESTED[lang].indexOf(c.id);
      return i === -1 ? 100 : i;
    };
    return [...list].sort((a, b) => order(a) - order(b) || nameOf(a).localeCompare(nameOf(b), lang));
  }, [data, q, lang, nameOf]);

  const server = data?.server_urls[0];
  const appUrl = data?.app_urls[0];
  const appReady = data?.app.status === "installed";

  return (
    <div className="stack">
      <div className="page-head">
        <h1>{t("maps")}</h1>
        <p className="muted">{t("mapsIntro")}</p>
      </div>
      {err && <p className="error" role="alert">{err}</p>}

      {data && (
        <div className="panel stack maps-phone">
          <h2>{t("mapsOnPhone")}</h2>
          <ol className="steps">
            <li>
              {t("mapsStep1")}
              {appReady && appUrl ? (
                <div className="qr-line">
                  <Qr value={appUrl} size={140} label={t("mapsAppQr")} />
                  <code className="url">{appUrl}</code>
                </div>
              ) : (
                <p className="muted" style={{ fontSize: 14 }}>{t("mapsAppComes")}</p>
              )}
            </li>
            <li>
              {t("mapsStep2")}
              {server && (
                <div className="qr-line">
                  <Qr value={server} size={140} label={t("mapsServerQr")} />
                  <code className="url">{server}</code>
                </div>
              )}
            </li>
            <li>{t("mapsStep3")}</li>
          </ol>
        </div>
      )}

      <div className="row between wrap">
        <h2 style={{ margin: 0 }}>{t("mapsOnHub")}</h2>
        {data && <span className="muted" style={{ fontSize: 14 }}>{t("mapsInstalledSize")}: {fmtBytes(data.installed_bytes)}</span>}
      </div>
      <input type="search" className="search" value={q} onChange={(e) => setQ(e.target.value)} placeholder={t("mapsSearch")} aria-label={t("mapsSearch")} />
      {!q && <p className="muted" style={{ fontSize: 14 }}>{lang === "sr" ? t("mapsSuggestedHint") : t("mapsSearchHint")}</p>}

      <div className="list">
        {shown.map((c) => {
          const s = countryState(c);
          const pct = c.size ? Math.round((s.done / c.size) * 100) : 0;
          const multi = c.regions.length > 1;
          return (
            <div className="item stack map-country" key={c.id}>
              <div className="row between wrap">
                <div>
                  <div className="map-name">{nameOf(c)}</div>
                  <div className="muted" style={{ fontSize: 13 }}>
                    {fmtBytes(c.size)}
                    {multi && ` · ${c.regions.length} ${t("mapsRegions")}`}
                    {s.some && !s.all && ` · ${s.installed}/${c.regions.length} ${t("installed").toLowerCase()}`}
                  </div>
                </div>
                <div className="row">
                  {s.all ? (
                    <span className="ok">{t("installed")}</span>
                  ) : s.busy ? (
                    <span className="muted">{pct}%</span>
                  ) : (
                    <button className="btn small" onClick={() => act(api(`/api/maps/${encodeURIComponent(c.id)}/download`, { method: "POST" }))}>
                      {s.some || s.failed ? t("resume") : t("download")}
                    </button>
                  )}
                  {(s.some || s.failed) && !s.busy && (
                    <ConfirmButton
                      label={t("remove")}
                      confirmLabel={t("yesRemove")}
                      cancelLabel={t("cancel")}
                      className="btn danger small"
                      onConfirm={() => act(api(`/api/maps/${encodeURIComponent(c.id)}`, { method: "DELETE" }))}
                    />
                  )}
                  {multi && (
                    <button className="btn secondary small" aria-expanded={open === c.id} onClick={() => setOpen(open === c.id ? null : c.id)}>
                      {open === c.id ? "▴" : "▾"}
                    </button>
                  )}
                </div>
              </div>
              {s.busy && <div className="bar"><i style={{ width: `${pct}%` }} /></div>}
              {s.failed && !s.busy && <div className="warn" style={{ fontSize: 13 }}>{t("mapsSomeFailed")}</div>}
              {multi && open === c.id && (
                <div className="list regions">
                  {c.regions.map((r) => (
                    <div className="row between region" key={r.id}>
                      <span>{nameOf(r)} <span className="muted" style={{ fontSize: 13 }}>· {fmtBytes(r.size)}</span></span>
                      {r.status === "installed" ? (
                        <ConfirmButton
                          label={t("remove")}
                          confirmLabel={t("yesRemove")}
                          cancelLabel={t("cancel")}
                          className="btn danger small"
                          onConfirm={() => act(api(`/api/packs/${encodeURIComponent("map:" + r.id)}`, { method: "DELETE" }))}
                        />
                      ) : ["queued", "downloading", "verifying"].includes(r.status) ? (
                        <span className="muted">{Math.round((r.bytes_done / Math.max(r.size, 1)) * 100)}%</span>
                      ) : (
                        <button className="btn secondary small" onClick={() => act(api(`/api/packs/${encodeURIComponent("map:" + r.id)}/download`, { method: "POST" }))}>
                          {t("download")}
                        </button>
                      )}
                    </div>
                  ))}
                </div>
              )}
            </div>
          );
        })}
        {data && shown.length === 0 && <p className="muted">{t("noResults")}</p>}
      </div>
      <p className="muted" style={{ fontSize: 12 }}>{t("mapsAttribution")}</p>
    </div>
  );
}
