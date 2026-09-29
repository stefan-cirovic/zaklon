import { useEffect, useId, useState, type ReactNode } from "react";
import { api } from "../api";
import type { Key } from "../i18n";
import { canScan } from "../scan";
import { fold } from "../settings";
import { errCode, errText } from "../errors";
import { countWord, daysUntil, fmtDate, fmtQty, unitName } from "../format";
import { byExpiry, lowShare, type Item } from "../screens/Supplies";
import CardHead from "./CardHead";
import { Icon } from "./Icon";

type T = (k: Key) => string;
export type SuppliesSummary = { total_items: number; expired: Item[]; expiring_soon: Item[]; running_low: Item[]; to_put_away?: number };
export type ShoppingEntry = { id: string; item_id?: string | null; text: string; quantity: number | null; unit: string | null; status: string };

/** How many lines a column shows; the rest is a link away. */
const LINES = 5;

/** "2 days" / "2 dana". */
function dayCount(n: number): string {
  return `${n} ${countWord(n, ["day", "days"], ["dan", "dana", "dana"])}`;
}

/** When, in a word or two (the column's heading says the rest): "today", "in 3 days", "expired 2 days ago". */
function whenText(t: T, days: number, expiry: string): string {
  if (days < 0) {
    if (days === -1) return t("attnExpiredYesterday");
    if (days >= -30) return t("attnExpiredAgo").replace("{n}", dayCount(-days));
    return t("attnExpiredOn").replace("{date}", fmtDate(expiry));
  }
  if (days === 0) return t("today");
  if (days === 1) return t("homeTomorrow");
  return t("homeInDays").replace("{n}", dayCount(days));
}

/** On the shopping list already: added (by hand or from the assistant), or suggested there because it runs low. */
function onShoppingList(shop: ShoppingEntry[], item: Item): boolean {
  const name = fold(item.name);
  return shop.some((e) => e.status === "open" && (e.item_id === item.id || e.id === `low:${item.id}` || fold(e.text) === name));
}

/**
 * The supplies at a glance, in three columns side by side (one under the
 * other on a phone): what expires (what has expired first), what runs low
 * (the emptiest first) and the shopping list, where a tick marks something
 * bought. Each shows a few lines and leads to the rest; an empty one says
 * "Nothing", so it reads as all is well. Below them adding an item (and
 * scanning one on a phone) and how many items there are.
 */
export default function SuppliesCard({
  t,
  sum,
  shop,
  unavailable,
  note,
  reload,
}: {
  t: T;
  sum: SuppliesSummary | null;
  shop: ShoppingEntry[] | null;
  unavailable: boolean;
  /** When the lists are old: from when, or that the hub does not answer. */
  note: string | null;
  /** Ask for the supplies and the shopping list again. */
  reload: () => void;
}) {
  const id = useId();
  const [scanner, setScanner] = useState(false);
  useEffect(() => {
    canScan()
      .then(setScanner)
      .catch(() => setScanner(false));
  }, []);
  const [err, setErr] = useState<string | null>(null);
  // Ticked as bought: shown ticked until the list from the hub (or the phone's copy) leaves it out.
  const [ticked, setTicked] = useState<string[]>([]);
  useEffect(() => {
    if (!shop) return;
    setTicked((list) => {
      const kept = list.filter((x) => shop.some((e) => e.id === x));
      return kept.length === list.length ? list : kept;
    });
  }, [shop]);

  const tick = async (e: ShoppingEntry) => {
    if (ticked.includes(e.id)) return;
    setTicked((list) => [...list, e.id]);
    setErr(null);
    try {
      // As "Bought" on the shopping list: it moves to Put away (on a phone away from home it waits for the hub).
      await api(`/api/shopping/${encodeURIComponent(e.id)}/bought`, { method: "POST" });
    } catch (ex) {
      setTicked((list) => list.filter((x) => x !== e.id));
      setErr(errText(t, ex));
      // Bought or deleted on another device meanwhile: show the list as it is now.
      if (errCode(ex) !== "not_found") return;
    }
    reload();
  };

  const expiring = sum ? [...sum.expired, ...sum.expiring_soon].sort(byExpiry) : [];
  const low = sum ? [...sum.running_low].sort((a, b) => lowShare(a) - lowShare(b)) : [];
  const toBuy = (shop ?? []).filter((e) => e.status === "open");
  const toPutAway = sum?.to_put_away ?? 0;
  const total = sum?.total_items ?? 0;

  return (
    <section className="panel left home-card home-supplies" aria-labelledby={id}>
      <CardHead id={id} icon="supplies" title={t("supplies")} href="#supplies" link={t("homeOpenSupplies")} />
      {unavailable ? (
        <p className="warn home-empty">{t("unavailable")}</p>
      ) : sum === null ? (
        <p className="muted home-empty">{t("aiLoading")}</p>
      ) : (
        <div className="sup-cols">
          <Column t={t} title={t("expiring")} count={expiring.length} more="#supplies/expiring">
            {expiring.slice(0, LINES).map((item) => {
              const days = item.expiry ? daysUntil(item.expiry) : 0;
              return (
                <li key={item.id} className={"sup-row " + (days < 0 ? "expired" : "soon")}>
                  <span className="sup-text">
                    <span className="sup-name" title={item.name}>{item.name}</span>
                    <span className="sup-state">{whenText(t, days, item.expiry ?? "")}</span>
                  </span>
                </li>
              );
            })}
          </Column>
          <Column t={t} title={t("runningLow")} count={low.length} more="#supplies/low">
            {low.slice(0, LINES).map((item) => {
              const min = item.min_quantity ?? 0;
              const listed = shop !== null && onShoppingList(shop, item);
              return (
                <li key={item.id} className={"sup-row low" + (item.quantity <= 0 ? " out" : "")}>
                  <span className="sup-text">
                    <span className="sup-name" title={item.name}>{item.name}</span>
                    <span className="sup-state">
                      {fmtQty(item.quantity)} {t("of")} {fmtQty(min)} {unitName(t, item.unit, min)}
                    </span>
                    {listed && (
                      <span className="sup-listed" title={t("onShoppingList")}>
                        <Icon name="cart" size={14} />
                        <span className="sr-only">{t("onShoppingList")}</span>
                      </span>
                    )}
                  </span>
                </li>
              );
            })}
          </Column>
          <Column
            t={t}
            title={t("homeToBuy")}
            count={toBuy.length}
            failed={shop === null}
            more="#supplies/shopping"
            after={
              toPutAway > 0 && (
                <a className="home-link" href="#supplies/putaway">
                  {t("homeToPutAway")}: {toPutAway}
                </a>
              )
            }
          >
            {toBuy.slice(0, LINES).map((e) => {
              const done = ticked.includes(e.id);
              return (
                <li key={e.id} className="sup-row buy">
                  <button
                    type="button"
                    role="checkbox"
                    aria-checked={done}
                    aria-label={`${t("bought")}: ${e.text}`}
                    className="sup-tick"
                    onClick={() => tick(e)}
                    disabled={done}
                    title={t("bought")}
                  >
                    <span className="sup-box" aria-hidden="true">
                      {done && <Icon name="check" size={12} />}
                    </span>
                    <span className="sup-text">
                      <span className="sup-name">{e.text}</span>
                      {e.quantity !== null && e.quantity > 0 && (
                        <span className="sup-state">
                          {fmtQty(e.quantity)}
                          {e.unit ? ` ${unitName(t, e.unit, e.quantity)}` : ""}
                        </span>
                      )}
                    </span>
                  </button>
                </li>
              );
            })}
          </Column>
        </div>
      )}
      {err && <p className="error home-empty" role="alert">{err}</p>}
      {(sum !== null || note) && (
        <div className="home-foot sup-foot">
          {sum !== null && (
            <>
              <a className="btn secondary small" href="#supplies/add">
                <Icon name="plus" size={16} />
                {t("addItem")}
              </a>
              {scanner && (
                <a className="btn secondary small" href="#supplies/scan" title={t("scanBarcode")}>
                  {t("scan")}
                </a>
              )}
            </>
          )}
          <span className="sup-foot-end">
            {sum !== null && (
              <span className="muted">
                {total > 0 ? `${total} ${countWord(total, ["item", "items"], ["stavka", "stavke", "stavki"])} ${t("homeAtHome")}` : t("homeNoSupplies")}
              </span>
            )}
            {note && <span className="muted home-note">{note}</span>}
          </span>
        </div>
      )}
    </section>
  );
}

/** One column: its heading, its first lines (or a calm "Nothing"), and a link to the rest. */
function Column({
  t,
  title,
  count,
  more,
  failed = false,
  after,
  children,
}: {
  t: T;
  title: string;
  count: number;
  /** The list could not be read: say so rather than "Nothing". */
  failed?: boolean;
  /** Where the rest is. */
  more: string;
  /** Anything below the lines. */
  after?: ReactNode;
  children: ReactNode;
}) {
  const id = useId();
  return (
    <section className="sup-col" aria-labelledby={id}>
      <h3 id={id} className="home-sub">{title}</h3>
      {failed ? (
        <p className="warn home-empty">{t("unavailable")}</p>
      ) : count === 0 ? (
        <p className="sup-nothing">
          <Icon name="check" size={14} />
          {t("homeNothing")}
        </p>
      ) : (
        <ul className="sup-list">{children}</ul>
      )}
      {count > LINES && (
        <a className="home-link" href={more}>
          {t("homePlusMore").replace("{n}", String(count - LINES))}
        </a>
      )}
      {after}
    </section>
  );
}
