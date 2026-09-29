import { useCallback, useEffect, useId, useState, type ReactNode } from "react";
import { api } from "../api";
import type { Key, Lang } from "../i18n";
import { errText } from "../errors";
import { fmtBytes } from "../format";
import { CopyToUsb, DrivePicker, type CopyItem, type Drive } from "../components/Usb";
import StarterSet from "../components/StarterSet";
import RichText from "../components/RichText";
import { BatteryTile, DriveTile, Entries, Folders } from "../components/PackViews";
import { ToolIcon } from "../components/ExplorerIcons";
import { isKindId, KINDS, kindOf, kindStat, rootName, type Entry, type KindId, type ViewMode } from "../packs";
import { countryState, SUGGESTED, type Country } from "../maps";
import { usePacks } from "../usePacks";

type T = (k: Key) => string;
type Props = {
  t: T;
  lang: Lang;
  isHub: boolean;
  /** The place in the address after #settings/storage/: a folder, the hub's drive ("disk") or another drive ("drive-E"). */
  place: string | null;
};

/**
 * Where on the page one is, as in a file explorer: the top (the drives,
 * the downloads, the folders and USB), a folder of one kind (guides and
 * books, maps, AI models, programs), the hub's drive with what is on it,
 * or (laptop) another drive to copy to or import from. It is part of the
 * address (#settings/storage/maps), so the browser's and the phone's Back
 * button work too.
 */
type Loc = { kind: "root" } | { kind: "folder"; id: KindId } | { kind: "disk" } | { kind: "drive"; letter: string };

const ROOT: Loc = { kind: "root" };
const VIEW_KEY = "zaklon.storageView";
const BASE = "settings/storage";

function locOf(place: string | null): Loc {
  if (!place) return ROOT;
  if (isKindId(place)) return { kind: "folder", id: place };
  if (place === "disk") return { kind: "disk" };
  const drive = /^drive-([A-Za-z])$/.exec(place);
  if (drive) return { kind: "drive", letter: drive[1].toUpperCase() };
  // A setting on the top of the page (#settings/storage/copy-usb), or nothing known.
  return ROOT;
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

/** The notes of the folders: where new things of the kind come from. */
const FOLDER_NOTE: Record<KindId, Key> = {
  guides: "storageGuidesNote",
  maps: "storageMapsNote",
  models: "storageModelsNote",
  programs: "folderProgramsDesc",
};

/**
 * Settings › Storage & Downloads: what the hub keeps and how much space it
 * takes, laid out like File Explorer's "This PC". The drives with how full
 * they are, what is downloading (and what waits: paused, failed, a new
 * version), then what is on the hub in folders by kind, the starter set and
 * USB. Removing is on the laptop. New guides are downloaded in the Library,
 * maps in Tools › Maps and AI models in Settings › AI assistant.
 */
export default function Storage({ t, lang, isHub, place }: Props) {
  const packs = usePacks(t, lang, isHub, { maps: true, worldCheck: true, models: true });
  const { data, maps } = packs;
  const [drives, setDrives] = useState<Drive[] | null>(null);
  const [view, setViewState] = useState<ViewMode>(readView);
  const loc = locOf(place);
  const drivesId = useId();
  const downloadsId = useId();
  const foldersId = useId();

  const setView = (v: ViewMode) => {
    setViewState(v);
    try {
      localStorage.setItem(VIEW_KEY, v);
    } catch {
      /* private mode: just for now */
    }
  };

  // Every place opened here is a step in the browser's history (marked), so
  // Back returns through the history like the browser's.
  const go = (to: string) => {
    history.pushState({ storageStep: true }, "", `#${BASE}/${to}`);
    window.dispatchEvent(new HashChangeEvent("hashchange"));
  };
  const up = () => {
    if (loc.kind === "root") return;
    // Came here from another screen (#settings/storage/maps): up, without leaving the page.
    if (history.state?.storageStep) history.back();
    else location.replace(`#${BASE}`);
  };

  // The laptop's drives: when the page opens, when the window gets the focus again
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

  // Everything, as entries, removable here (on the laptop); but AI models are
  // chosen, downloaded and removed in Settings › AI assistant: here only the
  // space they take, and their downloads under way.
  const packEntries = (data?.packs ?? []).map((p) => {
    const e = packs.packEntry(p, { removable: p.category !== "model" });
    return e.kind === "models" && !e.busy && !e.stopped && !e.update ? { ...e, actions: () => null } : e;
  });
  // Countries with something on the hub first, then the ones suggested for the language, then A to Z.
  const suggested = SUGGESTED[lang];
  const rank = (c: Country) => {
    const s = countryState(c);
    if (s.some || s.busy || s.paused || s.failed) return -1;
    const i = suggested.indexOf(c.id);
    return i === -1 ? 100 : i;
  };
  const countries = [...(maps?.countries ?? [])].sort((a, b) => rank(a) - rank(b) || packs.nameOf(a).localeCompare(packs.nameOf(b), lang));
  const all: Entry[] = [...packEntries, ...countries.map((c) => packs.countryEntry(c, { removable: true }))];
  const onDisk = all.filter((e) => e.onDisk);
  const kindName = (e: Entry) => t(kindOf(e.kind).name);

  // Apps (library engine, CoMaps) come along automatically with what needs them.
  const copyItems: CopyItem[] = [
    // (Not a pack Zaklon no longer offers: it is not passed on.)
    ...(data?.packs ?? [])
      .filter((p) => p.state.status === "installed" && p.category !== "app" && !p.withdrawn)
      .map((p) => ({ key: p.id, label: packs.title(p.title), ids: [p.id], size: p.size })),
    ...(maps?.countries ?? [])
      .filter((c) => c.regions.some((r) => r.status === "installed"))
      .map((c) => {
        const regions = c.regions.filter((r) => r.status === "installed");
        return {
          key: `map-country:${c.id}`,
          label: `${t("mapsOf")}: ${packs.nameOf(c)}`,
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
  const usedBytes = (data?.packs ?? []).reduce((s, p) => s + (p.state.status === "installed" ? p.size : 0), 0) + (maps?.installed_bytes ?? 0);
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
            {t("storageUse")}: {fmtBytes(usedBytes)}
          </span>
        }
        open={open ? () => go("disk") : undefined}
      />
    );

  const placeName =
    here.kind === "folder"
      ? t(kindOf(here.id).name)
      : here.kind === "disk"
        ? libName
        : here.kind === "drive"
          ? openDrive
            ? driveName(openDrive)
            : `${t("usbDrive")} (${here.letter}:)`
          : null;

  let content: ReactNode = null;
  if (!data) {
    content = null;
  } else if (here.kind === "folder") {
    // Programs are listed whether they are on the hub or not: this is the one place to get one again.
    const entries = here.id === "programs" ? all.filter((e) => e.kind === "programs") : onDisk.filter((e) => e.kind === here.id);
    content = (
      <>
        <p className="muted folder-note">
          <RichText text={t(FOLDER_NOTE[here.id])} />
        </p>
        {entries.length === 0 ? (
          <p className="muted">{here.id === "maps" && !maps ? t("aiLoading") : t("emptyFolder")}</p>
        ) : (
          <Entries t={t} entries={entries} view={view} glyph={kindOf(here.id).glyph} label={placeName ?? ""} />
        )}
      </>
    );
  } else if (here.kind === "disk") {
    const largest = [...onDisk].sort((a, b) => (b.installed ? b.installedBytes : b.bytesDone) - (a.installed ? a.installedBytes : a.bytesDone));
    content = (
      <>
        <p className="muted folder-note">{t("libraryDriveIntro")}</p>
        <div className="drive-grid">{libraryTile(false)}</div>
        {largest.length === 0 ? <p className="muted">{t("nothingInstalled")}</p> : <Entries t={t} entries={largest} view={view} label={libName} where={kindName} />}
      </>
    );
  } else if (here.kind === "drive") {
    content =
      drives === null ? (
        <p className="muted">{t("aiLoading")}</p>
      ) : openDrive ? (
        <>
          <p className="muted folder-note">{t("usbDriveIntro")}</p>
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
            <ImportPanel key={`import-${openDrive.path}`} t={t} dir={openDrive.path} onStarted={packs.load} />
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
    // What waits for a download: under way first, then paused or failed, then new versions.
    const order = (e: Entry) => (e.busy ? 0 : e.stopped ? 1 : 2);
    const waiting = all.filter((e) => e.busy || e.stopped || (e.installed && e.update)).sort((a, b) => order(a) - order(b));
    const stats = KINDS.map((k) => kindStat(k.id, all));
    content = (
      <>
        <section id="set-drives" className="set-anchor stack explorer-section" aria-labelledby={drivesId} tabIndex={-1}>
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
                open={() => go(`drive-${letterOf(d)}`)}
              />
            ))}
            {data.system.battery_percent !== null && <BatteryTile t={t} percent={data.system.battery_percent} plugged={data.system.plugged_in} />}
          </div>
        </section>
        <section className="stack explorer-section" aria-labelledby={foldersId}>
          <h2 id={foldersId} className="section-title">{t("storageFolders")}</h2>
          <Folders t={t} stats={stats} view={view} open={(id) => go(id)} label={t("storageFolders")} />
        </section>
        <section id="set-downloads" className="set-anchor stack explorer-section" aria-labelledby={downloadsId} tabIndex={-1}>
          <h2 id={downloadsId} className="section-title">
            {t("downloads")} <span className="muted">({waiting.length})</span>
          </h2>
          {waiting.length === 0 ? (
            <p className="muted" style={{ margin: 0 }}>{t("homeNoDownloads")}</p>
          ) : (
            <Entries t={t} entries={waiting} view={view} label={t("downloads")} where={kindName} />
          )}
        </section>
        {/* Like Explorer: drives and folders first, then the one-click starter set. */}
        {isHub && (
          <div id="set-starter" className="set-anchor" tabIndex={-1}>
            <StarterSet t={t} lang={lang} packs={data.packs} sets={data.starter_sets ?? []} freeBytes={data.system.disk_free} onStarted={packs.load} />
          </div>
        )}
        {isHub && (
          <div className="transfer">
            <div id="set-copy-usb" className="set-anchor" tabIndex={-1}>
              <CopyToUsb t={t} lang={lang} items={copyItems} />
            </div>
            <div id="set-import" className="set-anchor" tabIndex={-1}>
              <ImportPanel t={t} onStarted={packs.load} />
            </div>
          </div>
        )}
      </>
    );
  }

  return (
    <div className="stack storage">
      <p className="muted storage-intro">
        <RichText text={t("storageIntro")} />
      </p>
      <div className="explorer-bar">
        <button className="btn secondary icon-btn" onClick={up} disabled={here.kind === "root"} aria-label={t("back")} title={t("back")}>
          <ToolIcon name="back" />
        </button>
        <nav className="crumbs" aria-label={t("storageLocation")}>
          <ol>
            <li>
              {placeName ? (
                <button className="crumb" onClick={up}>{t("storage")}</button>
              ) : (
                <span className="crumb current" aria-current="page">{t("storage")}</span>
              )}
            </li>
            {placeName && (
              <li>
                <ToolIcon name="chevron" size={14} />
                <span className="crumb current" aria-current="page">{placeName}</span>
              </li>
            )}
          </ol>
        </nav>
        <div className="segmented view-toggle" role="group" aria-label={t("viewAs")}>
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
      {packs.err && <p className="error" role="alert">{packs.err}</p>}
      {!data && !packs.err && <p className="muted">{t("aiLoading")}</p>}
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
      // Each pack shows its progress under Downloads.
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
