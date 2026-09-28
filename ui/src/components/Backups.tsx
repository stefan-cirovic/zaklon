import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { api, inTauri } from "../api";
import type { Key } from "../i18n";
import { errCode, errText } from "../errors";
import { fmtBytes, fmtDateTime } from "../format";
import { DrivePicker } from "./Usb";

type T = (k: Key) => string;
type BackupFile = { path: string; name: string; size: number; created: string; automatic: boolean; encrypted: boolean };
type Reply = { backups: BackupFile[]; restore_pending: boolean; folder: string; encryption: "on" | "off" | "no_password" };
/** The backup chosen for restoring: one from the list, or the file typed in (not known yet whether it is encrypted). */
type Picked = { path: string; encrypted: boolean | null; fromFile: boolean };

/**
 * Ask the hub to prepare a restore. An unencrypted backup cannot be checked,
 * so where that matters the hub first answers "backup_not_encrypted"; this
 * returns false then, and the same call with `allowUnencrypted` restores it.
 */
async function requestRestore(path: string, password: string, allowUnencrypted: boolean): Promise<boolean> {
  try {
    await api("/api/backups/restore", { json: { path, password, allow_unencrypted: allowUnencrypted } });
    return true;
  } catch (e) {
    if (errCode(e) === "backup_not_encrypted") return false;
    throw e;
  }
}

/**
 * Backups of the household's data (laptop only): daily by itself, to USB on request, and restore.
 * The page it is on (Settings › Backups) gives the title; each part has an id the settings search jumps to.
 */
export default function Backups({ t }: { t: T }) {
  const [data, setData] = useState<Reply | null>(null);
  const [dir, setDir] = useState("");
  const [file, setFile] = useState("");
  const [picked, setPicked] = useState<Picked | null>(null);
  /** The hub said the picked backup is not encrypted. */
  const [unencrypted, setUnencrypted] = useState(false);
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

  // An unencrypted backup onto a hub whose backups are encrypted: said before
  // it is restored, and restored only when the person goes ahead anyway.
  const warnUnencrypted = unencrypted || (picked?.encrypted === false && data?.encryption === "on");

  const pick = (p: Picked | null) => {
    setErr(null);
    setUnencrypted(false);
    setPicked(p);
  };

  const restore = (password: string) => {
    if (!picked) return;
    const path = picked.path;
    run(async () => {
      if (await requestRestore(path, password, warnUnencrypted)) pick(null);
      else setUnencrypted(true);
    });
  };

  const turnOnEncryption = (password: string) =>
    run(async () => {
      await api("/api/backups/encryption", { json: { password } });
      setNote(t("encryptionOn"));
    });

  const confirm = (encrypted: boolean | null) => (
    <RestoreConfirm
      t={t}
      note={t("restoreKeepsPhones")}
      encrypted={encrypted}
      warn={warnUnencrypted}
      busy={busy}
      err={err}
      onRestore={restore}
      onCancel={() => pick(null)}
    />
  );

  // The buttons come with the list: appearing above them, it would move them
  // under a finger or pointer on its way to "Make a backup now".
  if (!data) {
    return (
      <div className="panel stack left">
        <p className="muted" style={{ margin: 0, fontSize: 14 }}>{t("backupsIntro")}</p>
        {err ? <p className="error" role="alert">{err}</p> : <p className="muted" style={{ margin: 0 }}>{t("loadingOrUnavailable")}</p>}
      </div>
    );
  }

  return (
    <div className="panel stack left">
      <p className="muted" style={{ margin: 0, fontSize: 14 }}>{t("backupsIntro")}</p>
      {(data.encryption === "on" || data.encryption === "off") && (
        <div id="set-backup-encryption" className="set-anchor stack" tabIndex={-1}>
          {data.encryption === "on" ? (
            <p className="muted" style={{ margin: 0, fontSize: 14 }}>{t("backupsEncrypted")}</p>
          ) : (
            <TurnOnEncryption t={t} busy={busy} onTurnOn={turnOnEncryption} />
          )}
        </div>
      )}
      {err && !picked && <p className="error" role="alert">{err}</p>}
      {note && <p className="ok" role="status" style={{ margin: 0, wordBreak: "break-all" }}>{note}</p>}

      {data?.restore_pending && <RestoreReady t={t} />}

      <div id="set-backup-now" className="set-anchor stack" tabIndex={-1}>
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
                  <button className="btn secondary small" disabled={busy} onClick={() => pick({ path: b.path, encrypted: b.encrypted, fromFile: false })}>
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
      </div>

      <div id="set-backup-usb" className="set-anchor stack" tabIndex={-1}>
        <div className="label">{t("backupToUsb")}</div>
        <DrivePicker t={t} value={dir} onChange={setDir} label={t("backupToUsb")} />
        <div>
          <button className="btn secondary" onClick={() => save(dir)} disabled={busy || !dir.trim()}>{t("saveBackup")}</button>
        </div>
      </div>

      <div id="set-restore" className="set-anchor stack restore-file" style={{ gap: 8 }} tabIndex={-1}>
        <div className="label">{t("restoreFromFile")}</div>
        <input
          type="text"
          value={file}
          onChange={(e) => { setFile(e.target.value); if (picked?.fromFile) pick(null); }}
          placeholder={BACKUP_FILE_EXAMPLE}
          aria-label={t("restoreFromFile")}
        />
        {picked?.fromFile ? (
          confirm(null)
        ) : (
          <div>
            <button className="btn secondary" disabled={busy || !file.trim()} onClick={() => pick({ path: file.trim(), encrypted: null, fromFile: true })}>
              {t("restore")}
            </button>
          </div>
        )}
      </div>
      <p className="muted" style={{ margin: 0, fontSize: 13 }}>{data?.encryption === "on" ? t("backupPrivateEncrypted") : t("backupPrivate")}</p>
    </div>
  );
}

const BACKUP_FILE_EXAMPLE = "E:\\zaklon-backup-2026-09-28-101500.zip";

/** A restore is prepared: it happens when Zaklon starts again. */
function RestoreReady({ t }: { t: T }) {
  const restart = () => {
    if (inTauri()) invoke("app_restart").catch(() => {});
  };
  return (
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
  );
}

type ConfirmProps = {
  t: T;
  /** What the restore keeps and what it takes from the backup. */
  note?: string;
  /** Whether the backup is encrypted; null when not known (a file typed in). */
  encrypted: boolean | null;
  /** The backup is not encrypted where that needs a warning (see `requestRestore`). */
  warn: boolean;
  busy: boolean;
  err: string | null;
  onRestore: (password: string) => void;
  onCancel: () => void;
};

/** The last step before a restore: what it keeps, and the password an encrypted backup needs. */
function RestoreConfirm({ t, note, encrypted, warn, busy, err, onRestore, onCancel }: ConfirmProps) {
  const [pw, setPw] = useState("");
  const plain = encrypted === false || warn;
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    onRestore(plain ? "" : pw);
  };
  return (
    <form className="stack restore-confirm" onSubmit={submit}>
      {note && <p className="muted" style={{ margin: 0, fontSize: 14 }}>{note}</p>}
      {warn &&<p className="warn" role="alert" style={{ margin: 0 }}>{t("restoreUnencrypted")}</p>}
      {!plain && (
        <label className="field">
          {t("restorePassword")}
          <input type="password" value={pw} onChange={(e) => setPw(e.target.value)} autoComplete="current-password" autoFocus />
        </label>
      )}
      {encrypted === null && !warn && <p className="muted" style={{ margin: 0, fontSize: 13 }}>{t("restorePasswordHint")}</p>}
      {err && <p className="error" role="alert" style={{ margin: 0 }}>{err}</p>}
      <div className="row wrap" style={{ gap: 8 }}>
        <button className="btn" disabled={busy || (encrypted === true && !pw)}>{warn ? t("restoreAnyway") : t("yesRestore")}</button>
        <button type="button" className="btn secondary" onClick={onCancel} disabled={busy}>{t("cancel")}</button>
      </div>
    </form>
  );
}

/**
 * On a hub that is not set up yet: restore the backup of the household's
 * previous hub instead. On a new install the restore takes the paired phones,
 * the household password and the hub's identity from the backup too (a hub
 * that was set up always keeps its own; see `Access` in the hub's backup.rs).
 * `onReady` says when a restore is prepared, so setup can step aside.
 */
export function SetupRestore({ t, onReady }: { t: T; onReady: () => void }) {
  const [file, setFile] = useState("");
  const [picked, setPicked] = useState(false);
  const [unencrypted, setUnencrypted] = useState(false);
  const [ready, setReady] = useState(false);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const done = useCallback(() => {
    setReady(true);
    onReady();
  }, [onReady]);

  // A restore prepared before the window was reopened is still waiting.
  useEffect(() => {
    api<Reply>("/api/backups")
      .then((r) => {
        if (r.restore_pending) done();
      })
      .catch(() => {});
  }, [done]);

  const pick = (on: boolean) => {
    setErr(null);
    setUnencrypted(false);
    setPicked(on);
  };

  const restore = async (password: string) => {
    if (busy) return;
    setBusy(true);
    setErr(null);
    try {
      if (await requestRestore(file.trim(), password, unencrypted)) done();
      else setUnencrypted(true);
    } catch (e) {
      setErr(errText(t, e));
    } finally {
      setBusy(false);
    }
  };

  if (ready) return <RestoreReady t={t} />;
  return (
    <div className="panel stack left restore-file">
      <h2>{t("setupRestoreTitle")}</h2>
      <p className="muted" style={{ margin: 0, fontSize: 14 }}>{t("setupRestoreIntro")}</p>
      <input
        type="text"
        value={file}
        onChange={(e) => { setFile(e.target.value); if (picked) pick(false); }}
        placeholder={BACKUP_FILE_EXAMPLE}
        aria-label={t("restoreFromFile")}
      />
      {picked ? (
        <RestoreConfirm t={t} encrypted={null} warn={unencrypted} busy={busy} err={err} onRestore={restore} onCancel={() => pick(false)} />
      ) : (
        <div>
          <button className="btn secondary" disabled={!file.trim()} onClick={() => pick(true)}>
            {t("restore")}
          </button>
        </div>
      )}
    </div>
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
