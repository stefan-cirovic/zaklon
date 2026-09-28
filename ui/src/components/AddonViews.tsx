import { useId, type ReactNode } from "react";
import type { Key } from "../i18n";
import { countWord, fmtBytes } from "../format";
import { folderOf, type Entry, type FolderId, type FolderStat, type ViewMode } from "../addons";
import { DriveIcon, FolderIcon, GlyphIcon, type DriveKind } from "./ExplorerIcons";

type T = (k: Key) => string;

/** "7 add-ons · 88 GB" or "214 countries · 120 GB". */
function countLine(t: T, s: FolderStat) {
  const [one, few, many] = s.id === "maps" ? (["countryWord1", "countryWord2", "countryWord5"] as const) : (["addonWord1", "addonWord2", "addonWord5"] as const);
  return `${s.count} ${countWord(s.count, [t(one), t(many)], [t(one), t(few), t(many)])} · ${fmtBytes(s.size)}`;
}

/** What of a folder is on the hub: "Downloading 45%", "On the hub: 2 · 14 GB", or nothing. */
function onHubLine(t: T, s: FolderStat): { text: string; tone: "ok" | "muted" } | null {
  if (s.busy) return { text: `${t("downloadingNow")}${s.progress !== null ? ` ${s.progress}%` : "…"}`, tone: "muted" };
  if (s.installed > 0) return { text: `${t("colOnHub")}: ${s.installed} · ${fmtBytes(s.installedBytes)}`, tone: "ok" };
  return null;
}

function Bar({ pct, label }: { pct: number; label: string }) {
  return (
    <div className="bar" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct} aria-label={label}>
      <i style={{ width: `${pct}%` }} />
    </div>
  );
}

/** The folders: big tiles, or rows with their numbers. */
export function Folders({ t, stats, view, open }: { t: T; stats: FolderStat[]; view: ViewMode; open: (id: FolderId) => void }) {
  if (view === "details") {
    return (
      <div className="details folders" role="table" aria-label={t("addonsFolders")}>
        <div className="d-row d-head" role="row">
          <div role="columnheader">{t("colName")}</div>
          <div role="columnheader" className="d-size">{t("colItems")}</div>
          <div role="columnheader" className="d-size">{t("colSize")}</div>
          <div role="columnheader">{t("colOnHub")}</div>
        </div>
        {stats.map((s) => {
          const f = folderOf(s.id);
          const hub = onHubLine(t, s);
          return (
            <div className="d-row clickable" role="row" key={s.id} onClick={() => open(s.id)}>
              <div className="d-name" role="cell">
                <button
                  className="d-open"
                  onClick={(e) => {
                    e.stopPropagation();
                    open(s.id);
                  }}
                >
                  <FolderIcon glyph={f.glyph} size={26} />
                  <span className="d-title">
                    <span className="entry-name">{t(f.name)}</span>
                    <span className="muted d-sub phone-only">{countLine(t, s)}</span>
                  </span>
                </button>
              </div>
              <div className="d-size" role="cell">{s.count}</div>
              <div className="d-size" role="cell">{fmtBytes(s.size)}</div>
              <div role="cell" className={hub?.tone === "ok" ? "ok" : "muted"}>{hub ? hub.text : "–"}</div>
            </div>
          );
        })}
      </div>
    );
  }
  return (
    <ul className="folder-grid" aria-label={t("addonsFolders")}>
      {stats.map((s) => (
        <li key={s.id}>
          <FolderTile t={t} s={s} open={open} />
        </li>
      ))}
    </ul>
  );
}

function FolderTile({ t, s, open }: { t: T; s: FolderStat; open: (id: FolderId) => void }) {
  const f = folderOf(s.id);
  const id = useId();
  const hub = onHubLine(t, s);
  return (
    <button className="folder-tile" onClick={() => open(s.id)} aria-describedby={id}>
      <FolderIcon glyph={f.glyph} />
      <span className="folder-text">
        <span className="folder-name">{t(f.name)}</span>
        <span id={id} className="folder-meta">
          <span className="muted">{countLine(t, s)}</span>
          {hub && <span className={hub.tone}>{hub.text}</span>}
        </span>
        {s.busy && s.progress !== null && (
          <span className="bar thin" aria-hidden="true">
            <i style={{ width: `${s.progress}%` }} />
          </span>
        )}
      </span>
    </button>
  );
}

/**
 * Packs and maps: cards, or a table with name, size, status and license. In a
 * folder (`folder`) each shows that folder's icon; elsewhere (search results,
 * the drive) the folders it is in are named, as a pack can be in several.
 */
export function Entries({ t, entries, view, folder, label }: { t: T; entries: Entry[]; view: ViewMode; folder?: FolderId; label: string }) {
  const glyph = (e: Entry) => folderOf(folder ?? e.folders[0]).glyph;
  const where = (e: Entry) => e.folders.map((f) => t(folderOf(f).name)).join(", ");
  const showFolder = !folder;
  if (view === "details") {
    return (
      <div className="details entries" role="table" aria-label={label}>
        <div className="d-row d-head" role="row">
          <div role="columnheader">{t("colName")}</div>
          <div role="columnheader" className="d-size">{t("colSize")}</div>
          <div role="columnheader">{t("colStatus")}</div>
          <div role="columnheader">{t("colLicense")}</div>
          <div role="columnheader" className="sr-only">{t("colActions")}</div>
        </div>
        {entries.map((e) => (
          <div className="d-row entry-row" role="row" key={e.key} data-entry={e.key}>
            <div className="d-name" role="cell">
              <span className="entry-icon small">
                <GlyphIcon glyph={glyph(e)} size={18} />
              </span>
              <span className="d-title">
                <span className="entry-name">
                  {e.name} {e.recommended && <span className="badge ok">{t("recommended")}</span>}
                </span>
                <span className="muted d-sub" title={e.desc || undefined}>{showFolder ? where(e) : e.desc}</span>
              </span>
            </div>
            <div className="d-size" role="cell">{fmtBytes(e.size)}</div>
            <div className="d-status" role="cell">
              <span className={e.tone}>{e.status}</span>
              {e.progress !== null && <Bar pct={e.progress} label={e.name} />}
              {e.detail && <span className="muted d-detail">{e.detail}</span>}
              {e.error && <span className="warn d-detail">{e.error}</span>}
            </div>
            <div className="d-license muted" role="cell">{e.license}</div>
            <div className="d-actions" role="cell">{e.actions(true)}</div>
          </div>
        ))}
      </div>
    );
  }
  return (
    <div className="list cols entry-grid" role="list" aria-label={label}>
      {entries.map((e) => (
        <div className="item entry-tile" role="listitem" key={e.key} data-entry={e.key}>
          <div className="entry-head">
            <span className="entry-icon">
              <GlyphIcon glyph={glyph(e)} />
            </span>
            <div className="entry-main">
              <div className="entry-name">
                {e.name} {e.recommended && <span className="badge ok">{t("recommended")}</span>}
              </div>
              {e.desc && <div className="muted small-text">{e.desc}</div>}
              <div className="muted small-text">
                {fmtBytes(e.size)}
                {e.meta && ` · ${e.meta}`}
                {showFolder && ` · ${where(e)}`}
              </div>
            </div>
          </div>
          {e.progress !== null && (
            <div>
              <Bar pct={e.progress} label={e.name} />
              {e.detail && <div className="muted small-text">{e.detail}</div>}
            </div>
          )}
          <div className="entry-foot">
            <span className={e.tone}>{e.status}</span>
            <div className="row wrap entry-actions">{e.actions(false)}</div>
          </div>
          {e.error && <div className="warn small-text">{e.error}</div>}
        </div>
      ))}
    </div>
  );
}

/**
 * A drive with how full it is, as Explorer's "This PC" shows it; the bar
 * turns red when the drive is nearly full. Opens when `open` is given.
 */
export function DriveTile({
  t,
  kind,
  name,
  badge,
  free,
  total,
  note,
  open,
}: {
  t: T;
  kind: DriveKind;
  name: string;
  badge?: string;
  free: number;
  total: number;
  note?: ReactNode;
  open?: () => void;
}) {
  const id = useId();
  const used = total > 0 ? Math.min(100, Math.max(0, Math.round(((total - free) / total) * 100))) : 0;
  const full = total > 0 && used >= 90;
  const body = (
    <>
      <DriveIcon kind={kind} />
      <span className="drive-text">
        <span className="drive-name">
          <span className="drive-title">{name}</span>
          {badge && <span className="badge">{badge}</span>}
        </span>
        {total > 0 && (
          <span className={"usage" + (full ? " full" : "")} aria-hidden="true">
            <i style={{ width: `${used}%` }} />
          </span>
        )}
        <span id={id} className="drive-free">
          {total > 0 && (
            <span className="muted">
              {fmtBytes(free)} {t("free")} {t("of")} {fmtBytes(total)}
            </span>
          )}
          {full && <span className="warn">{t("diskNearlyFull")}</span>}
          {note}
        </span>
      </span>
    </>
  );
  return open ? (
    <button className="drive-tile" onClick={open} aria-describedby={id}>
      {body}
    </button>
  ) : (
    <div className="drive-tile">{body}</div>
  );
}

/** The laptop's battery, next to the drives: downloads wait when it is low. */
export function BatteryTile({ t, percent, plugged }: { t: T; percent: number; plugged: boolean }) {
  return (
    <div className="drive-tile">
      <DriveIcon kind="battery" level={percent / 100} />
      <span className="drive-text">
        <span className="drive-name">
          <span className="drive-title">{t("battery")}</span>
        </span>
        <span className="drive-free">
          <span className="muted">
            {percent}% · {plugged ? t("pluggedIn") : t("onBattery")}
          </span>
          {!plugged && percent < 50 && <span className="warn">{t("batteryRule")}</span>}
        </span>
      </span>
    </div>
  );
}
