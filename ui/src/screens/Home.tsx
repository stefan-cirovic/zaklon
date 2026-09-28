import { useEffect, useId, useState } from "react";
import { api, ApiError, type Status } from "../api";
import type { Key, Lang } from "../i18n";
import { offlineState, onBackOnline, onOfflineChange } from "../offline";
import { useVisiblePoll } from "../poll";
import { countWord, daysUntil, fmtBytes, fmtDate, fmtDateTime, fmtQty, unitName } from "../format";
import { askInAssistant, fromText, openInAssistant, type Summary as Chat } from "../conversations";
import { BUSY, rootName, type CatalogReply, type Localized } from "../addons";
import { countryState, type MapsReply } from "../maps";
import { TOOLS, type ToolId } from "../tools";
import { askForPairing, fold } from "../settings";
import { errText } from "../errors";
import type { Item } from "./Supplies";
import HelpLink from "../components/HelpLink";
import Firewall from "../components/Firewall";
import { UpdateBanner } from "../components/Updates";
import { DriveTile } from "../components/AddonViews";
import { Icon, type IconName } from "../components/Icon";
import ZaklonMap from "../components/ZaklonMap";
import { homeText, useMapInfo } from "../components/MapPart";
import type { MapInfo } from "../map/info";

type T = (k: Key) => string;
type Props = {
  status: Status | null;
  /** When the hub last answered (ms); the values shown are from then. */
  statusAt: number | null;
  error: string | null;
  t: T;
  lang: Lang;
  go: (tab: string) => void;
  /** A phone: it has a copy of the supplies even while the hub is out of reach. */
  phone: boolean;
  /** The tool in the bar; Quick access leaves it out. */
  pinned: ToolId | null;
};
type SuppliesSummary = { total_items: number; expired: Item[]; expiring_soon: Item[]; running_low: Item[]; to_put_away?: number };
type ShoppingEntry = { id: string; item_id?: string | null; text: string; quantity: number | null; unit: string | null; status: string };

/** Supplies, the shopping list, the conversations and the add-ons are asked for again this often (never while the app is in the background). */
const EVERY = 30_000;
/** While something downloads, its bar moves this often. */
const BUSY_EVERY = 2_000;
/** The world's maps are a long answer: asked for rarely, unless a map is downloading. */
const MAPS_EVERY = 120_000;
/** How many things that need attention Home lists, how many downloads, and how many conversations. */
const ATTENTION = 6;
const SHOW = 5;
/** Something added to the shopping list from Home counts as on it this long, until the list itself shows it. */
const ADDED_KEEP = 60_000;
const RECENT = 3;

/**
 * Home: the household at a glance, using the whole window. The hub in one
 * line at the top, then the assistant, what needs attention in the supplies
 * with the home on the map beside it, the library and add-ons, and the
 * tools. A phone away from home shows its copy of the supplies and
 * conversations, marked as old.
 */
export default function Home({ status, statusAt, error, t, lang, go, phone, pinned }: Props) {
  const up = !!status && !error;
  // A phone loads even before (or without) an answer from the hub: away from home api() serves the copy it kept.
  const canLoad = !!status?.set_up || phone;
  // A phone away from home shows its own copy, and the banner above says so.
  const [away, setAway] = useState(() => phone && offlineState().since !== null);
  const [since, setSince] = useState(() => offlineState().since);
  useEffect(
    () =>
      onOfflineChange(() => {
        setAway(phone && offlineState().since !== null);
        setSince(offlineState().since);
      }),
    [phone],
  );
  // What only the hub knows (no copy on the phone) is not asked for away from home.
  const live = canLoad && !away;

  const [sum, setSum] = useState<SuppliesSummary | null>(null);
  const [shop, setShop] = useState<ShoppingEntry[] | null>(null);
  const [sumFailed, setSumFailed] = useState(false);
  const [chats, setChats] = useState<Chat[] | null>(null);
  const [chatsFailed, setChatsFailed] = useState(false);
  // An older hub keeps no conversations: then there is no list to show.
  const [chatsSaved, setChatsSaved] = useState(true);

  // The supplies, the shopping list and this device's conversations, on a
  // slow schedule. The last good data stays when a refresh fails, marked as
  // old. Back in reach of the hub: refresh at once.
  const reload = useVisiblePoll(async () => {
    const [s, sh, c] = await Promise.allSettled([
      api<SuppliesSummary>("/api/supplies/summary"),
      api<ShoppingEntry[]>("/api/shopping"),
      api<Chat[]>("/api/conversations"),
    ]);
    if (s.status === "fulfilled") setSum(s.value);
    setSumFailed(s.status === "rejected");
    if (sh.status === "fulfilled" && Array.isArray(sh.value)) setShop(sh.value);
    if (c.status === "fulfilled" && Array.isArray(c.value)) {
      setChats(c.value);
      setChatsFailed(false);
    } else {
      const e = c.status === "rejected" ? c.reason : new SyntaxError("not a list");
      // A hub from before saved conversations answers with its page, not a list.
      if (e instanceof SyntaxError || (e instanceof ApiError && (e.status === 404 || e.status === 405))) setChatsSaved(false);
      else setChatsFailed(true);
    }
    return s.status === "fulfilled";
  }, canLoad ? EVERY : null);
  useEffect(() => onBackOnline(reload), [reload]);

  // Add-ons (with the hub's battery and disk), and the maps, which have their own list.
  const [cat, setCat] = useState<CatalogReply | null>(null);
  const [catFailed, setCatFailed] = useState(false);
  const [maps, setMaps] = useState<MapsReply | null>(null);
  const packsBusy = cat?.packs.some((p) => BUSY.includes(p.state.status)) ?? false;
  const mapsBusy = maps?.countries.some((c) => countryState(c).busy) ?? false;
  useVisiblePoll(async () => {
    try {
      setCat(await api<CatalogReply>("/api/catalog"));
      setCatFailed(false);
      return true;
    } catch {
      setCatFailed(true);
      return false;
    }
  }, live ? (packsBusy ? BUSY_EVERY : EVERY) : null);
  useVisiblePoll(async () => {
    try {
      setMaps(await api<MapsReply>("/api/maps"));
      return true;
    } catch {
      return false;
    }
  }, live ? (mapsBusy ? BUSY_EVERY : MAPS_EVERY) : null);
  // The Zaklon map and the household's home on it.
  // Away from home a phone draws the overview it carries (see useMapInfo).
  const { info: mapInfo, err: mapErr } = useMapInfo(t, canLoad);

  // No data at all: say so instead of "nothing expires", which would read as all is well.
  const unavailable = sum === null && (sumFailed || (!canLoad && !!error));
  // The hub stopped answering: the facts at the top are the last ones it gave.
  const stale = !!error && status !== null;
  const suppliesNote = away && since !== null ? `${t("asOf")} ${fmtDateTime(new Date(since).toISOString())}` : sum !== null && sumFailed ? t("showingLastKnown") : null;

  return (
    <div className="home">
      <HubHead
        t={t}
        status={status}
        statusAt={statusAt}
        up={up}
        stale={stale}
        system={cat?.system ?? null}
        addPhone={
          // Laptop only: straight to pairing, as Settings › Devices › Add a phone.
          !phone && status?.set_up
            ? () => {
                askForPairing();
                go("settings/devices/add-phone");
              }
            : null
        }
      />
      <div className="home-alerts">
        {error && !away && (
          <div className="panel notice home-alert" role="alert">
            <Icon name="offline" size={20} />
            <span>{error}</span>
          </div>
        )}
        {!phone && status?.set_up && <Firewall t={t} />}
        <UpdateBanner t={t} />
      </div>
      <div className="home-grid">
        <AssistantCard t={t} go={go} chats={chats} failed={chatsFailed} saved={chatsSaved} />
        <SuppliesCard t={t} sum={sum} shop={shop} unavailable={unavailable} note={suppliesNote} reload={reload} />
        <HomeMapCard t={t} lang={lang} info={mapInfo} unreachable={!!mapErr && !mapInfo} />
        <AddonsCard t={t} lang={lang} cat={cat} maps={maps} failed={catFailed} away={away} go={go} />
        <QuickAccess t={t} pinned={pinned} />
      </div>
    </div>
  );
}

/** The hub in one line: its name, whether it runs, its power, the paired devices (with "Add a phone" on the laptop) and its address. */
function HubHead({
  t,
  status,
  statusAt,
  up,
  stale,
  system,
  addPhone,
}: {
  t: T;
  status: Status | null;
  statusAt: number | null;
  up: boolean;
  stale: boolean;
  system: CatalogReply["system"] | null;
  addPhone: (() => void) | null;
}) {
  const asOfId = useId();
  const power =
    system === null ? "–" : system.battery_percent === null ? t("homeOnPower") : `${system.battery_percent}% · ${system.plugged_in ? t("pluggedIn") : t("onBattery")}`;
  const lowBattery = !!system && system.battery_percent !== null && !system.plugged_in && system.battery_percent < 30;
  return (
    <header className="home-head">
      <h1>{status?.hub_name ?? "Zaklon"}</h1>
      <p className="home-state">
        <span className={"status-dot" + (up ? "" : " off")} aria-hidden="true" />
        {up ? t("online") : t("offline")}
      </p>
      {/* Dimmed when old, but still readable (4.6:1). */}
      <dl className={"home-facts" + (stale ? " stale" : "")} aria-describedby={stale ? asOfId : undefined}>
        <div>
          <dt>{t("homePower")}</dt>
          <dd className={lowBattery ? "warn" : undefined}>{power}</dd>
        </div>
        <div>
          <dt>{t("devices")}</dt>
          <dd className="home-devices">
            {status?.devices ?? "–"}
            {addPhone && (
              <button type="button" className="btn secondary small home-add-phone" onClick={addPhone} title={t("addDevice")}>
                <Icon name="plus" size={14} />
                <span className="home-add-phone-text">{t("addDevice")}</span>
              </button>
            )}
          </dd>
        </div>
        <div>
          <dt>{t("addresses")}</dt>
          <dd>{status?.addresses?.join(", ") || "–"}</dd>
        </div>
      </dl>
      <HelpLink t={t} topic="home" />
      {stale && (
        <p id={asOfId} className="muted home-asof">
          {statusAt !== null ? `${t("asOf")} ${fmtDateTime(new Date(statusAt).toISOString())}` : t("showingLastKnown")}
        </p>
      )}
    </header>
  );
}

/** A card's title with its icon, and a link to the screen it comes from. */
function CardHead({ id, icon, title, href, link }: { id: string; icon: IconName; title: string; href: string; link: string }) {
  return (
    <div className="home-card-head">
      <h2 id={id}>
        <span className="home-card-icon">
          <Icon name={icon} size={18} />
        </span>
        {title}
      </h2>
      <a className="home-link" href={href}>{link}</a>
    </div>
  );
}

/** Something in the supplies that needs attention. */
type Attention = {
  item: Item;
  kind: "expired" | "soon" | "low";
  /** Days until it expires (below 0: since it did). */
  days: number;
  /** Below its "Warn below" amount (also when it is listed for its date). */
  low: boolean;
};

/**
 * One list of what needs attention, the most urgent first: what has expired
 * (the longest ago first), what expires within 30 days (the soonest first),
 * then what is running low. Something that is more than one of these is
 * listed once, where it comes first.
 */
function needsAttention(sum: SuppliesSummary): Attention[] {
  const lowIds = new Set(sum.running_low.map((i) => i.id));
  const seen = new Set<string>();
  const out: Attention[] = [];
  const add = (item: Item, kind: Attention["kind"]) => {
    if (seen.has(item.id)) return;
    seen.add(item.id);
    out.push({ item, kind, days: item.expiry ? daysUntil(item.expiry) : 0, low: lowIds.has(item.id) });
  };
  const byDate = (a: Item, b: Item) => (a.expiry ?? "").localeCompare(b.expiry ?? "");
  for (const i of [...sum.expired].sort(byDate)) add(i, "expired");
  for (const i of [...sum.expiring_soon].sort(byDate)) add(i, "soon");
  for (const i of sum.running_low) add(i, "low");
  return out;
}

/** "2 days" / "2 dana". */
function dayCount(n: number): string {
  return `${n} ${countWord(n, ["day", "days"], ["dan", "dana", "dana"])}`;
}

/** What is wrong, in a few words: "expired 2 days ago", "expires tomorrow", "2 of 3 kg". */
function whatIsWrong(t: T, a: Attention): string {
  const { item, days } = a;
  if (a.kind === "low") {
    const min = item.min_quantity ?? 0;
    return `${fmtQty(item.quantity)} ${t("of")} ${fmtQty(min)} ${unitName(t, item.unit, min)}`;
  }
  if (a.kind === "expired") {
    if (days >= 0) return t("expired");
    if (days === -1) return t("attnExpiredYesterday");
    if (days >= -30) return t("attnExpiredAgo").replace("{n}", dayCount(-days));
    return t("attnExpiredOn").replace("{date}", fmtDate(item.expiry ?? ""));
  }
  if (days <= 0) return t("attnExpiresToday");
  if (days === 1) return t("attnExpiresTomorrow");
  return t("attnExpiresIn").replace("{n}", dayCount(days));
}

/** On the shopping list already: added (by hand or from here), or suggested there because it runs low. */
function onShoppingList(shop: ShoppingEntry[], item: Item): boolean {
  const name = fold(item.name);
  return shop.some((e) => e.status === "open" && (e.item_id === item.id || e.id === `low:${item.id}` || fold(e.text) === name));
}

/**
 * The supplies at a glance: one list of what needs attention (a line each:
 * the name, what is wrong and, where it helps, "Add to shopping list"), or a
 * calm "All good"; below it one line with the shopping list and the rest.
 */
function SuppliesCard({
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
  const listId = useId();
  const [adding, setAdding] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  // Added from here: on the list at once, before the list from the hub (or the phone's copy) shows it.
  const [added, setAdded] = useState<string[]>([]);
  const waiting = added.filter((id) => !shop?.some((e) => e.status === "open" && e.item_id === id));
  const listed = (item: Item) => waiting.includes(item.id) || (shop !== null && onShoppingList(shop, item));

  const all = sum ? needsAttention(sum) : [];
  const toBuy = (shop ?? []).filter((e) => e.status === "open").length + waiting.length;
  const toPutAway = sum?.to_put_away ?? 0;

  const addToList = async (a: Attention) => {
    const { item } = a;
    setAdding(item.id);
    setErr(null);
    // Enough to be back at the "Warn below" amount; for something expired, as much as there was.
    const missing = (item.min_quantity ?? 0) - item.quantity;
    const quantity = a.kind === "expired" ? Math.max(item.quantity, missing) : missing;
    try {
      await api("/api/shopping", {
        json: { text: item.name, quantity: quantity > 0 ? Math.round(quantity * 1000) / 1000 : null, unit: item.unit, item_id: item.id },
      });
      setAdded((list) => [...list.filter((id) => id !== item.id), item.id]);
      setTimeout(() => setAdded((list) => list.filter((id) => id !== item.id)), ADDED_KEEP);
      reload();
    } catch (e) {
      setErr(errText(t, e));
    } finally {
      setAdding(null);
    }
  };

  return (
    <section className="panel left home-card home-supplies" aria-labelledby={id}>
      <CardHead id={id} icon="supplies" title={t("supplies")} href="#supplies" link={t("homeOpenSupplies")} />
      {unavailable ? (
        <p className="warn home-empty">{t("unavailable")}</p>
      ) : sum === null ? (
        <p className="muted home-empty">{t("aiLoading")}</p>
      ) : all.length === 0 ? (
        <p className="home-allgood">
          <span className="home-allgood-icon">
            <Icon name="check" size={16} />
          </span>
          <span>
            <strong>{t("homeAllGood")}</strong>
            <span className="muted">: {t("homeAllGoodMore")}</span>
          </span>
        </p>
      ) : (
        <section className="home-part" aria-labelledby={listId}>
          <h3 id={listId} className="home-sub">
            {t("homeNeedsAttention")} <span className={"home-count " + (all[0].kind === "expired" ? "warn" : "soon")}>{all.length}</span>
          </h3>
          <ul className="attn-list">
            {all.slice(0, ATTENTION).map((a) => {
              const shopping = a.kind === "expired" || a.low;
              const on = shopping && listed(a.item);
              return (
                <li key={a.item.id} className={`attn-row ${a.kind}`}>
                  <span className="attn-name" title={a.item.name}>{a.item.name}</span>
                  <span className="attn-what">{whatIsWrong(t, a)}</span>
                  <span className="attn-act">
                    {shopping && !on && (
                      <button
                        type="button"
                        className="btn secondary small attn-add"
                        onClick={() => addToList(a)}
                        disabled={adding !== null}
                        aria-label={`${t("addToShopping")}: ${a.item.name}`}
                        title={t("addToShopping")}
                      >
                        <Icon name="cart" size={16} />
                        <span className="attn-add-text">{t("addToShopping")}</span>
                      </button>
                    )}
                    {on && (
                      <span className="attn-listed" title={t("onShoppingList")}>
                        <Icon name="check" size={16} />
                        <span className="attn-listed-text">{t("onShoppingList")}</span>
                      </span>
                    )}
                  </span>
                </li>
              );
            })}
          </ul>
          {all.length > ATTENTION && (
            <a className="home-link" href="#supplies">{t("homeMoreN").replace("{n}", String(all.length - ATTENTION))}</a>
          )}
        </section>
      )}
      {err && <p className="error home-empty" role="alert">{err}</p>}
      {(sum !== null || note) && (
        <div className="home-foot">
          {shop !== null && (
            <a className="home-link" href="#supplies/shopping">
              {toBuy > 0 ? `${t("homeToBuy")}: ${toBuy} ${countWord(toBuy, ["item", "items"], ["stavka", "stavke", "stavki"])}` : t("homeShoppingEmpty")}
            </a>
          )}
          {toPutAway > 0 && (
            <a className="home-link" href="#supplies/putaway">
              {t("homeToPutAway")}: {toPutAway}
            </a>
          )}
          {sum !== null &&
            (sum.total_items > 0 ? (
              <span className="muted">
                {t("homeItemsTotal")}: {sum.total_items}
              </span>
            ) : (
              <span className="muted">
                {t("homeNoSupplies")} <a className="home-link" href="#supplies">{t("addItem")}</a>
              </span>
            ))}
          {note && <span className="muted home-note">{note}</span>}
        </div>
      )}
    </section>
  );
}

/** "Ask anything": the question starts a new conversation in the Assistant. Below it, the last few conversations of this device. */
function AssistantCard({ t, go, chats, failed, saved }: { t: T; go: (tab: string) => void; chats: Chat[] | null; failed: boolean; saved: boolean }) {
  const id = useId();
  const recentId = useId();
  const [question, setQuestion] = useState("");
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    const q = question.trim();
    if (!q) return;
    askInAssistant(q);
    go("assistant");
  };
  const open = (chat: string) => {
    openInAssistant(chat);
    go("assistant");
  };
  return (
    <section className="panel left home-card home-ask" aria-labelledby={id}>
      <CardHead id={id} icon="assistant" title={t("assistant")} href="#assistant" link={t("homeAllChats")} />
      <form className="home-ask-form" onSubmit={submit}>
        <div className="composer-box home-ask-box">
          <input
            type="text"
            value={question}
            onChange={(e) => setQuestion(e.target.value)}
            placeholder={t("homeAskPlaceholder")}
            aria-label={t("homeAskLabel")}
            maxLength={2000}
            enterKeyHint="send"
          />
          <button className="send-btn" disabled={!question.trim()} aria-label={t("ask")} title={t("ask")}>
            <Icon name="send" size={20} />
          </button>
        </div>
      </form>
      {saved && (
        <section className="home-part" aria-labelledby={recentId}>
          <h3 id={recentId} className="home-sub">{t("homeRecentChats")}</h3>
          {chats === null ? (
            <p className={(failed ? "warn" : "muted") + " home-empty"}>{failed ? t("unavailable") : t("aiLoading")}</p>
          ) : chats.length === 0 ? (
            <p className="muted home-empty">{t("homeNoChats")}</p>
          ) : (
            <ul className="home-recent">
              {chats.slice(0, RECENT).map((c) => {
                const from = fromText(t, c);
                const title = c.title || t("aiNewChat");
                return (
                  <li key={c.id}>
                    <button type="button" className="home-recent-item" onClick={() => open(c.id)} title={title}>
                      <span className="home-recent-title">
                        {from && (
                          <span className="convo-from">
                            <Icon name="shared" size={14} />
                          </span>
                        )}
                        <span className="home-recent-text">{title}</span>
                      </span>
                      <span className="muted home-recent-meta">
                        {from ? `${from} · ` : ""}
                        {fmtDateTime(c.updated_at)}
                      </span>
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
        </section>
      )}
    </section>
  );
}

type Download = { key: string; name: string; status: string; pct: number; detail: string };

/** The library's drive, what is downloading (with bars), and what waits: new versions, paused and failed downloads. */
function AddonsCard({
  t,
  lang,
  cat,
  maps,
  failed,
  away,
  go,
}: {
  t: T;
  lang: Lang;
  cat: CatalogReply | null;
  maps: MapsReply | null;
  failed: boolean;
  away: boolean;
  go: (tab: string) => void;
}) {
  const id = useId();
  const dlId = useId();
  const head = <CardHead id={id} icon="addons" title={t("homeLibraryAddons")} href="#addons" link={t("goToAddons")} />;
  if (!cat) {
    return (
      <section className="panel left home-card home-addons" aria-labelledby={id}>
        {head}
        <p className={(failed || away ? "warn" : "muted") + " home-empty"}>{failed || away ? t("unavailable") : t("aiLoading")}</p>
      </section>
    );
  }
  const title = (l: Localized) => (lang === "sr" && l.sr ? l.sr : l.en);
  const downloads: Download[] = [];
  let paused = 0;
  let broken = 0;
  let updates = 0;
  for (const p of cat.packs) {
    const s = p.state;
    const total = s.bytes_total || p.size;
    const pct = total ? Math.min(100, Math.round((s.bytes_done / total) * 100)) : 0;
    if (BUSY.includes(s.status)) {
      const speed = s.status === "downloading" && s.speed > 0 ? ` · ${fmtBytes(s.speed)}/s` : "";
      downloads.push({
        key: p.id,
        name: title(p.title),
        status: s.status === "queued" ? t("queued") : s.status === "verifying" ? t("verifying") : `${pct}%`,
        pct: s.status === "verifying" ? 100 : pct,
        detail: `${fmtBytes(s.bytes_done)} / ${fmtBytes(total)}${speed}`,
      });
    } else if (s.status === "paused") paused++;
    else if (s.status === "failed") broken++;
    else if (s.status === "installed" && s.update_available) updates++;
  }
  for (const c of maps?.countries ?? []) {
    const s = countryState(c);
    if (s.busy) {
      const pct = c.size ? Math.min(100, Math.round((s.done / c.size) * 100)) : 0;
      downloads.push({
        key: `map:${c.id}`,
        name: `${t("mapsOf")}: ${lang === "sr" ? c.name_sr : c.name}`,
        status: `${pct}%`,
        pct,
        detail: `${fmtBytes(s.done)} / ${fmtBytes(c.size)}`,
      });
    } else if (s.failed) broken++;
    else if (s.paused) paused++;
    if (s.update) updates++;
  }
  const root = cat.library_drive ?? "";
  const libName = `${t("hubDisk")}${/^[A-Za-z]:/.test(root) ? ` (${rootName(root)})` : ""}`;
  const addonsBytes = cat.packs.reduce((s, p) => s + (p.state.status === "installed" ? p.size : 0), 0) + (maps?.installed_bytes ?? 0);
  return (
    <section className="panel left home-card home-addons" aria-labelledby={id}>
      {head}
      <DriveTile
        t={t}
        kind="disk"
        name={libName}
        badge={t("libraryDriveBadge")}
        free={cat.system.disk_free}
        total={cat.system.disk_total}
        note={
          <span className="muted">
            {addonsBytes > 0 ? `${t("addonsUse")}: ${fmtBytes(addonsBytes)}` : t("nothingInstalled")}
          </span>
        }
        open={() => go("addons/library")}
      />
      <section className="home-part" aria-labelledby={dlId}>
        <h3 id={dlId} className="home-sub">
          {t("homeDownloads")} <span className="home-count">{downloads.length}</span>
        </h3>
        {downloads.length === 0 ? (
          <p className="muted home-empty">{t("homeNoDownloads")}</p>
        ) : (
          <ul className="home-downloads">
            {downloads.slice(0, SHOW).map((d) => (
              <li key={d.key}>
                <div className="home-dl-line">
                  <span className="home-dl-name" title={d.name}>{d.name}</span>
                  <span className="muted">{d.status}</span>
                </div>
                <div className="bar" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={d.pct} aria-label={d.name}>
                  <i style={{ width: `${d.pct}%` }} />
                </div>
                <div className="muted small-text">{d.detail}</div>
              </li>
            ))}
          </ul>
        )}
        {downloads.length > SHOW && (
          <a className="home-link" href="#addons/library">{t("homeMoreN").replace("{n}", String(downloads.length - SHOW))}</a>
        )}
      </section>
      {(updates > 0 || paused > 0 || broken > 0) && (
        <ul className="home-notes">
          {updates > 0 && (
            <li>
              <a className="home-link" href="#addons/library">{t("packUpdateAvailable")}: {updates}</a>
            </li>
          )}
          {paused > 0 && (
            <li>
              <a className="home-link" href="#addons/library">{t("pausedStatus")}: {paused}</a>
            </li>
          )}
          {broken > 0 && (
            <li>
              <a className="home-link warn" href="#addons/library">{t("failedStatus")}: {broken}</a>
            </li>
          )}
        </ul>
      )}
      {failed && <p className="muted home-note">{t("showingLastKnown")}</p>}
    </section>
  );
}

/**
 * The household's home on the map, at about a town's size. Without a home
 * location the map shows the world, with a button to set it.
 */
function HomeMapCard({ t, lang, info, unreachable }: { t: T; lang: Lang; info: MapInfo | null; unreachable: boolean }) {
  const id = useId();
  const home = info?.home ?? null;
  return (
    <section className="panel left home-card home-map" aria-labelledby={id}>
      <CardHead id={id} icon="maps" title={t("homeMapTitle")} href="#maps" link={t("openMaps")} />
      <ZaklonMap t={t} lang={lang} info={info} home={home} start="home" compact label={t("homeMapTitle")}>
        {unreachable && !info && <p className="zmap-note warn">{t("mapUnreachable")}</p>}
        {info && !home && !info.phone && (
          <a className="btn small zmap-set-home" href="#maps/home">
            {t("homeLocationSet")}
          </a>
        )}
      </ZaklonMap>
      {home && <p className="muted home-map-place">{homeText(home)}</p>}
    </section>
  );
}

/** The tools not already in the bar, and Help. */
function QuickAccess({ t, pinned }: { t: T; pinned: ToolId | null }) {
  const id = useId();
  const tiles: { id: string; title: Key; icon: IconName }[] = [
    ...TOOLS.filter((x) => x.id !== pinned).map((x) => ({ id: x.id, title: x.title, icon: x.id })),
    { id: "help", title: "help", icon: "help" },
  ];
  return (
    <section className="home-tools" aria-labelledby={id}>
      <h2 id={id} className="section-title">{t("homeQuickAccess")}</h2>
      <ul className="home-tool-grid">
        {tiles.map((x) => (
          <li key={x.id}>
            <a className="home-tool" href={`#${x.id}`}>
              <span className="tool-icon">
                <Icon name={x.icon} size={22} />
              </span>
              <span className="home-tool-title">{t(x.title)}</span>
            </a>
          </li>
        ))}
      </ul>
    </section>
  );
}
