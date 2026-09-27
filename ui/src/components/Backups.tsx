import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { api, inTauri } from "../api";
import type { Key } from "../i18n";
import { errText } from "../errors";
import { fmtBytes, fmtDateTime } from "../format";
import ConfirmButton from "./ConfirmButton";
import { DrivePicker } from "./Usb";

type T = (k: Key) => string;
type BackupFile = { path: string; name: string; size: number; created: string; automatic: boolean };
type Reply = { backups: BackupFile[]; restore_pending: boolean; folder: string };

/** Backups of the household's data (laptop only): daily by itself, to USB on request, and restore. */
export default function Backups({ t }: { t: T }) {
  const [data, setData] = useState<Reply | null>(null);
  const [dir, setDir] = useState("");
  const [file, setFile] = useState("");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const running = useRef(false);

  const load = useCallback(async () => {
    try {
      setData(await api<Reply>("/api/backups"));
    } catch (e) {
      setErr(errText(t, e));
    }
  }, [t]);
  useEffect(() => {
    load();
  }, [load]);

  const run = async (f: () => Promise<void>) => {
    if (running.current) return;
    running.current = true;
    setBusy(true);
    setErr(null);
    setNote(null);
    try {
      await f();
      await load();
    } catch (e) {
      setErr(errText(t, e));
    } finally {
      running.current = false;
      setBusy(false);
    }
  };

  const save = (target: string) =>
    run(async () => {
      const r = await api<{ path: string }>("/api/backups", { json: { dir: target } });
      setNote(`${t("backupSaved")} ${r.path}`);
    });

  const restore = (path: string) =>
    run(async () => {
      await api("/api/backups/restore", { json: { path } });
    });

  const restart = () => {
    if (inTauri()) invoke("app_restart").catch(() => {});
  };

  return (
    <div className="panel stack left">
      <h2>{t("backups")}</h2>
      <p className="muted" style={{ margin: 0, fontSize: 14 }}>{t("backupsIntro")}</p>
      {err && <p className="error" role="alert">{err}</p>}
      {note && <p className="ok" role="status" style={{ margin: 0, wordBreak: "break-all" }}>{note}</p>}

      {data?.restore_pending && (
        <div className="stack notice">
          <p style={{ margin: 0 }}><strong>{t("restoreReady")}</strong></p>
          {inTauri() ? (
            <div>
              <button className="btn" onClick={restart}>{t("restartNow")}</button>
            </div>
          ) : (
            <p className="muted" style={{ margin: 0 }}>{t("restartByHand")}</p>
          )}
        </div>
      )}

      <div className="label">{t("backupsOnHub")}</div>
      {data && data.backups.length === 0 && <p className="muted" style={{ margin: 0 }}>{t("noBackupsYet")}</p>}
      <div className="list">
        {data?.backups.slice(0, 10).map((b) => (
          <div className="row between wrap backup-row" key={b.path}>
            <span>
              {b.created ? fmtDateTime(b.created) : b.name}
              <span className="muted" style={{ fontSize: 13 }}> · {b.automatic ? t("backupAuto") : t("backupManual")} · {fmtBytes(b.size)}</span>
            </span>
            <ConfirmButton
              label={t("restore")}
              confirmLabel={t("yesRestore")}
              cancelLabel={t("cancel")}
              className="btn secondary small"
              onConfirm={() => restore(b.path)}
            />
          </div>
        ))}
      </div>
      <div>
        <button className="btn secondary" onClick={() => save("")} disabled={busy}>{t("backupNow")}</button>
      </div>

      <div className="label">{t("backupToUsb")}</div>
      <DrivePicker t={t} value={dir} onChange={setDir} label={t("backupToUsb")} />
      <div>
        <button className="btn secondary" onClick={() => save(dir)} disabled={busy || !dir.trim()}>{t("saveBackup")}</button>
      </div>

      <div className="label">{t("restoreFromFile")}</div>
      <input type="text" value={file} onChange={(e) => setFile(e.target.value)} placeholder="E:\zaklon-backup-2026-09-28-101500.zip" aria-label={t("restoreFromFile")} />
      <div>
        <ConfirmButton
          label={t("restore")}
          confirmLabel={t("yesRestore")}
          cancelLabel={t("cancel")}
          className="btn secondary"
          onConfirm={() => restore(file)}
        />
      </div>
      <p className="muted" style={{ margin: 0, fontSize: 13 }}>{t("backupPrivate")}</p>
    </div>
  );
}
