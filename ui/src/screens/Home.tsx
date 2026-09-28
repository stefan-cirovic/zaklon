import { useEffect, useId, useState, type ReactNode } from "react";
import { api, ApiError, type Status } from "../api";
import type { Key, Lang } from "../i18n";
import { offlineState, onBackOnline, onOfflineChange } from "../offline";
import { useVisiblePoll } from "../poll";
import { fmtBytes, fmtDateTime, fmtQty, unitName } from "../format";
import { askInAssistant, fromText, openInAssistant, type Summary as Chat } from "../conversations";
import { BUSY, rootName, type CatalogReply, type Localized } from "../addons";
import { countryState, type MapsReply } from "../maps";
import { TOOLS, type ToolId } from "../tools";
import type { Item } from "./Supplies";
import ExpiryBadge from "../components/ExpiryBadge";
import HelpLink from "../components/HelpLink";
import Firewall from "../components/Firewall";
import { UpdateBanner } from "../components/Updates";
import { DriveTile } from "../components/AddonViews";
import { Icon, type IconName } from "../components/Icon";

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
type ShoppingEntry = { id: string; text: string; quantity: number | null; unit: string | null; status: string };

/** Supplies, the shopping list, the conversations and the add-ons are asked for again this often (never while the app is in the background). */
const EVERY = 30_000;
/** While something downloads, its bar moves this often. */
const BUSY_EVERY = 2_000;
/** The world's maps are a long answer: asked for rarely, unless a map is downloading. */
const MAPS_EVERY = 120_000;
/** How many entries of each list Home shows, and how many conversations. */
const SHOW = 5;
const RECENT = 3;

/**
 * Home: the household at a glance, using the whole window. The hub in one
 * line at the top, then what needs attention, then the assistant, the
 * supplies, the library and add-ons, and the tools. A phone away from home
 * shows its copy of the supplies and conversations, marked as old.
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

  // No data at all: say so instead of "nothing expires", which would read as all is well.
  const unavailable = sum === null && (sumFailed || (!canLoad && !!error));
  // The hub stopped answering: the facts at the top are the last ones it gave.
  const stale = !!error && status !== null;
  const suppliesNote = away && since !== null ? `${t("asOf")} ${fmtDateTime(new Date(since).toISOString())}` : sum !== null && sumFailed ? t("showingLastKnown") : null;

  return (
    <div className="home">
      <HubHead t={t} status={status} statusAt={statusAt} up={up} stale={stale} system={cat?.system ?? null} />
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
        <SuppliesCard t={t} sum={sum} shop={shop} unavailable={unavailable} note={suppliesNote} />
        <AddonsCard t={t} lang={lang} cat={cat} maps={maps} failed={catFailed} away={away} go={go} />
        <QuickAccess t={t} pinned={pinned} />
      </div>
    </div>
  );
}

/** The hub in one line: its name, whether it runs, its power, the paired devices and its address. */
function HubHead({
  t,
  status,
  statusAt,
  up,
  stale,
  system,
}: {
  t: T;
  status: Status | null;
  statusAt: number | null;
  up: boolean;
  stale: boolean;
  system: CatalogReply["system"] | null;
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
          <dd>{status?.devices ?? "–"}</dd>
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

/** One list of a card: its name and how many, the first few entries, and a link to the rest. */
function Part({
  t,
  title,
  count,
  tone,
  empty,
  more,
  href,
  link,
  children,
}: {
  t: T;
  title: string;
  count: number;
  tone?: "warn" | "soon";
  empty: string;
  /** How many are not shown. */
  more: number;
  href: string;
  /** A link that is always there (otherwise only when some are not shown). */
  link?: string;
  children: ReactNode;
}) {
  const id = useId();
  const moreText = more > 0 ? t("homeMoreN").replace("{n}", String(more)) : "";
  return (
    <section className="home-part" aria-labelledby={id}>
      <h3 id={id} className="home-sub">
        {title} <span className={"home-count" + (tone && count > 0 ? ` ${tone}` : "")}>{count}</span>
      </h3>
      {count === 0 ? <p className="muted home-empty">{empty}</p> : <div className="mini-list">{children}</div>}
      {(link || more > 0) && (
        <a className="home-link" href={href}>
          {link ? `${link}${moreText ? ` (${moreText})` : ""}` : moreText}
        </a>
      )}
    </section>
  );
}

function SuppliesCard({
  t,
  sum,
  shop,
  unavailable,
  note,
}: {
  t: T;
  sum: SuppliesSummary | null;
  shop: ShoppingEntry[] | null;
  unavailable: boolean;
  /** When the lists are old: from when, or that the hub does not answer. */
  note: string | null;
}) {
  const id = useId();
  const expired = sum?.expired ?? [];
  const expiring = [...expired, ...(sum?.expiring_soon ?? [])];
  const low = sum?.running_low ?? [];
  const toBuy = (shop ?? []).filter((e) => e.status === "open");
  const toPutAway = sum?.to_put_away ?? 0;
  return (
    <section className="panel left home-card home-supplies" aria-labelledby={id}>
      <CardHead id={id} icon="supplies" title={t("supplies")} href="#supplies" link={t("homeOpenSupplies")} />
      {unavailable ? (
        <p className="warn home-empty">{t("unavailable")}</p>
      ) : sum === null ? (
        <p className="muted home-empty">{t("aiLoading")}</p>
      ) : (
        <div className="home-cols">
          <Part t={t} title={t("expiringSoon")} count={expiring.length} tone={expired.length > 0 ? "warn" : "soon"} empty={t("homeNothingExpires")} more={expiring.length - SHOW} href="#supplies">
            {expiring.slice(0, SHOW).map((i) => (
              <div key={i.id} className="mini-row">
                <span title={i.name}>{i.name}</span>
                <ExpiryBadge date={i.expiry} t={t} />
              </div>
            ))}
          </Part>
          <Part t={t} title={t("runningLow")} count={low.length} tone="warn" empty={t("homeNothingLow")} more={low.length - SHOW} href="#supplies">
            {low.slice(0, SHOW).map((i) => (
              <div key={i.id} className="mini-row">
                <span title={i.name}>{i.name}</span>
                <span className="muted">
                  {fmtQty(i.quantity)} / {fmtQty(i.min_quantity ?? 0)} {unitName(t, i.unit, i.min_quantity ?? 0)}
                </span>
              </div>
            ))}
          </Part>
          <Part
            t={t}
            title={t("shoppingList")}
            count={toBuy.length}
            empty={shop === null ? t("unavailable") : t("homeShoppingEmpty")}
            more={toBuy.length - SHOW}
            href="#supplies/shopping"
            link={t("homeOpenShopping")}
          >
            {toBuy.slice(0, SHOW).map((e) => (
              <div key={e.id} className="mini-row">
                <span title={e.text}>{e.text}</span>
                {e.quantity !== null && (
                  <span className="muted">
                    {fmtQty(e.quantity)} {e.unit ? unitName(t, e.unit, e.quantity) : ""}
                  </span>
                )}
              </div>
            ))}
          </Part>
        </div>
      )}
      {(sum !== null || note) && (
        <div className="home-foot">
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
          {toPutAway > 0 && (
            <a className="home-link" href="#supplies/putaway">
              {t("homeToPutAway")}: {toPutAway}
            </a>
          )}
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
            {t("addonsUse")}: {fmtBytes(addonsBytes)}
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
