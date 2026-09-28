import { useCallback, useEffect, useId, useRef, useState } from "react";
import { api } from "../api";
import type { Key, Lang } from "../i18n";
import { errText } from "../errors";
import { useVisiblePoll } from "../poll";
import { fmtDateTime } from "../format";
import { coords, distanceKm, hasTiles, phoneMapInfo, placeLabel, rememberHome, wrapLon, type FoundPlace, type HomePlace, type LatLon, type MapInfo } from "../map/info";
import ConfirmButton from "./ConfirmButton";
import ZaklonMap, { type ZaklonMapHandle } from "./ZaklonMap";

type T = (k: Key) => string;

/** How often the map's state is asked for again; faster while the world map downloads or is checked. */
const EVERY = 20_000;
const BUSY_EVERY = 3_000;
const BUSY = ["queued", "downloading", "verifying"];
/** A tap this close to a place found by name is taken as the same town: its name stays. */
const SAME_TOWN_KM = 20;
/** Typing pauses this long before "find a place" asks the hub. */
const SEARCH_DELAY = 250;

/**
 * What the hub says about its map (and the home), asked for again now and
 * then. A phone that cannot reach the hub draws the world overview it
 * carries instead, with the home it saw last.
 */
export function useMapInfo(t: T, enabled = true) {
  const [info, setInfo] = useState<MapInfo | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const busy = !!info?.world && BUSY.includes(info.world.status);
  const reload = useVisiblePoll(
    useCallback(async () => {
      try {
        const fromHub = await api<MapInfo>("/api/map");
        rememberHome(fromHub.home);
        setInfo(fromHub);
        setErr(null);
        return true;
      } catch (e) {
        const onPhone = await phoneMapInfo().catch(() => null);
        if (onPhone) setInfo(onPhone);
        setErr(errText(t, e));
        return false;
      }
    }, [t]),
    enabled ? (busy ? BUSY_EVERY : EVERY) : null,
  );
  return { info, err, reload };
}

/**
 * One line over the map about how much of it there is: only the world
 * overview (with the way to the world map in Add-ons), the world map on its
 * way or being checked, or no map at all.
 */
export function MapNote({ t, info, err }: { t: T; info: MapInfo | null; err: string | null }) {
  if (err && !info) return <p className="zmap-note warn">{t("mapUnreachable")}</p>;
  if (!info) return null;
  if (info.phone) return <p className="zmap-note">{t("mapPhoneOverview")}</p>;
  const w = info.world;
  const pct = w && w.bytes_total ? Math.min(100, Math.floor((w.bytes_done / w.bytes_total) * 100)) : 0;
  const addons = (
    <a className="zmap-note-link" href="#addons/maps">
      {t("mapWorldInAddons")}
    </a>
  );
  if (!hasTiles(info)) {
    return (
      <p className="zmap-note">
        {t("mapNoData")} {addons}
      </p>
    );
  }
  if (info.tiles.detailed) {
    // The world map is shown while it is checked (it was put in place by hand, say).
    return w?.status === "verifying" ? <p className="zmap-note">{t("mapWorldChecking").replace("{n}", String(pct))}</p> : null;
  }
  if (w && ["queued", "downloading", "paused"].includes(w.status)) {
    return (
      <p className="zmap-note">
        {t("mapWorldDownloading").replace("{n}", String(pct))} {addons}
      </p>
    );
  }
  return (
    <p className="zmap-note">
      {t("mapOverviewOnly")} {addons}
    </p>
  );
}

/** A place chosen for the home but not saved yet. */
type Chosen = LatLon & { label: string | null; source: "gps" | "place" | "map" };

/** The home in a few words: its name, or where it is. */
export function homeText(home: HomePlace): string {
  return home.label || coords(home);
}

/**
 * The Zaklon map part of the Maps screen: the map as large as the window
 * allows, and beside it (below it on a phone) the household's home location
 * and the ways to set it: the phone's location, a town found by name, a tap
 * on the map.
 */
export default function MapPart({ t, lang, isHub, homeOpen }: { t: T; lang: Lang; isHub: boolean; homeOpen: boolean }) {
  const { info, err, reload } = useMapInfo(t);
  const map = useRef<ZaklonMapHandle>(null);
  const [editing, setEditing] = useState(homeOpen);
  const [chosen, setChosen] = useState<Chosen | null>(null);
  useEffect(() => {
    if (homeOpen) setEditing(true);
  }, [homeOpen]);

  const home = info?.home ?? null;
  const pick = (p: LatLon) => {
    const at = { lat: p.lat, lon: wrapLon(p.lon) };
    // Moving the mark a little within the town found by name keeps the town's name.
    const keep = chosen?.label && distanceKm(chosen, at) < SAME_TOWN_KM ? chosen.label : null;
    setChosen({ ...at, label: keep, source: "map" });
  };
  const choose = (c: Chosen, zoom?: number) => {
    setChosen(c);
    map.current?.flyTo(c, zoom);
  };

  return (
    <div className={"map-part with-side" + (editing ? " editing" : "")} role="tabpanel">
      <ZaklonMap ref={map} t={t} lang={lang} info={info} home={home} pending={editing ? chosen : null} onPick={editing ? pick : undefined} start="home" label={t("mapsZaklonMap")}>
        <MapNote t={t} info={info} err={err} />
      </ZaklonMap>
      <HomeLocation
        t={t}
        lang={lang}
        phone={!isHub}
        info={info}
        home={home}
        editing={editing}
        setEditing={(on) => {
          setEditing(on);
          if (!on) setChosen(null);
        }}
        chosen={chosen}
        choose={choose}
        saved={() => {
          setChosen(null);
          setEditing(false);
          reload();
        }}
        showHome={() => home && map.current?.flyTo(home)}
      />
    </div>
  );
}

/** The household's home: where it is, and setting, changing or removing it. */
function HomeLocation({
  t,
  lang,
  phone,
  info,
  home,
  editing,
  setEditing,
  chosen,
  choose,
  saved,
  showHome,
}: {
  t: T;
  lang: Lang;
  phone: boolean;
  info: MapInfo | null;
  home: HomePlace | null;
  editing: boolean;
  setEditing: (on: boolean) => void;
  chosen: Chosen | null;
  choose: (c: Chosen, zoom?: number) => void;
  saved: () => void;
  showHome: () => void;
}) {
  const headId = useId();
  const searchId = useId();
  const [q, setQ] = useState("");
  const [found, setFound] = useState<FoundPlace[] | null>(null);
  const [locating, setLocating] = useState(false);
  const [saving, setSaving] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);

  // "Find a place": ask the hub once typing pauses; the newest question wins.
  useEffect(() => {
    const query = q.trim();
    if (query.length < 2) {
      setFound(null);
      return;
    }
    let alive = true;
    const timer = setTimeout(() => {
      api<FoundPlace[]>(`/api/map/places?q=${encodeURIComponent(query)}`)
        .then((list) => alive && setFound(Array.isArray(list) ? list : []))
        .catch((e) => alive && setErr(errText(t, e)));
    }, SEARCH_DELAY);
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [q, t]);

  // The phone's GPS, only when asked for, and only for this.
  const locate = () => {
    setErr(null);
    setNote(null);
    if (typeof navigator === "undefined" || !navigator.geolocation) {
      setErr(t("gpsUnsupported"));
      return;
    }
    setLocating(true);
    navigator.geolocation.getCurrentPosition(
      (pos) => {
        setLocating(false);
        choose({ lat: pos.coords.latitude, lon: pos.coords.longitude, label: null, source: "gps" }, 14);
        setNote(t("gpsFound"));
      },
      (e) => {
        setLocating(false);
        setErr(e.code === 1 ? t("gpsDenied") : e.code === 3 ? t("gpsTimeout") : t("gpsUnavailable"));
      },
      { enableHighAccuracy: true, timeout: 30_000, maximumAge: 60_000 },
    );
  };

  const save = async () => {
    if (!chosen) return;
    setSaving(true);
    setErr(null);
    try {
      await api("/api/home-location", { method: "PUT", json: { lat: chosen.lat, lon: chosen.lon, label: chosen.label, source: chosen.source } });
      setQ("");
      setNote(t("homeLocationSaved"));
      saved();
    } catch (e) {
      setErr(errText(t, e));
    } finally {
      setSaving(false);
    }
  };

  const remove = async () => {
    setErr(null);
    try {
      await api("/api/home-location", { method: "DELETE" });
      setNote(null);
      saved();
    } catch (e) {
      setErr(errText(t, e));
    }
  };

  const places = info?.places ?? false;
  return (
    <section className="panel left home-loc" aria-labelledby={headId}>
      <h2 id={headId}>{t("homeLocation")}</h2>
      {home ? (
        <div className="home-loc-now">
          <button type="button" className="home-loc-place" onClick={showHome} title={t("mapGoHome")}>
            <span className="home-loc-dot" aria-hidden="true" />
            <span>{homeText(home)}</span>
          </button>
          {home.set_at && (
            <p className="muted home-loc-meta">
              {t("homeLocationSetBy").replace("{who}", home.set_by === "laptop" ? t("homeLocationLaptop") : home.set_by ?? "")} · {fmtDateTime(home.set_at)}
            </p>
          )}
        </div>
      ) : (
        !editing && <p className="muted">{t("homeLocationNone")}</p>
      )}

      {!editing ? (
        <div className="row wrap">
          <button type="button" className={home ? "btn secondary" : "btn"} onClick={() => setEditing(true)}>
            {home ? t("homeLocationChange") : t("homeLocationSet")}
          </button>
          {home && (
            <ConfirmButton label={t("remove")} confirmLabel={t("yesRemove")} cancelLabel={t("cancel")} className="btn danger" onConfirm={remove} />
          )}
        </div>
      ) : (
        <div className="stack home-loc-edit">
          {phone && (
            <button type="button" className="btn secondary" onClick={locate} disabled={locating}>
              {locating ? t("gpsLocating") : t("gpsUse")}
            </button>
          )}
          {places && (
            <div className="home-loc-find">
              <label htmlFor={searchId}>{t("findPlace")}</label>
              <input
                id={searchId}
                type="search"
                className="search"
                value={q}
                onChange={(e) => setQ(e.target.value)}
                placeholder={t("findPlaceHint")}
                autoComplete="off"
                enterKeyHint="search"
              />
              {found && found.length === 0 && <p className="muted home-loc-none">{t("findPlaceNone")}</p>}
              {found && found.length > 0 && (
                <ul className="home-loc-results" aria-label={t("findPlaceResults")}>
                  {found.map((p) => (
                    <li key={`${p.name}|${p.lat}|${p.lon}`}>
                      <button
                        type="button"
                        className={"home-loc-result" + (chosen && chosen.lat === p.lat && chosen.lon === p.lon ? " chosen" : "")}
                        onClick={() => choose({ lat: p.lat, lon: p.lon, label: placeLabel(p, lang), source: "place" }, 12)}
                      >
                        {placeLabel(p, lang)}
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          )}
          <p className="muted home-loc-hint">{t("homeLocationTapHint")}</p>
          {chosen && (
            <p className="home-loc-chosen">
              <span className="muted">{t("mapChosenPlace")}:</span> {chosen.label ?? coords(chosen)}
            </p>
          )}
          <div className="row wrap">
            <button type="button" className="btn" onClick={save} disabled={!chosen || saving}>
              {t("homeLocationSave")}
            </button>
            <button type="button" className="btn secondary" onClick={() => setEditing(false)}>
              {t("cancel")}
            </button>
          </div>
        </div>
      )}
      {note && !err && <p className="ok home-loc-note" role="status">{note}</p>}
      {err && <p className="error" role="alert">{err}</p>}
      <p className="muted home-loc-privacy">{t("homeLocationPrivate")}</p>
    </section>
  );
}
