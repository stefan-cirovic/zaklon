import { useEffect, useState } from "react";
import { api, type Status } from "../api";
import { ExpiryBadge, fmtQty, type Item } from "./Supplies";
import type { Key } from "../i18n";

type Props = { status: Status | null; error: string | null; t: (k: Key) => string; go: (tab: string) => void };

function fmtUptime(s?: number): string {
  if (s === undefined) return "–";
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60);
  return h > 0 ? `${h} h ${m} min` : `${m} min`;
}

type Summary = { total_items: number; expired: Item[]; expiring_soon: Item[]; running_low: Item[] };

function MiniList({ items, t, kind }: { items: Item[]; t: Props["t"]; kind: "expiry" | "low" }) {
  if (items.length === 0) return <p className="muted">{t("nothingYet")}</p>;
  return (
    <div className="mini-list">
      {items.slice(0, 5).map((i) => (
        <div key={i.id} className="mini-row">
          <span>{i.name}</span>
          {kind === "expiry" ? <ExpiryBadge date={i.expiry} t={t} /> : <span className="muted">{fmtQty(i.quantity)} / {fmtQty(i.min_quantity ?? 0)}</span>}
        </div>
      ))}
      {items.length > 5 && <div className="muted" style={{ fontSize: 13 }}>+{items.length - 5}</div>}
    </div>
  );
}

export default function Home({ status, error, t, go }: Props) {
  const up = !!status && !error;
  const [sum, setSum] = useState<Summary | null>(null);
  useEffect(() => {
    if (!status?.set_up) return;
    api<Summary>("/api/supplies/summary").then(setSum).catch(() => setSum(null));
  }, [status?.set_up, status?.uptime_secs]);
  return (
    <div className="stack">
      <div className="page-head">
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
        <div className="panel panel-list">
          <div className="label">{t("expiringSoon")}</div>
          <MiniList items={[...(sum?.expired ?? []), ...(sum?.expiring_soon ?? [])]} t={t} kind="expiry" />
        </div>
        <div className="panel panel-list">
          <div className="label">{t("runningLow")}</div>
          <MiniList items={sum?.running_low ?? []} t={t} kind="low" />
        </div>
      </div>
      <div className="row actions">
        <button className="btn" onClick={() => go("supplies")}>{t("addItem")}</button>
        <button className="btn secondary" onClick={() => go("assistant")}>{t("ask")}</button>
      </div>
    </div>
  );
}
