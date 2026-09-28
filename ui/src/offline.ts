// A phone away from home (at the shop, say) keeps working from its last
// copy of the supplies: everything reads from that copy, and the shopping
// list can still be changed. Those changes wait in an outbox on the phone
// and reach the hub the next time it is reachable. Only phones do this; the
// laptop is the hub.
//
// The copies of the hub's answers are only a convenience and live in the web
// view's storage. The outbox is the person's own work: on phones it is kept
// in files the app writes to the disk before a change counts as saved (see
// `setStore`), because the web view writes its storage a few seconds late
// and loses the last changes when Android kills the app.

const CACHE = "zaklon.cache:";
/** Where the outbox and the set-aside changes lived before they moved into files (and still live in tests and browsers). */
const LEGACY = { outbox: "zaklon.outbox", parked: "zaklon.outbox.parked" } as const;
const OFFLINE_EVENT = "zaklon-offline";
const PROBE_EVENT = "zaklon-probe";
const ONLINE_EVENT = "zaklon-online";
/** Ask the app to look for the hub at most this often when a screen was served from the copy. */
const PROBE_EVERY = 5000;

/** One saved conversation with the assistant (the list of them is kept too). */
const ONE_CONVERSATION = /^\/api\/conversations\/[^/?]+$/;
/** GET answers kept for when the hub is out of reach. */
const CACHED = [
  /^\/api\/items$/, /^\/api\/shopping$/, /^\/api\/put-away$/, /^\/api\/places$/, /^\/api\/supplies\/summary$/, /^\/api\/history(\?.*)?$/,
  /^\/api\/conversations$/, ONE_CONVERSATION,
];
/** Conversations kept for reading away from the hub: the ones opened last. */
const KEEP_CONVERSATIONS = 30;
/** Which conversations are kept, the one opened last at the end. */
const KEPT_CONVERSATIONS = CACHE + "#conversations";

type Queued = { method: string; path: string; body: string | null; at: number };
type Cached = { at: number; body: string };
/** Changes that were still waiting when the phone was unlinked, kept for that hub. */
type Parked = { hub_id: string; hub_name: string | null; items: Queued[] };
type ShoppingEntry = { id: string; item_id: string | null; text: string; quantity: number | null; unit: string | null; status: string; source: string };

export type StoreName = keyof typeof LEGACY;
/** Durable storage for the outbox and the set-aside changes. `save` resolves only once the data is safely stored. */
export type Store = { load: (name: StoreName) => Promise<string | null>; save: (name: StoreName, data: string) => Promise<void> };

/** The web view's own storage, checked by reading it back. */
const webStore: Store = {
  load: async (name) => localStorage.getItem(LEGACY[name]),
  save: async (name, data) => {
    localStorage.setItem(LEGACY[name], data);
    if (localStorage.getItem(LEGACY[name]) !== data) throw new Error("could not save on this phone");
  },
};

let store: Store = webStore;
let ready: Promise<void> | null = null;
let box: Queued[] = [];
let parked: Parked[] = [];
let offlineSince: number | null = null;
/** When a request last failed to reach the hub; null once it answers again. */
let lastMiss: number | null = null;
let lastProbe = 0;
/** Bumped when the phone leaves its hub, so a send still running stops. */
let generation = 0;

function read<T>(key: string): T | null {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : null;
  } catch {
    return null;
  }
}

function write(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* storage full or blocked: work without the copy */
  }
}

function changed() {
  window.dispatchEvent(new CustomEvent(OFFLINE_EVENT));
}

/** Random id: 32 hex digits. */
export function newId(): string {
  const b = new Uint8Array(16);
  crypto.getRandomValues(b);
  return Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");
}

function parseQueue(raw: string | null): Queued[] {
  try {
    const q = raw ? (JSON.parse(raw) as unknown) : [];
    return Array.isArray(q) ? (q as Queued[]) : [];
  } catch {
    return [];
  }
}

function parseParked(raw: string | null): Parked[] {
  try {
    const p = raw ? (JSON.parse(raw) as unknown) : [];
    // An older version kept one hub's changes as a single object.
    const list = Array.isArray(p) ? p : p && typeof p === "object" ? [p] : [];
    return (list as Parked[]).filter((x) => x && typeof x.hub_id === "string" && Array.isArray(x.items)).map((x) => ({ ...x, hub_name: x.hub_name ?? null }));
  } catch {
    return [];
  }
}

/**
 * Keep the outbox in `s` from now on (phones: files written by the app).
 * Changes an older version kept in the web view's storage move there.
 */
export function setStore(s: Store | null) {
  store = s ?? webStore;
  ready = null;
}

/** Load the outbox; everything that reads or changes it waits for this. */
export function offlineReady(): Promise<void> {
  if (!ready) {
    ready = (async () => {
      const [o, p] = await Promise.all([store.load("outbox"), store.load("parked")]);
      box = parseQueue(o);
      parked = parseParked(p);
      if (store !== webStore) {
        const oldBox = parseQueue(localStorage.getItem(LEGACY.outbox));
        const oldParked = parseParked(localStorage.getItem(LEGACY.parked));
        if (oldBox.length > 0 || oldParked.length > 0) {
          box = [...oldBox, ...box];
          parked = [...oldParked, ...parked];
          await persist("outbox");
          await persist("parked");
          try {
            localStorage.removeItem(LEGACY.outbox);
            localStorage.removeItem(LEGACY.parked);
          } catch {
            /* moved; the old copy is harmless */
          }
        }
      }
      changed();
    })().catch((e) => {
      ready = null;
      throw e;
    });
  }
  return ready;
}

let saving: Promise<void> = Promise.resolve();

/** Write the current outbox (or set-aside changes) through to the store, one write at a time. */
function persist(name: StoreName): Promise<void> {
  const run = saving.then(() => store.save(name, JSON.stringify(name === "outbox" ? box : parked)));
  saving = run.catch(() => {});
  return run.catch((e) => {
    const msg = e instanceof Error ? e.message : String(e);
    throw new Error(/could not save/i.test(msg) ? msg : `could not save on this phone: ${msg}`);
  });
}

export function isCacheable(method: string, path: string) {
  return method === "GET" && CACHED.some((re) => re.test(path));
}

export function remember(path: string, body: string) {
  write(CACHE + path, { at: Date.now(), body } satisfies Cached);
  if (ONE_CONVERSATION.test(path)) {
    // Only the last few: the phone's storage is small and the supplies come first.
    const kept = (read<string[]>(KEPT_CONVERSATIONS) ?? []).filter((p) => typeof p === "string" && p !== path);
    kept.push(path);
    while (kept.length > KEEP_CONVERSATIONS) forget(kept.shift() as string);
    write(KEPT_CONVERSATIONS, kept);
  }
}

/** Drop the copy of a GET (a conversation that was deleted). */
export function forget(path: string) {
  try {
    localStorage.removeItem(CACHE + path);
  } catch {
    /* nothing kept */
  }
}

export function recall(path: string): Cached | null {
  return read<Cached>(CACHE + path);
}

function setOffline(on: boolean, since?: number) {
  const before = offlineSince;
  offlineSince = on ? (offlineSince ?? since ?? Date.now()) : null;
  if (before !== offlineSince) changed();
  if (before !== null && offlineSince === null) window.dispatchEvent(new CustomEvent(ONLINE_EVENT));
}

/** The hub answered again after the phone showed its copy: screens load the fresh data. */
export function onBackOnline(f: () => void): () => void {
  window.addEventListener(ONLINE_EVENT, f);
  return () => window.removeEventListener(ONLINE_EVENT, f);
}

/** The time of the copy the phone shows while the hub is out of reach, or null when online. */
export function offlineState(): { since: number | null; waiting: number } {
  return { since: offlineSince, waiting: box.length };
}

export function onOfflineChange(f: () => void): () => void {
  window.addEventListener(OFFLINE_EVENT, f);
  return () => window.removeEventListener(OFFLINE_EVENT, f);
}

/** The hub answered. */
export function markOnline() {
  lastMiss = null;
  setOffline(false);
}

/** A request did not reach the hub: the next ones are answered from the copy at once. */
export function markMissed() {
  lastMiss = Date.now();
}

/** The last request did not reach the hub (the phone is probably away from home). */
export function knownAway(): boolean {
  return lastMiss !== null;
}

/** A screen was served from the copy: ask the app to look for the hub now (not too often). */
export function requestProbe() {
  const now = Date.now();
  if (now - lastProbe < PROBE_EVERY) return;
  lastProbe = now;
  window.dispatchEvent(new CustomEvent(PROBE_EVENT));
}

export function onProbeRequest(f: () => void): () => void {
  window.addEventListener(PROBE_EVENT, f);
  return () => window.removeEventListener(PROBE_EVENT, f);
}

/** Serve a GET from the copy; null when there is none. */
export function fromCache(path: string): string | null {
  const c = recall(path);
  if (!c) return null;
  setOffline(true, c.at);
  return path === "/api/shopping" ? withWaiting(c.body) : c.body;
}

/** Shopping list changes that can wait for the hub. */
export function isQueueable(method: string, path: string) {
  return method === "POST" && (path === "/api/shopping" || /^\/api\/shopping\/[^/]+\/(bought|dismiss)$/.test(path));
}

/**
 * Every shopping list add carries a random id, also when it goes straight to
 * the hub: if the reply is lost and the add waits in the outbox, the hub
 * recognises the second copy and does not make a second entry.
 */
export function withClientId(method: string, path: string, body: string | undefined): string | undefined {
  if (method !== "POST" || path !== "/api/shopping" || !body) return body;
  try {
    const b = JSON.parse(body) as Record<string, unknown>;
    if (typeof b.client_id === "string" && b.client_id) return body;
    return JSON.stringify({ ...b, client_id: newId() });
  } catch {
    return body;
  }
}

function waitingEntry(q: Queued): ShoppingEntry | null {
  try {
    const b = JSON.parse(q.body ?? "{}") as Partial<ShoppingEntry> & { offline_id?: string };
    if (!b.offline_id) return null;
    return { id: b.offline_id, item_id: b.item_id ?? null, text: b.text ?? "", quantity: b.quantity ?? null, unit: b.unit ?? null, status: "open", source: "manual" };
  } catch {
    return null;
  }
}

/**
 * The shopping list as this phone shows it: the hub's list (or the copy of
 * it) with the changes still waiting applied, so things added at the shop do
 * not vanish before they reach the hub (and get typed in twice).
 */
export function withWaiting(listJson: string): string {
  if (box.length === 0) return listJson;
  let list: ShoppingEntry[];
  try {
    const parsed = JSON.parse(listJson) as unknown;
    if (!Array.isArray(parsed)) return listJson;
    list = parsed as ShoppingEntry[];
  } catch {
    return listJson;
  }
  for (const q of box) {
    if (q.path === "/api/shopping") {
      const e = waitingEntry(q);
      if (e && !list.some((x) => x.id === e.id)) list.push(e);
    } else {
      const id = decodeURIComponent(q.path.split("/")[3] ?? "");
      list = list.filter((x) => x.id !== id);
    }
  }
  return JSON.stringify(list);
}

/**
 * Keep a shopping list change for later. It counts as done only once it is
 * safely stored on the phone; otherwise this throws and nothing changes.
 * Returns what the hub would have answered.
 */
export async function queue(method: string, path: string, body: string | null): Promise<string> {
  await offlineReady();
  const item: Queued = { method, path, body, at: Date.now() };
  let answer = "";
  if (path === "/api/shopping") {
    const b = body ? (JSON.parse(body) as Partial<ShoppingEntry> & { client_id?: string }) : {};
    const clientId = typeof b.client_id === "string" && b.client_id ? b.client_id : newId();
    // The temporary id shown on the list until the hub gives the real one;
    // later changes to this entry follow it.
    const offlineId = `offline-${clientId}`;
    item.body = JSON.stringify({ ...b, client_id: clientId, offline_id: offlineId });
    answer = JSON.stringify(waitingEntry(item));
  }
  box.push(item);
  try {
    await persist("outbox");
  } catch (e) {
    box = box.filter((x) => x !== item);
    throw e;
  }
  changed();
  return answer;
}

let flushing: Promise<void> | null = null;

/**
 * Send what waited, in order, one flush at a time. `send` performs one
 * request and returns the hub's status and body; a network failure throws and
 * stops the flush (the rest waits for the next one).
 */
export function flush(send: (method: string, path: string, body: string | null) => Promise<{ status: number; body: string }>): Promise<void> {
  if (!flushing) {
    flushing = flushOnce(send).finally(() => {
      flushing = null;
    });
  }
  return flushing;
}

/** Wait (at most `ms`) for a send that is running to finish. */
export async function settle(ms: number): Promise<void> {
  const running = flushing;
  if (!running) return;
  await Promise.race([running.catch(() => {}), new Promise((r) => setTimeout(r, ms))]);
}

/** Remove a change that was sent (a new change may have been added meanwhile). */
async function drop(sent: Queued) {
  box = box.filter((x) => x !== sent);
  await persist("outbox").catch(() => {
    /* kept in the file: sent again after a restart, which the hub recognises */
  });
}

/** After an add reached the hub, later changes to its temporary id use the hub's id. */
function renameInOutbox(offlineId: string, realId: string) {
  box = box.map((item) =>
    item.path.includes(encodeURIComponent(offlineId)) || item.path.includes(offlineId)
      ? { ...item, path: item.path.replace(encodeURIComponent(offlineId), encodeURIComponent(realId)).replace(offlineId, realId) }
      : item,
  );
}

async function flushOnce(send: (method: string, path: string, body: string | null) => Promise<{ status: number; body: string }>) {
  await offlineReady();
  const mine = generation;
  for (;;) {
    if (mine !== generation) return; // the phone left its hub meanwhile
    const item = box[0];
    if (!item) break;
    let body = item.body;
    let offlineId: string | null = null;
    if (item.path === "/api/shopping" && body) {
      const b = JSON.parse(body) as Record<string, unknown>;
      offlineId = typeof b.offline_id === "string" ? b.offline_id : null;
      // The hub recognizes a repeated add by this id, so a reply lost on the
      // way back never makes a second entry. (Older queued adds used their
      // temporary id for it.)
      if (typeof b.client_id !== "string" && offlineId) b.client_id = offlineId;
      delete b.offline_id;
      body = JSON.stringify(b);
    } else {
      const id = decodeURIComponent(item.path.split("/")[3] ?? "");
      if (id.startsWith("offline-")) {
        // Its add is no longer waiting and never got a hub id: nothing to change.
        const stillWaiting = box.some((x) => x.path === "/api/shopping" && (x.body ?? "").includes(id));
        if (!stillWaiting) {
          await drop(item);
          continue;
        }
      }
    }
    const res = await send(item.method, item.path, body); // throws when the hub is still out of reach
    if (mine !== generation) return;
    if (res.status >= 500 || res.status === 408 || res.status === 429 || res.status === 401 || res.status === 403) {
      // The hub is busy or had a problem, or did not accept this phone (it
      // may have been reinstalled; unlinking sets the changes aside): keep
      // the change and try again later.
      throw new Error(`hub replied ${res.status}`);
    }
    if (res.status < 300 && offlineId) {
      try {
        renameInOutbox(offlineId, (JSON.parse(res.body) as { id: string }).id);
      } catch {
        /* no id in the answer */
      }
    }
    // Sent, or refused for good (e.g. already bought on another phone).
    await drop(item);
  }
  changed();
}

/**
 * Forget the copy kept for this hub (the phone was unlinked). Shopping list
 * changes that still wait are not thrown away: they are set aside for this
 * hub (`hubId`, called `hubName`). Pairing with the same hub again sends
 * them; pairing with another hub offers to send them there or discard them.
 * Returns how many changes are set aside for this hub.
 */
export async function clearOffline(hubId?: string | null, hubName?: string | null): Promise<number> {
  // Changes that could not even be read stay where they are (not overwritten).
  const loaded = await offlineReady().then(
    () => true,
    () => false,
  );
  generation++;
  let count = 0;
  if (box.length > 0 && hubId) {
    const before = parked.find((p) => p.hub_id === hubId);
    // Earlier set-aside changes for the same hub go first.
    const entry: Parked = { hub_id: hubId, hub_name: hubName ?? before?.hub_name ?? null, items: [...(before?.items ?? []), ...box] };
    parked = [...parked.filter((p) => p.hub_id !== hubId), entry];
    count = entry.items.length;
    await persist("parked");
  }
  box = [];
  if (loaded) await persist("outbox").catch(() => {});
  try {
    const keys: string[] = [];
    for (let i = 0; i < localStorage.length; i++) {
      const k = localStorage.key(i);
      if (k && k.startsWith(CACHE)) keys.push(k);
    }
    keys.forEach((k) => localStorage.removeItem(k));
  } catch {
    /* nothing kept */
  }
  offlineSince = null;
  lastMiss = null;
  changed();
  return count;
}

/** Changes set aside when the phone left `hubId`; 0 when none. */
export function parkedFor(hubId: string | null | undefined): number {
  return parked.find((p) => hubId && p.hub_id === hubId)?.items.length ?? 0;
}

/** Changes set aside for hubs other than `hubId` (a hub that was replaced). */
export function parkedElsewhere(hubId: string | null | undefined): { hub_id: string; hub_name: string | null; count: number }[] {
  return parked.filter((p) => p.hub_id !== hubId && p.items.length > 0).map((p) => ({ hub_id: p.hub_id, hub_name: p.hub_name, count: p.items.length }));
}

async function takeParked(fromHubId: string): Promise<Queued[]> {
  const p = parked.find((x) => x.hub_id === fromHubId);
  if (!p) return [];
  parked = parked.filter((x) => x !== p);
  return p.items;
}

/** The phone was paired: if it is the hub the set-aside changes were for, they wait to be sent again. */
export async function adoptParked(hubId: string | null | undefined): Promise<number> {
  await offlineReady();
  if (!hubId) return 0;
  const items = await takeParked(hubId);
  if (items.length === 0) return 0;
  box = [...items, ...box];
  await persist("outbox");
  await persist("parked");
  changed();
  return items.length;
}

/** Send changes set aside for another hub (one that was replaced) to the hub this phone is paired with now. */
export async function sendParkedHere(fromHubId: string): Promise<number> {
  await offlineReady();
  const items = await takeParked(fromHubId);
  box = [...box, ...items];
  await persist("outbox");
  await persist("parked");
  changed();
  return items.length;
}

/** Throw away changes set aside for another hub. */
export async function discardParked(fromHubId: string): Promise<void> {
  await offlineReady();
  await takeParked(fromHubId);
  await persist("parked");
  changed();
}
