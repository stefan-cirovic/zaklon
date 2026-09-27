// A phone away from home (at the shop, say) keeps working from its last
// copy of the supplies: everything reads from that copy, and the shopping
// list can still be changed. Those changes wait in an outbox on the phone
// and reach the hub the next time it is reachable. Only phones do this; the
// laptop is the hub.

const CACHE = "zaklon.cache:";
const OUTBOX = "zaklon.outbox";
/** Changes that were still waiting when the phone was unlinked, kept for that hub. */
const PARKED = "zaklon.outbox.parked";
const OFFLINE_EVENT = "zaklon-offline";

/** GET answers kept for when the hub is out of reach. */
const CACHED = [/^\/api\/items$/, /^\/api\/shopping$/, /^\/api\/put-away$/, /^\/api\/places$/, /^\/api\/supplies\/summary$/, /^\/api\/history(\?.*)?$/];

type Queued = { method: string; path: string; body: string | null; at: number };
type Cached = { at: number; body: string };
type Parked = { hub_id: string; items: Queued[] };
type ShoppingEntry = { id: string; item_id: string | null; text: string; quantity: number | null; unit: string | null; status: string; source: string };

let offlineSince: number | null = null;

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

export function isCacheable(method: string, path: string) {
  return method === "GET" && CACHED.some((re) => re.test(path));
}

export function remember(path: string, body: string) {
  write(CACHE + path, { at: Date.now(), body } satisfies Cached);
}

export function recall(path: string): Cached | null {
  return read<Cached>(CACHE + path);
}

function setOffline(on: boolean, since?: number) {
  const before = offlineSince;
  offlineSince = on ? (offlineSince ?? since ?? Date.now()) : null;
  if (before !== offlineSince) window.dispatchEvent(new CustomEvent(OFFLINE_EVENT));
}

/** The time of the copy the phone shows while the hub is out of reach, or null when online. */
export function offlineState(): { since: number | null; waiting: number } {
  return { since: offlineSince, waiting: outbox().length };
}

export function onOfflineChange(f: () => void): () => void {
  window.addEventListener(OFFLINE_EVENT, f);
  return () => window.removeEventListener(OFFLINE_EVENT, f);
}

export function markOnline() {
  setOffline(false);
}

/** Serve a GET from the copy; null when there is none. */
export function fromCache(path: string): string | null {
  const c = recall(path);
  if (!c) return null;
  setOffline(true, c.at);
  return c.body;
}

function outbox(): Queued[] {
  return read<Queued[]>(OUTBOX) ?? [];
}

/** Shopping list changes that can wait for the hub. */
export function isQueueable(method: string, path: string) {
  return method === "POST" && (path === "/api/shopping" || /^\/api\/shopping\/[^/]+\/(bought|dismiss)$/.test(path));
}

/**
 * Keep a shopping list change for later and apply it to the copy right away,
 * so the list on the screen is right. Returns what the hub would have answered.
 */
export function queue(method: string, path: string, body: string | null): string {
  const q = outbox();
  q.push({ method, path, body, at: Date.now() });
  write(OUTBOX, q);
  const list = JSON.parse(recall("/api/shopping")?.body ?? "[]") as ShoppingEntry[];
  let answer = "";
  if (path === "/api/shopping") {
    const b = body ? (JSON.parse(body) as Partial<ShoppingEntry>) : {};
    const entry: ShoppingEntry = {
      id: `offline-${q.length}-${Date.now()}`,
      item_id: b.item_id ?? null,
      text: b.text ?? "",
      quantity: b.quantity ?? null,
      unit: b.unit ?? null,
      status: "open",
      source: "manual",
    };
    // The queued add remembers its temporary id, so later changes to it can follow.
    q[q.length - 1].body = JSON.stringify({ ...b, offline_id: entry.id });
    write(OUTBOX, q);
    list.push(entry);
    answer = JSON.stringify(entry);
  } else {
    const id = decodeURIComponent(path.split("/")[3]);
    const i = list.findIndex((e) => e.id === id);
    if (i >= 0) list.splice(i, 1);
  }
  write(CACHE + "/api/shopping", { at: recall("/api/shopping")?.at ?? Date.now(), body: JSON.stringify(list) } satisfies Cached);
  window.dispatchEvent(new CustomEvent(OFFLINE_EVENT));
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

/** Remove the first waiting change, re-reading the outbox (a new change may have been added meanwhile). */
function dropFirst(sent: Queued) {
  const q = outbox();
  if (q.length > 0 && q[0].at === sent.at && q[0].path === sent.path) {
    write(OUTBOX, q.slice(1));
  }
}

/** After an add reached the hub, later changes to its temporary id use the hub's id. */
function renameInOutbox(offlineId: string, realId: string) {
  const q = outbox().map((item) =>
    item.path.includes(encodeURIComponent(offlineId)) || item.path.includes(offlineId)
      ? { ...item, path: item.path.replace(encodeURIComponent(offlineId), encodeURIComponent(realId)).replace(offlineId, realId) }
      : item,
  );
  write(OUTBOX, q);
}

async function flushOnce(send: (method: string, path: string, body: string | null) => Promise<{ status: number; body: string }>) {
  for (;;) {
    const q = outbox();
    if (q.length === 0) break;
    const item = q[0];
    let body = item.body;
    let offlineId: string | null = null;
    if (item.path === "/api/shopping" && body) {
      const b = JSON.parse(body) as Record<string, unknown>;
      offlineId = typeof b.offline_id === "string" ? b.offline_id : null;
      // The hub recognizes a repeated add by this id, so a reply lost on the
      // way back never makes a second entry.
      if (offlineId) b.client_id = offlineId;
      delete b.offline_id;
      body = JSON.stringify(b);
    } else {
      const id = decodeURIComponent(item.path.split("/")[3] ?? "");
      if (id.startsWith("offline-")) {
        // Its add is no longer waiting and never got a hub id: nothing to change.
        const stillWaiting = q.some((x) => x.path === "/api/shopping" && (x.body ?? "").includes(id));
        if (!stillWaiting) {
          dropFirst(item);
          continue;
        }
      }
    }
    const res = await send(item.method, item.path, body); // throws when the hub is still out of reach
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
    dropFirst(item);
  }
  window.dispatchEvent(new CustomEvent(OFFLINE_EVENT));
}

/**
 * Forget the copy kept for this hub (the phone was unlinked). Shopping list
 * changes that still wait are not thrown away: they are set aside for this
 * hub (`hubId`) and sent if the phone is paired with the same hub again (for
 * example after the hub was restored from an older backup). Returns how many
 * changes were set aside.
 */
export function clearOffline(hubId?: string | null): number {
  const waiting = outbox();
  let parked = 0;
  if (waiting.length > 0 && hubId) {
    const before = read<Parked>(PARKED);
    // Earlier set-aside changes for the same hub go first.
    const items = before && before.hub_id === hubId ? [...before.items, ...waiting] : waiting;
    write(PARKED, { hub_id: hubId, items } satisfies Parked);
    parked = items.length;
  }
  try {
    const keys: string[] = [];
    for (let i = 0; i < localStorage.length; i++) {
      const k = localStorage.key(i);
      if (k && (k.startsWith(CACHE) || k === OUTBOX)) keys.push(k);
    }
    keys.forEach((k) => localStorage.removeItem(k));
  } catch {
    /* nothing kept */
  }
  offlineSince = null;
  window.dispatchEvent(new CustomEvent(OFFLINE_EVENT));
  return parked;
}

/** Changes set aside when the phone left `hubId`; 0 when none. */
export function parkedFor(hubId: string | null | undefined): number {
  const p = read<Parked>(PARKED);
  return p && hubId && p.hub_id === hubId ? p.items.length : 0;
}

/** The phone was paired: if it is the hub the set-aside changes were for, they wait to be sent again. */
export function adoptParked(hubId: string | null | undefined): number {
  const p = read<Parked>(PARKED);
  if (!p || !hubId || p.hub_id !== hubId) return 0;
  write(OUTBOX, [...p.items, ...outbox()]);
  try {
    localStorage.removeItem(PARKED);
  } catch {
    /* ignore */
  }
  window.dispatchEvent(new CustomEvent(OFFLINE_EVENT));
  return p.items.length;
}
