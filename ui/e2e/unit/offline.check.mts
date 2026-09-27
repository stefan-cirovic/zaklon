// Run: node e2e/unit/offline.check.mts  (Node 23.6+ strips the types)
import assert from "node:assert/strict";

const store = new Map<string, string>();
(globalThis as any).localStorage = {
  getItem: (k: string) => store.get(k) ?? null,
  setItem: (k: string, v: string) => void store.set(k, v),
  removeItem: (k: string) => void store.delete(k),
};
(globalThis as any).window = { dispatchEvent: () => true, addEventListener() {}, removeEventListener() {} };
(globalThis as any).CustomEvent = class { type: string; constructor(type: string) { this.type = type; } };

const off = await import("../../src/offline.ts");

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
off.queue("POST", "/api/shopping/a/bought", null);
const added = JSON.parse(off.queue("POST", "/api/shopping", JSON.stringify({ text: "Bread" })));
assert.match(added.id, /^offline-/);
off.queue("POST", `/api/shopping/${encodeURIComponent(added.id)}/bought`, null);
const now = JSON.parse(off.fromCache("/api/shopping")!);
assert.deepEqual(now.map((e: any) => e.id), ["low:m"], "bought and added-then-bought are off the list");
assert.equal(off.offlineState().waiting, 3);

// Back home: the outbox is sent in order, the new entry's real id is used.
const sent: string[] = [];
await off.flush(async (method, path, body) => {
  sent.push(`${method} ${path} ${body ?? ""}`);
  if (path === "/api/shopping") return { status: 201, body: JSON.stringify({ id: "real-1" }) };
  return { status: 204, body: "" };
});
assert.deepEqual(sent, ["POST /api/shopping/a/bought ", 'POST /api/shopping {"text":"Bread"}', "POST /api/shopping/real-1/bought "]);
assert.equal(off.offlineState().waiting, 0);

// A flush that fails midway keeps the rest for later.
off.queue("POST", "/api/shopping/x/dismiss", null);
off.queue("POST", "/api/shopping/y/dismiss", null);
let calls = 0;
await assert.rejects(off.flush(async () => {
  calls++;
  if (calls === 2) throw new Error("hub not reachable");
  return { status: 204, body: "" };
}));
assert.equal(off.offlineState().waiting, 1, "the unsent change stays");
off.markOnline();
assert.equal(off.offlineState().since, null);
console.log("offline: all checks passed");
