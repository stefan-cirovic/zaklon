import { useCallback, useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, inTauri } from "../api";
import type { Key } from "../i18n";
import { fmtDateTime } from "../format";
import { detail } from "../errors";

type T = (k: Key) => string;
export type UpdateState = {
  enabled: boolean;
  current: string;
  latest: string | null;
  newer: boolean;
  url: string | null;
  checked_at: string | null;
  error: string | null;
};

function open(url: string) {
  if (inTauri()) openUrl(url).catch(() => window.open(url, "_blank", "noopener"));
  else window.open(url, "_blank", "noopener");
}

/** On Home: a quiet line when a newer Zaklon is out. */
export function UpdateBanner({ t }: { t: T }) {
  const [u, setU] = useState<UpdateState | null>(null);
  useEffect(() => {
    api<UpdateState>("/api/updates").then(setU).catch(() => {});
  }, []);
  if (!u?.newer || !u.latest) return null;
  return (
    <div className="panel notice row between wrap update-banner">
      <span>
        {t("updateAvailable")} <strong>{u.latest}</strong>
      </span>
      {u.url && (
        <button className="btn secondary small" onClick={() => open(u.url!)}>{t("updateOpen")}</button>
      )}
    </div>
  );
}

/** Settings › Updates: the switch (laptop), a "check now" button and the last result. */
export function UpdateSettings({ t, isHub }: { t: T; isHub: boolean }) {
  const [u, setU] = useState<UpdateState | null>(null);
  const [busy, setBusy] = useState(false);
  const load = useCallback(() => {
    api<UpdateState>("/api/updates").then(setU).catch(() => {});
  }, []);
  useEffect(load, [load]);

  const toggle = async (on: boolean) => {
    await api("/api/updates/settings", { json: { enabled: on } }).catch(() => {});
    load();
  };
  const check = async () => {
    setBusy(true);
    try {
      setU(await api<UpdateState>("/api/updates/check", { method: "POST" }));
    } catch {
      /* shown as "no internet" by the hub next time */
    } finally {
      setBusy(false);
    }
  };

  if (!u) return null;
  let status: string;
  if (u.error) status = u.error === "no internet" ? t("updateNoInternet") : `${t("updateFailed")}${detail(t, u.error)}. ${t("updateTryLater")}`;
  else if (!u.checked_at) status = t("updateNotChecked");
  else if (u.newer && u.latest) status = `${t("updateAvailable")} ${u.latest}`;
  else if (!u.latest) status = t("updateNoReleases");
  else status = t("updateUpToDate");

  return (
    <div className="stack" style={{ gap: 10 }}>
      {isHub && (
        <label className="check-line">
          <input type="checkbox" checked={u.enabled} onChange={(e) => toggle(e.target.checked)} />
          <span>{t("updateSwitch")}</span>
        </label>
      )}
      <p className="muted" style={{ margin: 0, fontSize: 14 }}>
        {status}
        {u.checked_at && ` · ${t("updateChecked")} ${fmtDateTime(u.checked_at)}`}
      </p>
      <div className="row wrap">
        <button className="btn secondary small" onClick={check} disabled={busy}>{t("updateCheckNow")}</button>
        {u.newer && u.url && <button className="btn small" onClick={() => open(u.url!)}>{t("updateOpen")}</button>}
      </div>
    </div>
  );
}
