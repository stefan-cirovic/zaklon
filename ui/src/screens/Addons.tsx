import { useCallback, useEffect, useId, useRef, useState, type ReactNode } from "react";
import { api } from "../api";
import type { Key, Lang } from "../i18n";
import { errText } from "../errors";
import { useVisiblePoll } from "../poll";
import { fmtBytes, fold } from "../format";
import ConfirmButton from "../components/ConfirmButton";
import { CopyToUsb, DrivePicker, type CopyItem, type Drive } from "../components/Usb";
import StarterSet from "../components/StarterSet";
import HelpLink from "../components/HelpLink";
import { BatteryTile, DriveTile, Entries, Folders } from "../components/AddonViews";
import { ToolIcon } from "../components/ExplorerIcons";
import {
  BUSY,
  FOLDERS,
  folderOf,
  folderStat,
  isFolderId,
  packFolder,
  rootName,
  WITH_BAR,
  type CatalogReply,
  type Entry,
  type FolderId,
  type Localized,
  type Pack,
  type ViewMode,
} from "../addons";
import { countryState, SUGGESTED, type Country, type MapsReply } from "../maps";

type T = (k: Key) => string;
type Props = { t: T; lang: Lang; isHub: boolean };

/**
 * Where on the screen one is, as in a file explorer: the top ("This PC"), a
 * folder, the hub's drive with what is on it, or (laptop) a USB drive to copy
 * to or import from. It is part of the address (#addons/maps), so the
 * browser's and the phone's Back button go back to the top.
 */
type Loc = { kind: "root" } | { kind: "folder"; id: FolderId } | { kind: "library" } | { kind: "drive"; letter: string };

const ROOT: Loc = { kind: "root" };
const VIEW_KEY = "zaklon.addonsView";

function locFromHash(): Loc {
  const [tab, a, b] = location.hash.replace(/^#/, "").split("/");
  if (tab !== "addons") return ROOT;
  if (a === "library") return { kind: "library" };
  if (a === "drive" && b && /^[A-Za-z]$/.test(b)) return { kind: "drive", letter: b.toUpperCase() };
  if (isFolderId(a)) return { kind: "folder", id: a };
  return ROOT;
}

function hashOf(l: Loc): string {
  if (l.kind === "folder") return `addons/${l.id}`;
  if (l.kind === "library") return "addons/library";
  if (l.kind === "drive") return `addons/drive/${l.letter}`;
  return "addons";
}

function readView(): ViewMode {
  try {
    return localStorage.getItem(VIEW_KEY) === "details" ? "details" : "tiles";
  } catch {
    return "tiles";
  }
}

function sameRoot(a: string, b: string) {
  return !!a && !!b && rootName(a).toUpperCase() === rootName(b).toUpperCase();
}

export default function Addons({ t, lang, isHub }: Props) {
  const [data, setData] = useState<CatalogReply | null>(null);
  const [maps, setMaps] = useState<MapsReply | null>(null);
  const [drives, setDrives] = useState<Drive[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [loc, setLoc] = useState<Loc>(locFromHash);
  const [query, setQuery] = useState("");
  const [view, setViewState] = useState<ViewMode>(readView);
  const headRef = useRef<HTMLHeadingElement>(null);
  const drivesId = useId();
  const foldersId = useId();

  useEffect(() => {
    const onHash = () => setLoc(locFromHash());
    window.addEventListener("hashchange", onHash);
    window.addEventListener("popstate", onHash);
    return () => {
      window.removeEventListener("hashchange", onHash);
      window.removeEventListener("popstate", onHash);
    };
  }, []);

  // A new place starts at the top, with the keyboard and screen reader on its name;
  // back at the top, the page is where it was left.
  const where = hashOf(loc);
  const shownWhere = useRef(where);
  useEffect(() => {
    if (shownWhere.current === where) return;
    shownWhere.current = where;
    window.scrollTo(0, where === "addons" ? Number(history.state?.addonsScroll ?? 0) : 0);
    headRef.current?.focus({ preventScroll: true });
  }, [where]);

  // Every place opened here is a step in the browser's history (marked, with how far
  // the page was scrolled), so Back returns through the history like the browser's.
  const go = (l: Loc) => {
    history.replaceState({ ...history.state, addonsScroll: window.scrollY }, "");
    history.pushState({ addonsStep: true }, "", `#${hashOf(l)}`);
    setQuery("");
    setLoc(l);
  };
  const goRoot = () => {
    setQuery("");
    if (loc.kind === "root") return;
    if (history.state?.addonsStep) {
      history.back();
    } else {
      // Came here from another screen (#addons/models): up, without leaving Add-ons.
      history.replaceState(null, "", "#addons");
      setLoc(ROOT);
    }
  };
  const back = () => (query ? setQuery("") : goRoot());

  const setView = (v: ViewMode) => {
    setViewState(v);
    try {
      localStorage.setItem(VIEW_KEY, v);
    } catch {
      /* private mode: just for now */
    }
  };

  const load = useCallback(async () => {
    try {
      setData(await api<CatalogReply>("/api/catalog"));
      setErr(null);
      return true;
    } catch (e) {
      setErr(errText(t, e));
      return false;
    }
  }, [t]);
  const busy = data?.packs.some((p) => BUSY.includes(p.state.status)) ?? false;
  useVisiblePoll(load, busy ? 1500 : 10000);

  const loadMaps = useCallback(async () => {
    try {
      setMaps(await api<MapsReply>("/api/maps"));
      return true;
    } catch {
      return false;
    }
  }, []);
  const mapsBusy = maps?.countries.some((c) => countryState(c).busy) ?? false;
  const refreshMaps = useVisiblePoll(loadMaps, mapsBusy ? 2000 : 15000);

  // The laptop's drives: when the screen opens, when the window gets the focus again
  // (a stick was just plugged in), and on Refresh.
  const loadDrives = useCallback(() => {
    if (!isHub) return;
    api<Drive[]>("/api/drives")
      .then(setDrives)
      .catch(() => setDrives((d) => d ?? []));
  }, [isHub]);
  useEffect(() => {
    loadDrives();
    window.addEventListener("focus", loadDrives);
    return () => window.removeEventListener("focus", loadDrives);
  }, [loadDrives]);

  const act = async (path: string, method: string, then: () => void) => {
    try {
      await api(path, { method });
      then();
    } catch (e) {
      setErr(errText(t, e));
    }
  };

  const title = (l: Localized) => (lang === "sr" && l.sr ? l.sr : l.en);
  const nameOf = (x: { name: string; name_sr: string }) => (lang === "sr" ? x.name_sr : x.name);
  // In the details view every button is a quiet one: a column of accent buttons would shout.
  const btn = (small: boolean, primary: boolean) => (primary && !small ? "btn" : "btn secondary") + (small ? " small" : "");

  const packEntry = (p: Pack): Entry => {
    const s = p.state;
    const name = title(p.title);
    const pct = s.bytes_total ? Math.min(100, Math.round((s.bytes_done / s.bytes_total) * 100)) : 0;
    const url = `/api/packs/${encodeURIComponent(p.id)}`;
    const download = () => act(`${url}/download`, "POST", load);
    const pause = () => act(`${url}/pause`, "POST", load);
    let status = t("notDownloaded");
    let tone: Entry["tone"] = "muted";
    if (s.status === "queued") status = t("queued");
    else if (s.status === "downloading") status = `${t("downloadingNow")} ${pct}%`;
    else if (s.status === "verifying") status = t("verifying");
    else if (s.status === "paused") status = `${t("pausedStatus")} ${pct}%`;
    else if (s.status === "failed") {
      status = t("failedStatus");
      tone = "warn";
    } else if (s.status === "installed") {
      status = s.update_available ? t("packUpdateAvailable") : t("installed");
      tone = s.update_available ? "warn" : "ok";
    }
    const remove = (small: boolean) =>
      isHub && (
        <ConfirmButton
          label={t("remove")}
          ariaLabel={`${t("remove")}: ${name}`}
          confirmLabel={t("yesRemove")}
          cancelLabel={t("cancel")}
          className={"btn danger" + (small ? " small" : "")}
          onConfirm={() => act(url, "DELETE", load)}
        />
      );
    const button = (small: boolean, primary: boolean, label: string, onClick: () => void) => (
      <button className={btn(small, primary)} aria-label={`${label}: ${name}`} onClick={onClick}>
        {label}
      </button>
    );
    return {
      key: p.id,
      folder: packFolder(p),
      name,
      desc: title(p.description),
      size: p.size,
      meta: `${p.version} · ${p.license}`,
      license: p.license,
      recommended: p.recommended_for.includes(lang),
      status,
      tone,
      progress: WITH_BAR.includes(s.status) ? pct : null,
      detail:
        (s.status === "downloading" || s.status === "paused" || s.status === "queued") && s.bytes_total
          ? `${fmtBytes(s.bytes_done)} / ${fmtBytes(s.bytes_total)}${s.status === "downloading" && s.speed > 0 ? ` · ${fmtBytes(s.speed)}/s` : ""}`
          : null,
      error: s.status === "failed" && s.error ? errText(t, new Error(s.error)) : null,
      actions: (small) => (
        <>
          {s.status === "not_installed" && button(small, true, t("download"), download)}
          {s.status === "queued" && button(small, false, t("cancel"), pause)}
          {(s.status === "downloading" || s.status === "verifying") && button(small, false, t("pause"), pause)}
          {(s.status === "paused" || s.status === "failed") && button(small, true, s.status === "paused" ? t("resume") : t("retry"), download)}
          {s.status === "installed" && s.update_available && button(small, true, t("packUpdate"), download)}
          {(s.status === "paused" || s.status === "failed" || s.status === "installed") && remove(small)}
        </>
      ),
      onDisk: s.status === "installed" || s.bytes_done > 0 || BUSY.includes(s.status),
      installed: s.status === "installed",
      installedBytes: p.size,
      busy: BUSY.includes(s.status),
      bytesDone: s.status === "installed" ? p.size : s.bytes_done,
      bytesTotal: s.bytes_total || p.size,
      search: fold([p.title.en, p.title.sr, p.description.en, p.description.sr, p.id].join(" ")),
    };
  };

  const countryEntry = (c: Country): Entry => {
    const s = countryState(c);
    const name = nameOf(c);
    const pct = c.size ? Math.min(100, Math.round((s.done / c.size) * 100)) : 0;
    const url = `/api/maps/${encodeURIComponent(c.id)}`;
    let status = t("notDownloaded");
    let tone: Entry["tone"] = "muted";
    if (s.busy) status = `${t("downloadingNow")} ${pct}%`;
    else if (s.all) {
      status = s.update ? t("packUpdateAvailable") : t("installed");
      tone = s.update ? "warn" : "ok";
    } else if (s.failed) {
      status = t("failedStatus");
      tone = "warn";
    } else if (s.paused) status = `${t("pausedStatus")} ${pct}%`;
    else if (s.some) status = `${s.installed}/${c.regions.length} ${t("installed").toLowerCase()}`;
    const label = s.update ? t("packUpdate") : s.some || s.paused ? t("resume") : s.failed ? t("retry") : t("download");
    const installedBytes = c.regions.reduce((sum, r) => sum + (r.status === "installed" ? r.size : 0), 0);
    return {
      key: `map:${c.id}`,
      folder: "maps",
      name,
      desc: c.regions.length > 1 ? `${c.regions.length} ${t("mapsRegions")}` : "",
      size: c.size,
      meta: "ODbL-1.0",
      license: "ODbL-1.0",
      recommended: false,
      status,
      tone,
      progress: s.busy || s.paused ? pct : null,
      detail: s.busy ? `${fmtBytes(s.done)} / ${fmtBytes(c.size)}` : null,
      error: s.failed && !s.busy ? t("mapsSomeFailed") : null,
      actions: (small) => (
        <>
          {!(s.all && !s.update) && !s.busy && (
            <button className={btn(small, true)} aria-label={`${label}: ${name}`} onClick={() => act(`${url}/download`, "POST", refreshMaps)}>
              {label}
            </button>
          )}
          {isHub && (s.some || s.failed || s.paused) && !s.busy && (
            <ConfirmButton
              label={t("remove")}
              ariaLabel={`${t("remove")}: ${name}`}
              confirmLabel={t("yesRemove")}
              cancelLabel={t("cancel")}
              className={"btn danger" + (small ? " small" : "")}
              onConfirm={() => act(url, "DELETE", refreshMaps)}
            />
          )}
        </>
      ),
      onDisk: s.some || s.busy || s.paused || s.done > 0,
      installed: s.some,
      installedBytes,
      busy: s.busy,
      bytesDone: s.done,
      bytesTotal: c.size,
      search: fold([c.name, c.name_sr, ...c.regions.flatMap((r) => [r.name, r.name_sr])].join(" ")),
    };
  };

  const folderIndex = (id: FolderId) => FOLDERS.findIndex((f) => f.id === id);
  const packEntries = (data?.packs ?? []).map(packEntry).sort((a, b) => folderIndex(a.folder) - folderIndex(b.folder));
  // Countries with something on the hub first, then the ones suggested for the language, then A to Z.
  const suggested = SUGGESTED[lang];
  const rank = (c: Country) => {
    const s = countryState(c);
    if (s.some || s.busy || s.paused || s.failed) return -1;
    const i = suggested.indexOf(c.id);
    return i === -1 ? 100 : i;
  };
  const countries = [...(maps?.countries ?? [])].sort((a, b) => rank(a) - rank(b) || nameOf(a).localeCompare(nameOf(b), lang));
  const all = [...packEntries, ...countries.map(countryEntry)];

  const words = fold(query.trim()).split(/\s+/).filter(Boolean);
  const searching = words.length > 0;
  const results = searching ? all.filter((e) => words.every((w) => e.search.includes(w))) : [];

  // Apps (library engine, CoMaps) come along automatically with what needs them.
  const copyItems: CopyItem[] = [
    ...(data?.packs ?? [])
      .filter((p) => p.state.status === "installed" && p.category !== "app")
      .map((p) => ({ key: p.id, label: title(p.title), ids: [p.id], size: p.size })),
    ...(maps?.countries ?? [])
      .filter((c) => c.regions.some((r) => r.status === "installed"))
      .map((c) => {
        const regions = c.regions.filter((r) => r.status === "installed");
        return {
          key: `map-country:${c.id}`,
          label: `${t("mapsOf")}: ${nameOf(c)}`,
          ids: regions.map((r) => `map:${r.id}`),
          size: regions.reduce((s, r) => s + r.size, 0),
        };
      }),
  ];

  // The drives: the one the library is on, then the others (USB first).
  const libRoot = data?.library_drive ?? "";
  const driveName = (d: Drive) => `${d.label || (d.kind === "removable" ? t("usbDrive") : t("localDisk"))} (${rootName(d.path)})`;
  const libDrive = drives?.find((d) => sameRoot(d.path, libRoot));
  const libName = libDrive ? driveName(libDrive) : `${t("hubDisk")}${/^[A-Za-z]:/.test(libRoot) ? ` (${rootName(libRoot)})` : ""}`;
  const otherDrives = (drives ?? [])
    .filter((d) => !sameRoot(d.path, libRoot))
    .sort((a, b) => Number(b.kind === "removable") - Number(a.kind === "removable") || a.path.localeCompare(b.path));
  const addonsBytes = (data?.packs ?? []).reduce((s, p) => s + (p.state.status === "installed" ? p.size : 0), 0) + (maps?.installed_bytes ?? 0);
  const letterOf = (d: Drive) => d.path.charAt(0).toUpperCase();
  const openDrive = loc.kind === "drive" && isHub ? (drives?.find((d) => letterOf(d) === loc.letter) ?? null) : null;
  const here: Loc = loc.kind === "drive" && !isHub ? ROOT : loc;

  const libraryTile = (open: boolean) =>
    data && (
      <DriveTile
        t={t}
        kind="disk"
        name={libName}
        badge={t("libraryDriveBadge")}
        free={data.system.disk_free}
        total={data.system.disk_total}
        note={
          <span className="muted">
            {t("addonsUse")}: {fmtBytes(addonsBytes)}
          </span>
        }
        open={open ? () => go({ kind: "library" }) : undefined}
      />
    );

  const [heading, intro] =
    here.kind === "folder"
      ? [t(folderOf(here.id).name), t(folderOf(here.id).desc)]
      : here.kind === "library"
        ? [libName, t("libraryDriveIntro")]
        : here.kind === "drive"
          ? [openDrive ? driveName(openDrive) : `${t("usbDrive")} (${here.letter}:)`, t("usbDriveIntro")]
          : [t("addons"), t("addonsIntro")];

  const crumbs: { label: string; onClick?: () => void }[] = [{ label: t("addons"), onClick: here.kind !== "root" || searching ? goRoot : undefined }];
  if (here.kind !== "root") crumbs.push({ label: heading, onClick: searching ? () => setQuery("") : undefined });
  if (searching) crumbs.push({ label: t("searchResults") });

  let content: ReactNode = null;
  if (!data) {
    content = null;
  } else if (searching) {
    content = (
      <section className="stack explorer-section" aria-label={t("searchResults")}>
        <h2 className="section-title">
          {t("searchResults")} <span className="muted">({results.length})</span>
        </h2>
        {results.length === 0 ? <p className="muted">{t("noResults")}</p> : <Entries t={t} entries={results} view={view} showFolder label={t("searchResults")} />}
      </section>
    );
  } else if (here.kind === "folder") {
    const entries = all.filter((e) => e.folder === here.id);
    content = (
      <>
        {here.id === "maps" && (
          <p className="muted folder-note">
            {t("mapsPhoneNote")} <a href="#maps">{t("openMaps")}</a>
          </p>
        )}
        {entries.length === 0 ? (
          <p className="muted">{here.id === "maps" && !maps ? t("aiLoading") : t("emptyFolder")}</p>
        ) : (
          <Entries t={t} entries={entries} view={view} showFolder={false} label={heading} />
        )}
      </>
    );
  } else if (here.kind === "library") {
    const onDisk = all.filter((e) => e.onDisk).sort((a, b) => (b.installed ? b.installedBytes : b.bytesDone) - (a.installed ? a.installedBytes : a.bytesDone));
    content = (
      <>
        <div className="drive-grid">{libraryTile(false)}</div>
        {onDisk.length === 0 ? <p className="muted">{t("nothingInstalled")}</p> : <Entries t={t} entries={onDisk} view={view} showFolder label={libName} />}
      </>
    );
  } else if (here.kind === "drive") {
    content =
      drives === null ? (
        <p className="muted">{t("aiLoading")}</p>
      ) : openDrive ? (
        <>
          <div className="drive-grid">
            <DriveTile
              t={t}
              kind={openDrive.kind === "removable" ? "usb" : "disk"}
              name={driveName(openDrive)}
              badge={openDrive.kind === "removable" ? t("driveRemovable") : undefined}
              free={openDrive.free}
              total={openDrive.total}
              note={<span className="muted">{openDrive.file_system}</span>}
            />
          </div>
          <div className="transfer">
            <CopyToUsb key={`copy-${openDrive.path}`} t={t} lang={lang} items={copyItems} dir={openDrive.path} />
            <ImportPanel key={`import-${openDrive.path}`} t={t} dir={openDrive.path} onStarted={load} />
          </div>
        </>
      ) : (
        <div className="stack">
          <p className="muted" style={{ margin: 0 }}>{t("driveGone")}</p>
          <div>
            <button className="btn secondary" onClick={loadDrives}>{t("refreshDrives")}</button>
          </div>
        </div>
      );
  } else {
    const stats = FOLDERS.map((f) => folderStat(f.id, all)).filter((s) => s.count > 0);
    content = (
      <>
        <section className="stack explorer-section" aria-labelledby={drivesId}>
          <div className="row between">
            <h2 id={drivesId} className="section-title">{t("devicesAndDrives")}</h2>
            {isHub && (
              <button className="btn secondary small icon-btn" onClick={loadDrives} aria-label={t("refreshDrives")} title={t("refreshDrives")}>
                <ToolIcon name="refresh" size={16} />
              </button>
            )}
          </div>
          <div className="drive-grid">
            {libraryTile(true)}
            {otherDrives.map((d) => (
              <DriveTile
                key={d.path}
                t={t}
                kind={d.kind === "removable" ? "usb" : "disk"}
                name={driveName(d)}
                badge={d.kind === "removable" ? t("driveRemovable") : undefined}
                free={d.free}
                total={d.total}
                open={() => go({ kind: "drive", letter: letterOf(d) })}
              />
            ))}
            {data.system.battery_percent !== null && <BatteryTile t={t} percent={data.system.battery_percent} plugged={data.system.plugged_in} />}
          </div>
        </section>
        <section className="stack explorer-section" aria-labelledby={foldersId}>
          <h2 id={foldersId} className="section-title">{t("addonsFolders")}</h2>
          <Folders t={t} stats={stats} view={view} open={(id) => go({ kind: "folder", id })} />
        </section>
        {/* Like Explorer: drives and folders first, then the one-click starter set. */}
        {isHub && <StarterSet t={t} lang={lang} packs={data.packs} freeBytes={data.system.disk_free} onStarted={load} />}
        {isHub && (
          <div className="transfer">
            <CopyToUsb t={t} lang={lang} items={copyItems} />
            <ImportPanel t={t} onStarted={load} />
          </div>
        )}
      </>
    );
  }

  return (
    <div className="stack addons">
      <div className="page-head">
        <div className="title-line">
          <h1 ref={headRef} tabIndex={-1}>{heading}</h1>
          <HelpLink t={t} topic="addons" />
        </div>
        <p className="muted">{intro}</p>
      </div>
      <div className="explorer-bar">
        <button className="btn secondary icon-btn" onClick={back} disabled={here.kind === "root" && !searching} aria-label={t("back")} title={t("back")}>
          <ToolIcon name="back" />
        </button>
        <nav className="crumbs" aria-label={t("addonsLocation")}>
          <ol>
            {crumbs.map((c, i) => (
              <li key={i}>
                {i > 0 && <ToolIcon name="chevron" size={14} />}
                {c.onClick ? (
                  <button className="crumb" onClick={c.onClick}>{c.label}</button>
                ) : (
                  <span className="crumb current" aria-current="page">{c.label}</span>
                )}
              </li>
            ))}
          </ol>
        </nav>
        <input
          type="search"
          className="search explorer-search"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t("addonsSearch")}
          aria-label={t("addonsSearch")}
        />
        <div className="segmented view-toggle" role="group" aria-label={t("addonsView")}>
          {(["tiles", "details"] as const).map((v) => (
            <button
              key={v}
              className={view === v ? "active" : ""}
              aria-pressed={view === v}
              aria-label={v === "tiles" ? t("viewTiles") : t("viewDetails")}
              title={v === "tiles" ? t("viewTiles") : t("viewDetails")}
              onClick={() => setView(v)}
            >
              <ToolIcon name={v} />
            </button>
          ))}
        </div>
      </div>
      {err && <p className="error" role="alert">{err}</p>}
      {!data && !err && <p className="muted">{t("aiLoading")}</p>}
      {content}
    </div>
  );
}

/** Import packs from a USB stick or a folder; the hub copies them in the background. */
function ImportPanel({ t, dir: initialDir = "", onStarted }: { t: T; dir?: string; onStarted: () => void }) {
  const [dir, setDir] = useState(initialDir);
  const [note, setNote] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [importing, setImporting] = useState(false);

  const doImport = async () => {
    setNote(null);
    setErr(null);
    setImporting(true);
    try {
      // Each pack shows its progress in its folder.
      const r = await api<{ importing: string[] }>("/api/packs/import", { json: { dir } });
      setNote(r.importing.length ? `${t("importing")}: ${r.importing.length}` : t("nothingToImport"));
      onStarted();
    } catch (e) {
      setErr(errText(t, e));
    } finally {
      setImporting(false);
    }
  };

  return (
    <div className="panel stack left">
      <h2>{t("importTitle")}</h2>
      <p className="muted" style={{ margin: 0 }}>{t("importIntro")}</p>
      <DrivePicker t={t} value={dir} onChange={setDir} label={t("importTitle")} />
      <div>
        <button className="btn secondary" onClick={doImport} disabled={!dir.trim() || importing}>
          {importing ? `${t("importing")}…` : t("importBtn")}
        </button>
      </div>
      {err && <p className="error" role="alert">{err}</p>}
      {note && <p className="ok" role="status">{note}</p>}
    </div>
  );
}
