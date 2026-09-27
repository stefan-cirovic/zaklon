// A phone away from home (at the shop, say) keeps working from its last
// copy of the supplies: everything reads from that copy, and the shopping
// list can still be changed. Those changes wait in an outbox on the phone
// and reach the hub the next time it is reachable. Only phones do this; the
// laptop is the hub.

const CACHE = "zaklon.cache:";
const OUTBOX = "zaklon.outbox";
const OFFLINE_EVENT = "zaklon-offline";

/** GET answers kept for when the hub is out of reach. */
const CACHED = [/^\/api\/items$/, /^\/api\/shopping$/, /^\/api\/put-away$/, /^\/api\/places$/, /^\/api\/supplies\/summary$/, /^\/api\/history(\?.*)?$/];

type Queued = { method: string; path: string; body: string | null; at: number };
type Cached = { at: number; body: string };
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

/**
 * Send what waited, in order. `send` performs one request and returns the
 * hub's status and body; a network failure throws and stops the flush.
 */
export async function flush(send: (method: string, path: string, body: string | null) => Promise<{ status: number; body: string }>) {
  let q = outbox();
  if (q.length === 0) return;
  const ids = new Map<string, string>(); // offline id -> the hub's id
  while (q.length > 0) {
    const item = q[0];
    let path = item.path;
    let body = item.body;
    let offlineId: string | null = null;
    if (path === "/api/shopping" && body) {
      const b = JSON.parse(body) as Record<string, unknown>;
      offlineId = typeof b.offline_id === "string" ? b.offline_id : null;
      delete b.offline_id;
      body = JSON.stringify(b);
    } else {
      const id = decodeURIComponent(path.split("/")[3] ?? "");
      if (id.startsWith("offline-")) {
        const real = ids.get(id);
        if (!real) {
          // Its add never reached the hub; nothing to do.
          q = q.slice(1);
          write(OUTBOX, q);
          continue;
        }
        path = path.replace(encodeURIComponent(id), encodeURIComponent(real)).replace(id, real);
      }
    }
    const res = await send(item.method, path, body); // throws when the hub is still out of reach
    if (res.status < 300 && offlineId) {
      try {
        ids.set(offlineId, (JSON.parse(res.body) as { id: string }).id);
      } catch {
        /* no id in the answer */
      }
    }
    // Sent, or refused for good (e.g. already bought on another phone): either way it is done.
    q = q.slice(1);
    write(OUTBOX, q);
  }
  window.dispatchEvent(new CustomEvent(OFFLINE_EVENT));
}
