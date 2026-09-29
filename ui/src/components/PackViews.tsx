import { useEffect, useId, useRef, type ReactNode } from "react";
import type { Key } from "../i18n";
import { countWord, fmtBytes } from "../format";
import { kindOf, type Entry, type KindId, type KindStat, type ViewMode } from "../packs";
import { DriveIcon, FolderIcon, GlyphIcon, type DriveKind, type Glyph } from "./ExplorerIcons";

type T = (k: Key) => string;

/** "7 items · 88 GB". */
function countLine(t: T, s: KindStat) {
  return `${s.count} ${countWord(s.count, [t("itemWord1"), t("itemWord5")], [t("itemWord1"), t("itemWord2"), t("itemWord5")])} · ${fmtBytes(s.size)}`;
}

/** "https://www.appropedia.org/x" -> "appropedia.org". */
function siteName(url: string) {
  try {
    return new URL(url).host.replace(/^www\./, "");
  } catch {
    return url;
  }
}

/** A pack's license, its credit line and where it comes from, folded away until opened. */
function Credit({ t, e }: { t: T; e: Entry }) {
  if (!e.credit) return null;
  return (
    <details className="pack-credit small-text">
      <summary>{t("packCredit")}</summary>
      <p>
        {t("colLicense")}: {e.license}
      </p>
      {e.credit.attribution && <p>{e.credit.attribution}</p>}
      {e.credit.source && (
        <p>
          {t("packSource")}: {siteName(e.credit.source)}
        </p>
      )}
    </details>
  );
}

/**
 * The question before downloading a pack people download themselves: its
 * license, named, and a deliberate yes. Focus goes to the yes; Escape or
 * Cancel closes it. Also asks other questions before a download (removing
 * the old world map first), with their own yes (`yes`, a red one when `danger`).
 */
export function LicenseAsk({
  t,
  text,
  name,
  onYes,
  onNo,
  yes: yesLabel,
  danger = false,
}: {
  t: T;
  text: string;
  name: string;
  onYes: () => void;
  onNo: () => void;
  yes?: string;
  danger?: boolean;
}) {
  const yes = useRef<HTMLButtonElement>(null);
  const id = useId();
  useEffect(() => {
    yes.current?.focus();
  }, []);
  return (
    <div
      className="license-ask"
      role="group"
      aria-labelledby={id}
      onKeyDown={(ev) => {
        if (ev.key === "Escape") onNo();
      }}
    >
      <p id={id}>{text}</p>
      <div className="row wrap">
        <button ref={yes} className={danger ? "btn danger-solid" : "btn"} aria-label={`${yesLabel ?? t("offerAccept")}: ${name}`} onClick={onYes}>
          {yesLabel ?? t("offerAccept")}
        </button>
        <button className="btn secondary" onClick={onNo}>
          {t("cancel")}
        </button>
      </div>
    </div>
  );
}

function Bar({ pct, label }: { pct: number; label: string }) {
  return (
    <div className="bar" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct} aria-label={label}>
      <i style={{ width: `${pct}%` }} />
    </div>
  );
}

/** What of a kind is on the hub: "Downloading 45%", or nothing. */
function busyLine(t: T, s: KindStat): string | null {
  return s.busy ? `${t("downloadingNow")}${s.progress !== null ? ` ${s.progress}%` : "…"}` : null;
}

/** The folders of Storage & Downloads, one for each kind: big tiles, or rows with their numbers. */
export function Folders({ t, stats, view, open, label }: { t: T; stats: KindStat[]; view: ViewMode; open: (id: KindId) => void; label: string }) {
  if (view === "details") {
    return (
      <div className="details folders" role="table" aria-label={label}>
        <div className="d-row d-head" role="row">
          <div role="columnheader">{t("colName")}</div>
          <div role="columnheader" className="d-size">{t("colItems")}</div>
          <div role="columnheader" className="d-size">{t("colSize")}</div>
          <div role="columnheader">{t("colStatus")}</div>
        </div>
        {stats.map((s) => {
          const k = kindOf(s.id);
          const busy = busyLine(t, s);
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
                  <FolderIcon glyph={k.glyph} size={26} />
                  <span className="d-title">
                    <span className="entry-name">{t(k.name)}</span>
                    <span className="muted d-sub phone-only">{countLine(t, s)}</span>
                  </span>
                </button>
              </div>
              <div className="d-size" role="cell">{s.count}</div>
              <div className="d-size" role="cell">{fmtBytes(s.size)}</div>
              <div role="cell" className="muted">{busy ?? "–"}</div>
            </div>
          );
        })}
      </div>
    );
  }
  return (
    <ul className="folder-grid" aria-label={label}>
      {stats.map((s) => (
        <li key={s.id}>
          <FolderTile t={t} s={s} open={open} />
        </li>
      ))}
    </ul>
  );
}

function FolderTile({ t, s, open }: { t: T; s: KindStat; open: (id: KindId) => void }) {
  const k = kindOf(s.id);
  const id = useId();
  const busy = busyLine(t, s);
  return (
    <button className="folder-tile" onClick={() => open(s.id)} aria-describedby={id}>
      <FolderIcon glyph={k.glyph} />
      <span className="folder-text">
        <span className="folder-name">{t(k.name)}</span>
        <span id={id} className="folder-meta">
          <span className="muted">{s.count > 0 ? countLine(t, s) : t("emptyFolder")}</span>
          {busy && <span className="muted">{busy}</span>}
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

/** One pack as a card: what it is, its size and license, its state and its buttons. */
export function EntryTile({ t, e, glyph, where, as = "listitem" }: { t: T; e: Entry; glyph: Glyph; where?: string; as?: "listitem" | "group" }) {
  return (
    <div className="item entry-tile" role={as} aria-label={as === "group" ? e.name : undefined} data-entry={e.key}>
      <div className="entry-head">
        <span className="entry-icon">
          <GlyphIcon glyph={glyph} />
        </span>
        <div className="entry-main">
          <div className="entry-name">
            {e.name} {e.recommended && <span className="badge ok">{t("recommended")}</span>}
          </div>
          {e.desc && <div className="muted small-text">{e.desc}</div>}
          <div className="muted small-text">
            {fmtBytes(e.size)}
            {e.meta && ` · ${e.meta}`}
            {where && ` · ${where}`}
          </div>
          {e.note && <div className="offer-note small-text">{e.note}</div>}
          <Credit t={t} e={e} />
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
      {e.ask}
      {e.error && <div className="warn small-text">{e.error}</div>}
    </div>
  );
}

/**
 * Packs and maps: cards, or a table with name, size, status and license.
 * Each shows `glyph` (a topic's, or a kind's), or else its kind's; `where`
 * adds a line of its own to each (the kind, where things of every kind are
 * listed together).
 */
export function Entries({
  t,
  entries,
  view,
  label,
  glyph,
  where,
}: {
  t: T;
  entries: Entry[];
  view: ViewMode;
  label: string;
  glyph?: Glyph;
  where?: (e: Entry) => string;
}) {
  const icon = (e: Entry) => glyph ?? kindOf(e.kind).glyph;
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
                <GlyphIcon glyph={icon(e)} size={18} />
              </span>
              <span className="d-title">
                <span className="entry-name">
                  {e.name} {e.recommended && <span className="badge ok">{t("recommended")}</span>}
                </span>
                <span className="muted d-sub" title={e.desc || undefined}>{where ? where(e) : e.desc}</span>
              </span>
            </div>
            <div className="d-size" role="cell">{fmtBytes(e.size)}</div>
            <div className="d-status" role="cell">
              <span className={e.tone}>{e.status}</span>
              {e.progress !== null && <Bar pct={e.progress} label={e.name} />}
              {e.detail && <span className="muted d-detail">{e.detail}</span>}
              {e.note && <span className="offer-note d-detail">{e.note}</span>}
              {e.error && <span className="warn d-detail">{e.error}</span>}
            </div>
            <div className="d-license muted" role="cell">{e.license}</div>
            <div className="d-actions" role="cell">{e.actions(true)}</div>
            {e.ask && (
              <div className="d-ask" role="cell">
                {e.ask}
              </div>
            )}
          </div>
        ))}
      </div>
    );
  }
  return (
    <div className="list cols entry-grid" role="list" aria-label={label}>
      {entries.map((e) => (
        <EntryTile key={e.key} t={t} e={e} glyph={icon(e)} where={where?.(e)} />
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
