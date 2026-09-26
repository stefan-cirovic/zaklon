import { useEffect, useState } from "react";
import { api, type Status } from "../api";
import ExpiryBadge from "../components/ExpiryBadge";
import { fmtQty } from "../format";
import type { Key } from "../i18n";
import type { Item } from "./Supplies";

type T = (k: Key) => string;
type Props = { status: Status | null; error: string | null; t: T; go: (tab: string) => void };
type Summary = { total_items: number; expired: Item[]; expiring_soon: Item[]; running_low: Item[] };

const SUMMARY_EVERY = 30_000;

function fmtUptime(s: number | undefined, t: T): string {
  if (s === undefined) return "–";
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return h > 0 ? `${h} ${t("hoursShort")} ${m} ${t("minutesShort")}` : `${m} ${t("minutesShort")}`;
}

function MiniList({ items, t, kind }: { items: Item[]; t: T; kind: "expiry" | "low" }) {
  if (items.length === 0) return <p className="muted">{t("nothingYet")}</p>;
  return (
    <div className="mini-list">
      {items.slice(0, 5).map((i) => (
        <div key={i.id} className="mini-row">
          <span>{i.name}</span>
          {kind === "expiry" ? (
            <ExpiryBadge date={i.expiry} t={t} />
          ) : (
            <span className="muted">
              {fmtQty(i.quantity)} / {fmtQty(i.min_quantity ?? 0)}
            </span>
          )}
        </div>
      ))}
      {items.length > 5 && <div className="muted" style={{ fontSize: 13 }}>+{items.length - 5}</div>}
    </div>
  );
}

export default function Home({ status, error, t, go }: Props) {
  const up = !!status && !error;
  const setUp = !!status?.set_up;
  const [sum, setSum] = useState<Summary | null>(null);
  const [sumFailed, setSumFailed] = useState(false);

  // Refresh the supplies summary on its own slow schedule; keep the last good
  // data when a refresh fails, and say that it is unavailable.
  useEffect(() => {
    if (!setUp) return;
    let alive = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const load = async () => {
      try {
        const s = await api<Summary>("/api/supplies/summary");
        if (alive) {
          setSum(s);
          setSumFailed(false);
        }
      } catch {
        if (alive) setSumFailed(true);
      }
      if (alive) timer = setTimeout(load, SUMMARY_EVERY);
    };
    load();
    return () => {
      alive = false;
      if (timer) clearTimeout(timer);
    };
  }, [setUp]);

  const unavailable = sum === null && sumFailed;
  return (
    <div className="stack">
      <div className="page-head">
        <h1>{status?.hub_name ?? "Zaklon"}</h1>
        <p className="muted row">
          <span className={"status-dot" + (up ? "" : " off")} aria-hidden="true" /> {up ? t("online") : t("offline")}
        </p>
        {error && <p className="error" role="alert">{error}</p>}
      </div>
      <div className="grid">
        <div className="panel">
          <div className="label">{t("devices")}</div>
          <div className="value">{status?.devices ?? "–"}</div>
        </div>
        <div className="panel">
          <div className="label">{t("uptime")}</div>
          <div className="value">{fmtUptime(status?.uptime_secs, t)}</div>
        </div>
        <div className="panel">
          <div className="label">{t("version")}</div>
          <div className="value">{status?.version ?? "–"}</div>
        </div>
        <div className="panel">
          <div className="label">{t("addresses")}</div>
          <div className="value" style={{ fontSize: 16 }}>{status?.addresses?.join(", ") || "–"}</div>
        </div>
      </div>
      <div className="grid">
        <div className="panel panel-list">
          <div className="label">{t("expiringSoon")}</div>
          {unavailable ? (
            <p className="warn">{t("unavailable")}</p>
          ) : (
            <MiniList items={[...(sum?.expired ?? []), ...(sum?.expiring_soon ?? [])]} t={t} kind="expiry" />
          )}
        </div>
        <div className="panel panel-list">
          <div className="label">{t("runningLow")}</div>
          {unavailable ? <p className="warn">{t("unavailable")}</p> : <MiniList items={sum?.running_low ?? []} t={t} kind="low" />}
        </div>
      </div>
      {sum !== null && sumFailed && <p className="muted" style={{ fontSize: 13 }}>{t("showingLastKnown")}</p>}
      <div className="row actions">
        <button className="btn" onClick={() => go("supplies")}>{t("addItem")}</button>
        <button className="btn secondary" onClick={() => go("assistant")}>{t("ask")}</button>
      </div>
    </div>
  );
}
