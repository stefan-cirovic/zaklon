// Run: node e2e/unit/offline.check.mts  (Node 23.6+ strips the types)
import assert from "node:assert/strict";

const store = new Map<string, string>();
(globalThis as any).localStorage = {
  getItem: (k: string) => store.get(k) ?? null,
  setItem: (k: string, v: string) => void store.set(k, v),
  removeItem: (k: string) => void store.delete(k),
  key: (i: number) => [...store.keys()][i] ?? null,
  get length() {
    return store.size;
  },
};
const events: string[] = [];
const listeners = new Map<string, Set<() => void>>();
(globalThis as any).window = {
  dispatchEvent: (e: { type: string }) => {
    events.push(e.type);
    listeners.get(e.type)?.forEach((f) => f());
    return true;
  },
  addEventListener: (type: string, f: () => void) => {
    if (!listeners.has(type)) listeners.set(type, new Set());
    listeners.get(type)!.add(f);
  },
  removeEventListener: (type: string, f: () => void) => void listeners.get(type)?.delete(f),
};
(globalThis as any).CustomEvent = class { type: string; constructor(type: string) { this.type = type; } };

const off = await import("../../src/offline.ts");
await off.offlineReady();

// A phone that was at home saw the shopping list.
const list = [
  { id: "a", item_id: null, text: "Sugar", quantity: 1, unit: "kg", status: "open", source: "manual" },
  { id: "low:m", item_id: "m", text: "Milk", quantity: 2, unit: "l", status: "open", source: "running_low" },
];
assert.ok(off.isCacheable("GET", "/api/shopping"));
assert.ok(!off.isCacheable("POST", "/api/shopping"));
assert.ok(off.isCacheable("GET", "/api/history?limit=100"));
off.remember("/api/shopping", JSON.stringify(list));

// At the shop, without the hub:
assert.deepEqual(JSON.parse(off.fromCache("/api/shopping")!), list);
assert.notEqual(off.offlineState().since, null, "offline is shown");
assert.ok(off.isQueueable("POST", "/api/shopping/a/bought"));
assert.ok(!off.isQueueable("POST", "/api/items"));
await off.queue("POST", "/api/shopping/a/bought", null);
const added = JSON.parse(await off.queue("POST", "/api/shopping", JSON.stringify({ text: "Bread" })));
assert.match(added.id, /^offline-/);
assert.equal(added.text, "Bread");
// The waiting add is on the list the phone shows, the bought entry is not.
assert.deepEqual(JSON.parse(off.fromCache("/api/shopping")!).map((e: any) => e.id), ["low:m", added.id]);
await off.queue("POST", `/api/shopping/${encodeURIComponent(added.id)}/bought`, null);
const now = JSON.parse(off.fromCache("/api/shopping")!);
assert.deepEqual(now.map((e: any) => e.id), ["low:m"], "bought and added-then-bought are off the list");
assert.equal(off.offlineState().waiting, 3);
// The outbox is written through to the store at once.
assert.equal(JSON.parse(store.get("zaklon.outbox")!).length, 3);

// Back home: the outbox is sent in order, the new entry's real id is used.
const sent: string[] = [];
await off.flush(async (method, path, body) => {
  sent.push(`${method} ${path} ${body ?? ""}`);
  if (path === "/api/shopping") return { status: 201, body: JSON.stringify({ id: "real-1" }) };
  return { status: 204, body: "" };
});
assert.deepEqual(sent[0], "POST /api/shopping/a/bought ");
assert.match(sent[1], /^POST \/api\/shopping \{"text":"Bread","client_id":"[0-9a-f]{32}"\}$/, "the add carries its id for the hub, not the temporary one");
assert.equal(sent[2], "POST /api/shopping/real-1/bought ");
assert.equal(off.offlineState().waiting, 0);
assert.equal(JSON.parse(store.get("zaklon.outbox")!).length, 0);

// A flush that fails midway keeps the rest for later.
await off.queue("POST", "/api/shopping/x/dismiss", null);
await off.queue("POST", "/api/shopping/y/dismiss", null);
let calls = 0;
await assert.rejects(off.flush(async () => {
  calls++;
  if (calls === 2) throw new Error("hub not reachable");
  return { status: 204, body: "" };
}));
assert.equal(off.offlineState().waiting, 1, "the unsent change stays");
off.markOnline();
assert.equal(off.offlineState().since, null);
await off.clearOffline(null);

// Two flushes at the same time send each change once.
store.clear();
await off.queue("POST", "/api/shopping/p/dismiss", null);
let sends = 0;
const slow = async () => {
  sends++;
  await new Promise((r) => setTimeout(r, 20));
  return { status: 204, body: "" };
};
await Promise.all([off.flush(slow), off.flush(slow)]);
assert.equal(sends, 1, "no double send");

// A busy hub (503) keeps the change for later; a refusal (404) drops it.
await off.queue("POST", "/api/shopping/q/dismiss", null);
await assert.rejects(off.flush(async () => ({ status: 503, body: "" })));
assert.equal(off.offlineState().waiting, 1, "kept after 503");
await off.flush(async () => ({ status: 404, body: "" }));
assert.equal(off.offlineState().waiting, 0, "dropped after 404");

// A "bought" for an entry the hub no longer has (404) is dropped, and the
// changes after it are still sent.
store.clear();
await off.queue("POST", "/api/shopping/gone/bought", null);
await off.queue("POST", "/api/shopping/r/dismiss", null);
const seen: string[] = [];
await off.flush(async (_m, path) => {
  seen.push(path);
  return { status: path.includes("gone") ? 404 : 204, body: "" };
});
assert.deepEqual(seen, ["/api/shopping/gone/bought", "/api/shopping/r/dismiss"]);
assert.equal(off.offlineState().waiting, 0, "dropped after 404, the rest sent");

// A 401 (token not accepted) keeps the changes: the phone may be paired
// again, and unlinking sets them aside for the same hub.
await off.queue("POST", "/api/shopping/s/bought", null);
await off.queue("POST", "/api/shopping/t/dismiss", null);
let unauthorized = 0;
await assert.rejects(off.flush(async () => {
  unauthorized++;
  return { status: 401, body: "" };
}));
assert.equal(unauthorized, 1, "stops at the first refusal");
assert.equal(off.offlineState().waiting, 2, "401 keeps the queue");
await off.clearOffline(null);
store.clear();

// Every add carries a client id, also one sent straight to the hub, and a
// queued copy of it keeps the same id (so the hub can tell it is a repeat).
const withId = off.withClientId("POST", "/api/shopping", JSON.stringify({ text: "Salt" }))!;
const cid = JSON.parse(withId).client_id;
assert.match(cid, /^[0-9a-f]{32}$/);
assert.equal(off.withClientId("POST", "/api/shopping", withId), withId, "an id is never replaced");
assert.equal(off.withClientId("POST", "/api/items", JSON.stringify({ name: "x" })), JSON.stringify({ name: "x" }));
assert.equal(off.withClientId("GET", "/api/shopping", undefined), undefined);
const salt = JSON.parse(await off.queue("POST", "/api/shopping", withId));
assert.equal(salt.id, `offline-${cid}`);
let saltBody = "";
await off.flush(async (_m, _p, body) => {
  saltBody = body ?? "";
  return { status: 201, body: JSON.stringify({ id: "real-salt" }) };
});
assert.equal(JSON.parse(saltBody).client_id, cid, "the queued copy is sent with the original id");
// An add queued by an older version (only a temporary id) uses that as its client id.
store.set("zaklon.outbox", JSON.stringify([{ method: "POST", path: "/api/shopping", body: JSON.stringify({ text: "Oil", offline_id: "offline-1-123" }), at: 1 }]));
off.setStore(null);
await off.offlineReady();
let oilBody = "";
await off.flush(async (_m, _p, body) => {
  oilBody = body ?? "";
  return { status: 201, body: "{}" };
});
assert.deepEqual(JSON.parse(oilBody), { text: "Oil", client_id: "offline-1-123" });
store.clear();

// After reconnecting, the hub's list still shows the adds waiting on the
// phone and hides what was bought here, until they are sent.
off.remember("/api/shopping", JSON.stringify(list));
const milk = JSON.parse(await off.queue("POST", "/api/shopping", JSON.stringify({ text: "Eggs" })));
await off.queue("POST", "/api/shopping/a/dismiss", null);
const fresh = JSON.parse(off.withWaiting(JSON.stringify([...list, { id: "b", item_id: null, text: "Rice", quantity: null, unit: null, status: "open", source: "manual" }])));
assert.deepEqual(fresh.map((e: any) => e.id), ["low:m", "b", milk.id]);
assert.equal(off.withWaiting("not json"), "not json");
await off.clearOffline(null);
store.clear();

// A change that cannot be stored on the phone is reported, and not kept.
off.setStore({
  load: async () => null,
  save: async () => {
    throw new Error("could not save on this phone: No space left on device");
  },
});
await off.offlineReady();
await assert.rejects(off.queue("POST", "/api/shopping/a/bought", null), /could not save on this phone/);
assert.equal(off.offlineState().waiting, 0, "the failed change is not counted as waiting");
// The web view's storage is checked by reading it back.
off.setStore(null);
await off.offlineReady();
const realSet = (globalThis as any).localStorage.setItem;
(globalThis as any).localStorage.setItem = () => {};
await assert.rejects(off.queue("POST", "/api/shopping/a/bought", null), /could not save on this phone/);
(globalThis as any).localStorage.setItem = realSet;
assert.equal(off.offlineState().waiting, 0);

// A durable store (the app's files on phones) takes over changes an older
// version kept in the web view's storage.
const files = new Map<string, string>();
const fileStore = {
  load: async (n: string) => files.get(n) ?? null,
  save: async (n: string, d: string) => void files.set(n, d),
};
store.clear();
store.set("zaklon.outbox", JSON.stringify([{ method: "POST", path: "/api/shopping/old/bought", body: null, at: 5 }]));
store.set("zaklon.outbox.parked", JSON.stringify({ hub_id: "hub-0", items: [{ method: "POST", path: "/api/shopping/z/dismiss", body: null, at: 1 }] }));
off.setStore(fileStore as any);
await off.offlineReady();
assert.equal(off.offlineState().waiting, 1, "moved from the web view's storage");
assert.equal(off.parkedFor("hub-0"), 1, "the old single-hub format is read");
assert.equal(JSON.parse(files.get("outbox")!).length, 1);
assert.equal(store.get("zaklon.outbox"), undefined, "the old copy is gone");
await off.discardParked("hub-0");
await off.clearOffline(null);

// Leaving the hub forgets its data, but sets waiting changes aside for it.
off.remember("/api/items", "[]");
await off.queue("POST", "/api/shopping/z/dismiss", null);
assert.equal(await off.clearOffline("hub-1", "Kuća"), 1, "one change set aside");
assert.equal(off.recall("/api/items"), null);
assert.equal(off.offlineState().waiting, 0);
assert.equal(off.parkedFor("hub-1"), 1);
// Pairing with another hub does not send them there by itself...
assert.equal(await off.adoptParked("hub-2"), 0);
assert.equal(off.offlineState().waiting, 0);
// ...but offers them, with the old hub's name.
assert.deepEqual(off.parkedElsewhere("hub-2"), [{ hub_id: "hub-1", hub_name: "Kuća", count: 1 }]);
assert.deepEqual(off.parkedElsewhere("hub-1"), []);
// Pairing with the same hub again: they wait to be sent.
assert.equal(await off.adoptParked("hub-1"), 1);
assert.equal(off.offlineState().waiting, 1);
assert.equal(off.parkedFor("hub-1"), 0);
// Without a known hub nothing can be set aside.
assert.equal(await off.clearOffline(null), 0);
assert.equal(off.offlineState().waiting, 0);

// Changes for two different hubs are both kept: leaving a second hub does not overwrite the first's.
await off.queue("POST", "/api/shopping/one/dismiss", null);
await off.clearOffline("hub-1", "Kuća");
await off.queue("POST", "/api/shopping/two/dismiss", null);
await off.clearOffline("hub-2", "Vikendica");
assert.equal(off.parkedFor("hub-1"), 1);
assert.equal(off.parkedFor("hub-2"), 1);
// Paired with a new hub: send one hub's changes here, discard the other's.
assert.equal(off.parkedElsewhere("hub-3").length, 2);
assert.equal(await off.sendParkedHere("hub-1"), 1);
assert.equal(off.offlineState().waiting, 1);
await off.discardParked("hub-2");
assert.deepEqual(off.parkedElsewhere("hub-3"), []);
assert.equal(JSON.parse(files.get("parked")!).length, 0, "written through");
await off.clearOffline(null);

// Unlinking while a change is on its way: the send stops, and what is set
// aside is exactly what was still waiting.
await off.queue("POST", "/api/shopping/u/dismiss", null);
await off.queue("POST", "/api/shopping/v/dismiss", null);
let release: () => void = () => {};
const inFlight = new Promise<void>((r) => (release = r));
const paths: string[] = [];
const flushing = off.flush(async (_m, path) => {
  paths.push(path);
  await inFlight;
  return { status: 204, body: "" };
});
const settled = off.settle(50);
await settled; // gave up waiting: the first change is still on its way
assert.equal(await off.clearOffline("hub-1", "Kuća"), 2, "both set aside (the first may not have arrived)");
release();
await flushing;
assert.deepEqual(paths, ["/api/shopping/u/dismiss"], "nothing more is sent after unlinking");
assert.equal(off.offlineState().waiting, 0);
assert.equal(off.parkedFor("hub-1"), 2);
await off.discardParked("hub-1");

// Known to be away: the app is asked to look for the hub, but not too often.
events.length = 0;
off.markMissed();
assert.ok(off.knownAway());
off.requestProbe();
off.requestProbe();
assert.equal(events.filter((e) => e === "zaklon-probe").length, 1, "one probe request per few seconds");
// Back online after showing the copy: screens are told to reload.
let reloaded = 0;
const stop = off.onBackOnline(() => reloaded++);
off.remember("/api/items", "[]");
off.fromCache("/api/items");
off.markOnline();
assert.ok(!off.knownAway());
assert.equal(reloaded, 1);
stop();

// Saved conversations: the list and the ones opened last are kept for reading
// away from the hub; a search is not.
assert.ok(off.isCacheable("GET", "/api/conversations"));
assert.ok(off.isCacheable("GET", "/api/conversations/c1"));
assert.ok(!off.isCacheable("GET", "/api/conversations?q=water"));
assert.ok(!off.isCacheable("GET", "/api/conversations/c1/send"));
assert.ok(!off.isQueueable("POST", "/api/assistant/ask"), "questions do not wait for the hub");
// The calculators' household plans are kept to look at away from home; a change is not kept for later.
assert.ok(off.isCacheable("GET", "/api/water") && off.isCacheable("GET", "/api/power"));
assert.ok(!off.isCacheable("PUT", "/api/water") && !off.isQueueable("PUT", "/api/water"));
for (let i = 0; i < 32; i++) off.remember(`/api/conversations/c${i}`, JSON.stringify({ id: `c${i}`, turns: [] }));
off.remember("/api/conversations/c0", JSON.stringify({ id: "c0", turns: [] }));
assert.equal(off.recall("/api/conversations/c1"), null, "the one opened longest ago made room");
assert.equal(off.recall("/api/conversations/c2"), null);
assert.notEqual(off.recall("/api/conversations/c0"), null, "opened again, so kept");
assert.notEqual(off.recall("/api/conversations/c31"), null);
off.forget("/api/conversations/c31");
assert.equal(off.recall("/api/conversations/c31"), null);
console.log("offline: all checks passed");
