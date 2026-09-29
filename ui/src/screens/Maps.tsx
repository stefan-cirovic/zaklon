import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { api } from "../api";
import { copyText } from "../clipboard";
import type { Key, Lang } from "../i18n";
import { errText } from "../errors";
import { useVisiblePoll } from "../poll";
import { countWord, fmtBytes, fold } from "../format";
import { countryState, SUGGESTED, type Country, type MapsReply } from "../maps";
import ConfirmButton from "../components/ConfirmButton";
import Qr from "../components/Qr";
import HelpLink from "../components/HelpLink";
import MapPart from "../components/MapPart";
import { openUrl } from "@tauri-apps/plugin-opener";

type T = (k: Key) => string;

/** The two parts of the screen, in the address after "#maps/". */
type Part = "map" | "navigation";

/**
 * "#maps", "#maps/home" and "#maps/world": the Zaklon map (the second with
 * the home location open, the third with the world map to download in
 * view); "#maps/navigation": CoMaps.
 */
function routeOf(hash: string): { part: Part; home: boolean; world: boolean } {
  const sub = hash.replace(/^#/, "").split("/")[1] ?? "";
  return { part: sub === "navigation" ? "navigation" : "map", home: sub === "home", world: sub === "world" };
}

/**
 * Maps: the Zaklon map (the world from the hub, the household's home on
 * it) and Navigation (CoMaps on phones, with maps from the hub, for finding
 * the way and for when a phone is away from home).
 */
export default function Maps({ t, lang, isHub }: { t: T; lang: Lang; isHub: boolean }) {
  const [route, setRoute] = useState(() => routeOf(location.hash));
  useEffect(() => {
    const onHash = () => setRoute(routeOf(location.hash));
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);
  const show = (part: Part) => {
    const hash = part === "map" ? "#maps" : "#maps/navigation";
    if (location.hash !== hash) history.replaceState(history.state, "", hash);
    setRoute({ part, home: false, world: false });
  };
  const parts: [Part, Key][] = [
    ["map", "mapsZaklonMap"],
    ["navigation", "mapsNavigation"],
  ];
  return (
    <div className={"stack maps-screen" + (route.part === "map" ? " map-part" : "")}>
      <div className="page-head">
        <div className="title-line">
          <h1>{t("maps")}</h1>
          <HelpLink t={t} topic="maps" section={route.part === "navigation" ? "phone" : undefined} />
        </div>
      </div>
      <div className="segmented maps-parts" role="tablist" aria-label={t("maps")}>
        {parts.map(([part, key]) => (
          <button key={part} type="button" role="tab" aria-selected={route.part === part} className={route.part === part ? "active" : ""} onClick={() => show(part)}>
            {t(key)}
          </button>
        ))}
      </div>
      {route.part === "map" ? <MapPart t={t} lang={lang} isHub={isHub} homeOpen={route.home} worldOpen={route.world} /> : <Navigation t={t} lang={lang} isHub={isHub} />}
    </div>
  );
}

/** CoMaps for phones: the app and the maps of every country, from the hub. */
function Navigation({ t, lang, isHub }: { t: T; lang: Lang; isHub: boolean }) {
  const [copied, setCopied] = useState(false);
  const [data, setData] = useState<MapsReply | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const [open, setOpen] = useState<string | null>(null);

  const [appBusy, setAppBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      setData(await api<MapsReply>("/api/maps"));
      setErr(null);
      return true;
    } catch (e) {
      setErr(errText(t, e));
      return false;
    }
  }, [t]);

  const busy = !!data && (data.countries.some((c) => countryState(c).busy) || ["queued", "downloading", "verifying"].includes(data.app.status));
  useVisiblePoll(load, busy ? 2000 : 15000);

  // A paired phone copies the app over its trusted connection to the hub and
  // checks it before the system installer sees it (the plain-HTTP address is
  // for phones that are not paired yet).
  const installApp = async () => {
    setErr(null);
    setAppBusy(true);
    try {
      await openUrl(await invoke<string>("client_fetch_app", { name: "comaps" }));
    } catch (e) {
      setErr(errText(t, e));
    } finally {
      setAppBusy(false);
    }
  };

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
      : data.countries.filter((c) => {
          // Without a search: the suggestions, and every country something was done with.
          const s = countryState(c);
          return SUGGESTED[lang].includes(c.id) || s.some || s.busy || s.failed || s.paused;
        });
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
    <div className="stack" role="tabpanel">
      <p className="muted" style={{ margin: 0 }}>{t("mapsIntro")}</p>
      {err && <p className="error" role="alert">{err}</p>}
      {!data && !err && <p className="muted">{t("aiLoading")}</p>}

      {data && (
        <div className="panel stack maps-phone">
          <h2>{t("mapsOnPhone")}</h2>
          <ol className="steps">
            <li>
              {t("mapsStep1")}
              {appReady && !isHub ? (
                <div className="qr-line">
                  <button className="btn" onClick={installApp} disabled={appBusy}>{appBusy ? t("mapsAppCopying") : t("mapsInstallApp")}</button>
                </div>
              ) : appReady && appUrl ? (
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
                  {isHub && <Qr value={server} size={140} label={t("mapsServerQr")} />}
                  <code className="url">{server}</code>
                  {!isHub && (
                    <button className="btn secondary small" onClick={async () => setCopied(await copyText(server))}>
                      {copied ? t("copied") : t("copyAddress")}
                    </button>
                  )}
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

      <div className="list cols">
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
                    {multi && ` · ${c.regions.length} ${countWord(c.regions.length, ["region", "regions"], ["region", "regiona", "regiona"])}`}
                    {s.some && !s.all && ` · ${s.installed}/${c.regions.length} ${t("installed").toLowerCase()}`}
                  </div>
                </div>
                <div className="row">
                  {s.all && !s.update ? (
                    <span className="ok">{t("installed")}</span>
                  ) : s.busy ? (
                    <span className="muted">{pct}%</span>
                  ) : (
                    <button className="btn small" onClick={() => act(api(`/api/maps/${encodeURIComponent(c.id)}/download`, { method: "POST" }))}>
                      {s.update ? t("packUpdate") : s.some || s.paused ? t("resume") : s.failed ? t("retry") : t("download")}
                    </button>
                  )}
                  {isHub && (s.some || s.failed || s.paused) && !s.busy && (
                    <ConfirmButton
                      label={t("remove")}
                      confirmLabel={t("yesRemove")}
                      cancelLabel={t("cancel")}
                      className="btn danger small"
                      onConfirm={() => act(api(`/api/maps/${encodeURIComponent(c.id)}`, { method: "DELETE" }))}
                    />
                  )}
                  {multi && (
                    <button className="btn secondary small" aria-expanded={open === c.id} aria-label={`${t("mapsRegionsOf")} ${nameOf(c)}`} onClick={() => setOpen(open === c.id ? null : c.id)}>
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
                      {r.status === "installed" && !isHub ? (
                        <span className="ok">{t("installed")}</span>
                      ) : r.status === "installed" ? (
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
        {data && q.trim() && shown.length === 0 && <p className="muted">{t("noResults")}</p>}
      </div>
      <p className="muted" style={{ fontSize: 12 }}>{t("mapsAttribution")}</p>
    </div>
  );
}
