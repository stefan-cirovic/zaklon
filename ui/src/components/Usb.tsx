import { useCallback, useEffect, useMemo, useState } from "react";
import { api } from "../api";
import type { Key, Lang } from "../i18n";
import { errText } from "../errors";
import { fmtBytes } from "../format";

type T = (k: Key) => string;

export type Drive = { path: string; label: string; kind: "removable" | "fixed"; file_system: string; free: number; total: number; system: boolean };
type ExportState = {
  running: boolean;
  target: string | null;
  current: string | null;
  files_done: number;
  files_total: number;
  bytes_done: number;
  bytes_total: number;
  error: string | null;
  finished: boolean;
  /** Installer and phone app that can go on the stick: [file name, size]. */
  apps?: [string, number][];
};

/** A copyable thing: one pack, or all installed pieces of a country's map. */
export type CopyItem = { key: string; label: string; ids: string[]; size: number };

const FAT_LIMIT = 4294967295;

/** A path in the form drive roots are compared in, so "E:" typed by hand is the drive "E:\". */
function rootKey(p: string) {
  const k = p.toUpperCase().replace(/\//g, "\\");
  return k.endsWith("\\") ? k : `${k}\\`;
}

function driveLabel(t: T, d: Drive) {
  const name = d.label ? `${d.label} (${d.path.replace(/\\$/, "")})` : d.path;
  const tags = [d.kind === "removable" ? t("driveRemovable") : null, d.system ? t("driveSystem") : null, d.file_system].filter(Boolean);
  return `${name} · ${tags.join(" · ")} · ${fmtBytes(d.free)} ${t("free")}`;
}

/** Drives of the laptop, USB first, plus a free-text folder. */
export function DrivePicker({ t, value, onChange, label }: { t: T; value: string; onChange: (dir: string) => void; label: string }) {
  const [drives, setDrives] = useState<Drive[] | null>(null);
  const load = useCallback(() => {
    api<Drive[]>("/api/drives")
      .then((d) => setDrives([...d].sort((a, b) => Number(b.kind === "removable") - Number(a.kind === "removable") || Number(a.system) - Number(b.system))))
      .catch(() => setDrives([]));
  }, []);
  useEffect(load, [load]);
  const known = drives?.some((d) => d.path === value);
  return (
    <div className="stack" style={{ gap: 8 }}>
      <div className="row wrap">
        <select value={known ? value : ""} onChange={(e) => onChange(e.target.value)} aria-label={label} style={{ flex: "1 1 260px" }}>
          <option value="">{drives && drives.length === 0 ? t("noDrives") : t("chooseDrive")}</option>
          {drives?.map((d) => (
            <option key={d.path} value={d.path}>{driveLabel(t, d)}</option>
          ))}
        </select>
        <button type="button" className="btn secondary small" onClick={load}>{t("refreshDrives")}</button>
      </div>
      <input type="text" value={value} onChange={(e) => onChange(e.target.value)} placeholder={`${t("orTypeFolder")}: E:\\`} aria-label={`${label}: ${t("orTypeFolder")}`} />
    </div>
  );
}

/** Copy installed packs to a USB drive, in the background, with progress. `dir`: the drive chosen at first. */
export function CopyToUsb({ t, items, dir: initialDir = "" }: { t: T; lang: Lang; items: CopyItem[]; dir?: string }) {
  const [dir, setDir] = useState(initialDir);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [drives, setDrives] = useState<Drive[]>([]);
  const [job, setJob] = useState<ExportState | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const [withApps, setWithApps] = useState(true);

  // Drives change when a stick is plugged in; look again when a drive is chosen, not on every key.
  const driveRoot = /^[A-Za-z]:/.test(dir) ? dir.slice(0, 2).toUpperCase() : "";
  useEffect(() => {
    api<Drive[]>("/api/drives").then(setDrives).catch(() => {});
  }, [driveRoot]);

  const poll = useCallback(async () => {
    try {
      setJob(await api<ExportState>("/api/export"));
    } catch {
      /* try again */
    }
  }, []);
  const running = job?.running ?? false;
  useEffect(() => {
    poll();
    if (!running) return;
    const id = setInterval(poll, 1000);
    return () => clearInterval(id);
  }, [poll, running]);

  const chosen = items.filter((i) => picked.has(i.key));
  const apps = job?.apps ?? [];
  const appsOn = withApps && apps.length > 0;
  const size = chosen.reduce((s, i) => s + i.size, 0) + (appsOn ? apps.reduce((s, [, n]) => s + n, 0) : 0);
  const drive = useMemo(() => drives.filter((d) => rootKey(dir).startsWith(rootKey(d.path))).sort((a, b) => b.path.length - a.path.length)[0], [drives, dir]);
  const tooBig = drive && size > drive.free;
  const fat = drive && drive.file_system.toUpperCase().startsWith("FAT") && drive.file_system.toUpperCase() !== "EXFAT";
  const fatProblem = fat && chosen.some((i) => i.size > FAT_LIMIT && i.ids.length === 1);

  const toggle = (key: string) =>
    setPicked((p) => {
      const n = new Set(p);
      if (n.has(key)) n.delete(key);
      else n.add(key);
      return n;
    });

  const start = async () => {
    if (starting) return;
    setErr(null);
    setStarting(true);
    try {
      await api("/api/export", { json: { dir, ids: chosen.flatMap((i) => i.ids), with_apps: appsOn } });
      await poll();
    } catch (e) {
      setErr(errText(t, e));
    } finally {
      setStarting(false);
    }
  };

  const pct = job && job.bytes_total ? Math.min(100, Math.round((job.bytes_done / job.bytes_total) * 100)) : 0;

  return (
    <div className="panel stack left">
      <h2>{t("copyToUsb")}</h2>
      <p className="muted" style={{ margin: 0 }}>{t("copyToUsbIntro")}</p>
      {items.length === 0 && apps.length === 0 ? (
        <p className="muted">{t("nothingInstalled")}</p>
      ) : (
        <>
          <div className="stack" style={{ gap: 6 }}>
            {items.map((i) => (
              <label key={i.key} className="check-line">
                <input type="checkbox" checked={picked.has(i.key)} onChange={() => toggle(i.key)} disabled={running} />
                <span>
                  {i.label} <span className="muted" style={{ fontSize: 13 }}>· {fmtBytes(i.size)}</span>
                </span>
              </label>
            ))}
          </div>
          {apps.length > 0 && (
            <label className="check-line">
              <input type="checkbox" checked={withApps} onChange={(e) => setWithApps(e.target.checked)} disabled={running} />
              <span>{t("usbWithApps")}</span>
            </label>
          )}
          <DrivePicker t={t} value={dir} onChange={setDir} label={t("drive")} />
          {(chosen.length > 0 || appsOn) && (
            <p className="muted" style={{ margin: 0, fontSize: 14 }}>
              {t("selectedSize")}: {fmtBytes(size)}
              {drive && ` · ${fmtBytes(drive.free)} ${t("free")}`}
            </p>
          )}
          {tooBig && <p className="warn" style={{ margin: 0 }}>{t("notEnoughSpace")}</p>}
          {fatProblem && <p className="warn" style={{ margin: 0 }}>{t("fat32Warn")}</p>}
          {err && <p className="error" role="alert">{err}</p>}
          {running && job ? (
            <div className="stack" style={{ gap: 6 }}>
              <div className="bar"><i style={{ width: `${pct}%` }} /></div>
              <div className="muted" style={{ fontSize: 13 }}>
                {t("copying")} {job.files_done + 1}/{job.files_total} · {fmtBytes(job.bytes_done)} / {fmtBytes(job.bytes_total)} ({pct}%)
              </div>
              <div>
                <button className="btn secondary" onClick={() => api("/api/export/cancel", { method: "POST" }).then(poll)}>{t("cancel")}</button>
              </div>
            </div>
          ) : (
            <div>
              <button className="btn" onClick={start} disabled={starting || !dir.trim() || (chosen.length === 0 && !appsOn) || !!tooBig || !!fatProblem}>{t("copyBtn")}</button>
            </div>
          )}
          {job && !job.running && job.finished && job.target && (
            <p className="ok" role="status" style={{ margin: 0 }}>{t("copyDone")} {job.target}</p>
          )}
          {job && !job.running && job.error && (
            <p className="warn" style={{ margin: 0 }}>{t("copyFailed")}: {errText(t, new Error(job.error))}</p>
          )}
        </>
      )}
    </div>
  );
}
