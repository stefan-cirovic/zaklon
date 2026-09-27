import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { api, inTauri } from "../api";
import type { Key } from "../i18n";
import { errText } from "../errors";
import { fmtBytes, fmtDateTime } from "../format";
import { DrivePicker } from "./Usb";

type T = (k: Key) => string;
type BackupFile = { path: string; name: string; size: number; created: string; automatic: boolean; encrypted: boolean };
type Reply = { backups: BackupFile[]; restore_pending: boolean; folder: string; encryption: "on" | "off" | "no_password" };
/** The backup chosen for restoring: one from the list, or the file typed in (not known yet whether it is encrypted). */
type Picked = { path: string; encrypted: boolean | null; fromFile: boolean };

/** Backups of the household's data (laptop only): daily by itself, to USB on request, and restore. */
export default function Backups({ t }: { t: T }) {
  const [data, setData] = useState<Reply | null>(null);
  const [dir, setDir] = useState("");
  const [file, setFile] = useState("");
  const [picked, setPicked] = useState<Picked | null>(null);
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

  const save = (target: string) => {
    setPicked(null);
    run(async () => {
      const r = await api<{ path: string }>("/api/backups", { json: { dir: target } });
      setNote(`${t("backupSaved")} ${r.path}`);
    });
  };

  const restore = (password: string) => {
    if (!picked) return;
    const path = picked.path;
    run(async () => {
      await api("/api/backups/restore", { json: { path, password } });
      setPicked(null);
    });
  };

  const turnOnEncryption = (password: string) =>
    run(async () => {
      await api("/api/backups/encryption", { json: { password } });
      setNote(t("encryptionOn"));
    });

  const restart = () => {
    if (inTauri()) invoke("app_restart").catch(() => {});
  };

  const confirm = (encrypted: boolean | null) => (
    <RestoreConfirm t={t} encrypted={encrypted} busy={busy} err={err} onRestore={restore} onCancel={() => { setPicked(null); setErr(null); }} />
  );

  return (
    <div className="panel stack left">
      <h2>{t("backups")}</h2>
      <p className="muted" style={{ margin: 0, fontSize: 14 }}>{t("backupsIntro")}</p>
      {data?.encryption === "on" && <p className="muted" style={{ margin: 0, fontSize: 14 }}>{t("backupsEncrypted")}</p>}
      {data?.encryption === "off" && <TurnOnEncryption t={t} busy={busy} onTurnOn={turnOnEncryption} />}
      {err && !picked && <p className="error" role="alert">{err}</p>}
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
        {data?.backups.slice(0, 10).map((b) => {
          const open = picked !== null && !picked.fromFile && picked.path === b.path;
          return (
            <div className="row between wrap backup-row" key={b.path}>
              <span>
                {b.created ? fmtDateTime(b.created) : b.name}
                <span className="muted" style={{ fontSize: 13 }}>
                  {" "}· {b.automatic ? t("backupAuto") : t("backupManual")} · {b.encrypted ? t("backupEncrypted") : t("backupNotEncrypted")} · {fmtBytes(b.size)}
                </span>
              </span>
              {!open && (
                <button className="btn secondary small" disabled={busy} onClick={() => { setErr(null); setPicked({ path: b.path, encrypted: b.encrypted, fromFile: false }); }}>
                  {t("restore")}
                </button>
              )}
              {open && confirm(b.encrypted)}
            </div>
          );
        })}
      </div>
      <div>
        <button className="btn secondary" onClick={() => save("")} disabled={busy}>{t("backupNow")}</button>
      </div>

      <div className="label">{t("backupToUsb")}</div>
      <DrivePicker t={t} value={dir} onChange={setDir} label={t("backupToUsb")} />
      <div>
        <button className="btn secondary" onClick={() => save(dir)} disabled={busy || !dir.trim()}>{t("saveBackup")}</button>
      </div>

      <div className="stack restore-file" style={{ gap: 8 }}>
        <div className="label">{t("restoreFromFile")}</div>
        <input
          type="text"
          value={file}
          onChange={(e) => { setFile(e.target.value); if (picked?.fromFile) setPicked(null); }}
          placeholder="E:\zaklon-backup-2026-09-28-101500.zip"
          aria-label={t("restoreFromFile")}
        />
        {picked?.fromFile ? (
          confirm(null)
        ) : (
          <div>
            <button className="btn secondary" disabled={busy || !file.trim()} onClick={() => { setErr(null); setPicked({ path: file.trim(), encrypted: null, fromFile: true }); }}>
              {t("restore")}
            </button>
          </div>
        )}
      </div>
      <p className="muted" style={{ margin: 0, fontSize: 13 }}>{data?.encryption === "on" ? t("backupPrivateEncrypted") : t("backupPrivate")}</p>
    </div>
  );
}

type ConfirmProps = {
  t: T;
  /** Whether the backup is encrypted; null when not known (a file typed in). */
  encrypted: boolean | null;
  busy: boolean;
  err: string | null;
  onRestore: (password: string) => void;
  onCancel: () => void;
};

/** The last step before a restore: what it keeps, and the password an encrypted backup needs. */
function RestoreConfirm({ t, encrypted, busy, err, onRestore, onCancel }: ConfirmProps) {
  const [pw, setPw] = useState("");
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    onRestore(encrypted === false ? "" : pw);
  };
  return (
    <form className="stack restore-confirm" onSubmit={submit}>
      <p className="muted" style={{ margin: 0, fontSize: 14 }}>{t("restoreKeepsPhones")}</p>
      {encrypted !== false && (
        <label className="field">
          {t("restorePassword")}
          <input type="password" value={pw} onChange={(e) => setPw(e.target.value)} autoComplete="current-password" autoFocus />
        </label>
      )}
      {encrypted === null && <p className="muted" style={{ margin: 0, fontSize: 13 }}>{t("restorePasswordHint")}</p>}
      {err && <p className="error" role="alert" style={{ margin: 0 }}>{err}</p>}
      <div className="row wrap" style={{ gap: 8 }}>
        <button className="btn" disabled={busy || (encrypted === true && !pw)}>{t("yesRestore")}</button>
        <button type="button" className="btn secondary" onClick={onCancel} disabled={busy}>{t("cancel")}</button>
      </div>
    </form>
  );
}

/** For a hub set up before backups were encrypted: the household password, typed once, turns encryption on. */
function TurnOnEncryption({ t, busy, onTurnOn }: { t: T; busy: boolean; onTurnOn: (password: string) => void }) {
  const [pw, setPw] = useState("");
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (pw) onTurnOn(pw);
  };
  return (
    <form className="stack notice" style={{ gap: 10 }} onSubmit={submit}>
      <p style={{ margin: 0 }}><strong>{t("backupsNotEncrypted")}</strong></p>
      <p className="muted" style={{ margin: 0, fontSize: 14 }}>{t("backupsEncryptIntro")}</p>
      <label className="field">
        {t("password")}
        <input type="password" value={pw} onChange={(e) => setPw(e.target.value)} autoComplete="current-password" />
      </label>
      <div>
        <button className="btn" disabled={busy || !pw}>{t("turnOnEncryption")}</button>
      </div>
    </form>
  );
}
