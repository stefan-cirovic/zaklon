import type { Status } from "../api";
import type { Key } from "../i18n";

type Props = { status: Status | null; error: string | null; t: (k: Key) => string; go: (tab: string) => void };

function fmtUptime(s?: number): string {
  if (s === undefined) return "–";
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60);
  return h > 0 ? `${h} h ${m} min` : `${m} min`;
}

export default function Home({ status, error, t, go }: Props) {
  const up = !!status && !error;
  return (
    <div className="stack">
      <div>
        <h1>{status?.hub_name ?? "Zaklon"}</h1>
        <p className="muted row">
          <span className={"status-dot" + (up ? "" : " off")} /> {up ? t("online") : t("offline")}
          {error && <span className="error"> · {error}</span>}
        </p>
      </div>
      <div className="grid">
        <div className="panel"><div className="label">{t("devices")}</div><div className="value">{status?.devices ?? "–"}</div></div>
        <div className="panel"><div className="label">{t("uptime")}</div><div className="value">{fmtUptime(status?.uptime_secs)}</div></div>
        <div className="panel"><div className="label">{t("version")}</div><div className="value">{status?.version ?? "–"}</div></div>
        <div className="panel"><div className="label">{t("addresses")}</div><div className="value" style={{ fontSize: 16 }}>{status?.addresses?.join(", ") || "–"}</div></div>
      </div>
      <div className="grid">
        <div className="panel"><div className="label">{t("expiringSoon")}</div><p className="muted">{t("nothingYet")}</p></div>
        <div className="panel"><div className="label">{t("runningLow")}</div><p className="muted">{t("nothingYet")}</p></div>
      </div>
      <div className="row">
        <button className="btn" onClick={() => go("supplies")}>{t("addItem")}</button>
        <button className="btn secondary" onClick={() => go("assistant")}>{t("ask")}</button>
      </div>
    </div>
  );
}
