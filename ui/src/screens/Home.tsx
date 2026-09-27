import { UpdateBanner } from "../components/Updates";
import { useEffect, useState } from "react";
import { api, type Status } from "../api";
import { onBackOnline } from "../offline";
import { useVisiblePoll } from "../poll";
import ExpiryBadge from "../components/ExpiryBadge";
import { fmtDateTime, fmtQty } from "../format";
import type { Key } from "../i18n";
import type { Item } from "./Supplies";

type T = (k: Key) => string;
type Props = {
  status: Status | null;
  /** When the hub last answered (ms); the values shown are from then. */
  statusAt: number | null;
  error: string | null;
  t: T;
  go: (tab: string) => void;
  /** A phone: it has a copy of the supplies even while the hub is out of reach. */
  phone: boolean;
};
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

export default function Home({ status, statusAt, error, t, go, phone }: Props) {
  const up = !!status && !error;
  // A phone loads the summary even before (or without) an answer from the hub:
  // away from home api() serves the copy it kept.
  const canLoad = !!status?.set_up || phone;
  const [sum, setSum] = useState<Summary | null>(null);
  const [sys, setSys] = useState<{ battery_percent: number | null; plugged_in: boolean } | null>(null);
  const [sumFailed, setSumFailed] = useState(false);

  // Refresh the supplies summary on its own slow schedule (not while the app
  // is in the background); keep the last good data when a refresh fails, and
  // say that it is unavailable. Back in reach of the hub: refresh at once.
  const reload = useVisiblePoll(async () => {
    api<{ battery_percent: number | null; plugged_in: boolean }>("/api/system").then(setSys).catch(() => {});
    try {
      setSum(await api<Summary>("/api/supplies/summary"));
      setSumFailed(false);
      return true;
    } catch {
      setSumFailed(true);
      return false;
    }
  }, canLoad ? SUMMARY_EVERY : null);
  useEffect(() => onBackOnline(reload), [reload]);

  // No data at all: say so instead of "nothing yet", which would read as "nothing expires".
  const unavailable = sum === null && (sumFailed || (!canLoad && !!error));
  // The hub stopped answering: the numbers below are the last ones it gave.
  const stale = !!error && (status !== null || sys !== null);
  const power = sys;
  return (
    <div className="stack">
      <div className="page-head">
        <h1>{status?.hub_name ?? "Zaklon"}</h1>
        <p className="muted row">
          <span className={"status-dot" + (up ? "" : " off")} aria-hidden="true" /> {up ? t("online") : t("offline")}
        </p>
        {error && <p className="error" role="alert">{error}</p>}
      </div>
      <UpdateBanner t={t} />
      <div className="grid" style={stale ? { opacity: 0.55 } : undefined} aria-describedby={stale ? "status-as-of" : undefined}>
        <div className="panel">
          <div className="label">{t("devices")}</div>
          <div className="value">{status?.devices ?? "–"}</div>
        </div>
        <div className="panel">
          <div className="label">{t("uptime")}</div>
          <div className="value">{fmtUptime(status?.uptime_secs, t)}</div>
        </div>
        <div className="panel">
          <div className="label">{t("battery")}</div>
          <div className="value">
            {power === null ? "–" : power.battery_percent === null ? t("mainsPower") : `${power.battery_percent}%`}
          </div>
          {power && power.battery_percent !== null && (
            <div className={power.plugged_in ? "muted" : power.battery_percent < 30 ? "warn" : "muted"} style={{ fontSize: 13 }}>
              {power.plugged_in ? t("pluggedIn") : t("onBattery")}
            </div>
          )}
        </div>
        <div className="panel">
          <div className="label">{t("addresses")}</div>
          <div className="value" style={{ fontSize: 16 }}>{status?.addresses?.join(", ") || "–"}</div>
        </div>
      </div>
      {stale && (
        <p id="status-as-of" className="muted" style={{ fontSize: 13, marginTop: -8 }}>
          {statusAt !== null ? `${t("asOf")} ${fmtDateTime(new Date(statusAt).toISOString())}` : t("showingLastKnown")}
        </p>
      )}
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
      <div className="home-cta">
        <button className="btn" onClick={() => go("assistant")}>{t("ask")}</button>
      </div>
    </div>
  );
}
