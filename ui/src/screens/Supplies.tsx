import { countWord } from "../format";
import { useCallback, useEffect, useMemo, useState } from "react";
import { api } from "../api";
import { onBackOnline, onOfflineChange } from "../offline";
import type { Key } from "../i18n";
import { canScan, scan } from "../scan";
import { errCode, errText } from "../errors";
import { fmtDateTime, fmtQty, parseNumber, unitLabel } from "../format";
import ConfirmButton from "../components/ConfirmButton";
import ExpiryBadge from "../components/ExpiryBadge";
import HelpLink from "../components/HelpLink";

type T = (k: Key) => string;

export type Batch = { id: string; quantity: number; expiry: string | null; added_at: string };
export type Item = {
  id: string;
  name: string;
  quantity: number;
  unit: string;
  category: string;
  place: string | null;
  expiry: string | null;
  barcode: string | null;
  min_quantity: number | null;
  notes: string | null;
  updated_at: string;
  updated_by: string | null;
  batches?: Batch[];
};
type Place = { id: string; name: string; preset: boolean };
type Shopping = {
  id: string;
  item_id: string | null;
  text: string;
  quantity: number | null;
  unit: string | null;
  status: "open" | "bought";
  source: "manual" | "running_low";
};
type History = { seq: number; at: string; actor: string | null; entity: string; entity_id: string; action: string; before: Partial<Item> | null; after: Partial<Item> | null };
type BarcodeReply = { barcode: string; item: Item | null; known: { name: string; unit: string | null; category: string | null } | null };
type View = "items" | "shopping" | "putaway" | "history";

const CATEGORIES = ["food", "drink", "medicine", "hygiene", "equipment", "fuel", "other"] as const;
const UNITS = ["pcs", "kg", "g", "l", "ml", "pack"] as const;

const catKey = (c: string) => ("cat_" + c) as Key;
const unitKey = (u: string) => ("unit_" + u) as Key;
const placeKey = (p: string) => ("place_" + p) as Key;

function unitName(t: T, unit: string, qty: number) {
  return unitLabel(unit, qty, (u) => (UNITS.includes(u as (typeof UNITS)[number]) ? t(unitKey(u)) : u));
}

export default function Supplies({ t }: { t: T }) {
  const [view, setView] = useState<View>("items");
  const [items, setItems] = useState<Item[] | null>(null);
  const [places, setPlaces] = useState<Place[]>([]);
  const [awayCount, setAwayCount] = useState(0);
  const [err, setErr] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const [cat, setCat] = useState<string>("all");
  const [editing, setEditing] = useState<Partial<Item> | null>(null);
  const [scanner, setScanner] = useState(false);

  const load = useCallback(async () => {
    try {
      const [i, p, a] = await Promise.all([api<Item[]>("/api/items"), api<Place[]>("/api/places"), api<Shopping[]>("/api/put-away")]);
      setItems(i);
      setPlaces(p);
      setAwayCount(a.length);
      setErr(null);
    } catch (e) {
      setErr(errText(t, e));
    }
  }, [t]);

  useEffect(() => {
    load();
    canScan().then(setScanner).catch(() => setScanner(false));
  }, [load]);
  // Back in reach of the hub after showing the phone's copy: load the fresh data.
  useEffect(() => onBackOnline(() => void load()), [load]);

  const placeName = useCallback(
    (p: string | null) => {
      if (!p) return "";
      const found = places.find((x) => x.id === p || x.name === p);
      if (found?.preset) return t(placeKey(found.id));
      return found?.name ?? p;
    },
    [places, t],
  );

  const shown = useMemo(() => {
    const needle = q.trim().toLowerCase();
    return (items ?? []).filter(
      (i) => (cat === "all" || i.category === cat) && (!needle || i.name.toLowerCase().includes(needle) || (i.barcode ?? "").includes(needle)),
    );
  }, [items, q, cat]);

  const adjust = async (i: Item, delta: number) => {
    try {
      const updated = await api<Item>(`/api/items/${i.id}/adjust`, { json: { delta } });
      setItems((all) => (all ?? []).map((x) => (x.id === updated.id ? updated : x)));
    } catch (e) {
      // Deleted on another device meanwhile: show the list as it is now.
      if (errCode(e) === "not_found") await load();
      setErr(errText(t, e));
    }
  };

  const scanAndOpen = async () => {
    const code = await scan("product", t);
    if (!code) return;
    try {
      const r = await api<BarcodeReply>(`/api/barcodes/${encodeURIComponent(code)}`);
      if (r.item) setEditing(r.item);
      else if (r.known) setEditing({ name: r.known.name, unit: r.known.unit ?? "pcs", category: r.known.category ?? "food", barcode: code, quantity: 1 });
      else setEditing({ barcode: code, quantity: 1, unit: "pcs", category: "food" });
    } catch (e) {
      setErr(errText(t, e));
    }
  };

  if (editing) {
    return (
      <ItemForm
        t={t}
        initial={editing}
        places={places}
        placeName={placeName}
        scanner={scanner}
        onPlacesChanged={load}
        onDone={async (message) => {
          setEditing(null);
          await load();
          if (message) setErr(message);
        }}
      />
    );
  }

  const tabs: [View, string][] = [
    // Two rows on phones: what we have, then what we buy.
    ["items", t("suppliesItems")],
    ["history", t("history")],
    ["shopping", t("shoppingList")],
    ["putaway", awayCount > 0 ? `${t("putAway")} (${awayCount})` : t("putAway")],
  ];

  return (
    <div className="stack">
      <div className="page-head">
        <div className="title-line">
          <h1>{t("supplies")}</h1>
          <HelpLink t={t} topic="supplies" />
        </div>
      </div>
      <div className="segmented tabs-4">
        {tabs.map(([v, label]) => (
          <button key={v} className={view === v ? "active" : ""} aria-pressed={view === v} onClick={() => setView(v)}>
            {label}
          </button>
        ))}
      </div>
      {err && <p className="error" role="alert">{err}</p>}

      {view === "items" && (
        <>
          <div className="row actions">
            <button className="btn" onClick={() => setEditing({ quantity: 1, unit: "pcs", category: "food" })}>{t("addItem")}</button>
            {scanner && <button className="btn secondary" onClick={scanAndOpen}>{t("scanBarcode")}</button>}
          </div>
          <input type="search" className="search" value={q} onChange={(e) => setQ(e.target.value)} placeholder={t("searchSupplies")} aria-label={t("searchSupplies")} />
          <div className="chips">
            {["all", ...CATEGORIES].map((c) => (
              <button key={c} className={"chip" + (cat === c ? " active" : "")} aria-pressed={cat === c} onClick={() => setCat(c)}>
                {c === "all" ? t("all") : t(catKey(c))}
              </button>
            ))}
          </div>
          {items === null ? (
            !err && <p className="muted">{t("aiLoading")}</p>
          ) : shown.length === 0 ? (
            <p className="muted" style={{ textAlign: "center" }}>{items.length === 0 ? t("noItemsYet") : t("noResults")}</p>
          ) : (
            <div className="list cols">
              {shown.map((i) => {
                const low = i.min_quantity !== null && i.quantity < i.min_quantity;
                const batches = i.batches?.length ?? 0;
                return (
                  <div className="item supply" key={i.id}>
                    <button className="supply-main" onClick={() => setEditing(i)}>
                      <div className="supply-name">{i.name}</div>
                      <div className="muted supply-meta">
                        {t(catKey(i.category))}
                        {i.place ? ` · ${placeName(i.place)}` : ""}
                        {batches > 1 ? ` · ${batches} ${countWord(batches, ["batch", "batches"], ["serija", "serije", "serija"])}` : ""}
                      </div>
                      <div className="supply-badges">
                        <ExpiryBadge date={i.expiry} t={t} />
                        {low && <span className="badge warn">{t("runningLow")}</span>}
                      </div>
                    </button>
                    <div className="qty">
                      <button className="qty-btn" aria-label={`${t("useOne")}: ${i.name}`} onClick={() => adjust(i, -1)} disabled={i.quantity <= 0}>−</button>
                      <div className="qty-val">
                        <div>{fmtQty(i.quantity)}</div>
                        <div className="muted" style={{ fontSize: 12 }}>{unitName(t, i.unit, i.quantity)}</div>
                      </div>
                      <button className="qty-btn" aria-label={`${t("addOne")}: ${i.name}`} onClick={() => adjust(i, 1)}>+</button>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </>
      )}

      {view === "shopping" && <ShoppingView t={t} onChanged={load} />}
      {view === "putaway" && <PutAwayView t={t} items={items ?? []} places={places} placeName={placeName} onChanged={load} />}
      {view === "history" && <HistoryView t={t} />}
    </div>
  );
}

// ---- add / edit an item ------------------------------------------------------

function ItemForm({
  t,
  initial,
  places,
  placeName,
  scanner,
  onPlacesChanged,
  onDone,
}: {
  t: T;
  initial: Partial<Item>;
  places: Place[];
  placeName: (p: string | null) => string;
  scanner: boolean;
  onPlacesChanged: () => void;
  /** Back to the list; with a message when the item was gone meanwhile. */
  onDone: (message?: string) => void;
}) {
  const isNew = !initial.id;
  const [f, setF] = useState({
    name: initial.name ?? "",
    quantity: initial.quantity !== undefined ? String(initial.quantity) : "1",
    unit: initial.unit ?? "pcs",
    category: initial.category ?? "food",
    place: initial.place ?? "",
    expiry: initial.expiry ?? "",
    min_quantity: initial.min_quantity !== undefined && initial.min_quantity !== null ? String(initial.min_quantity) : "",
    barcode: initial.barcode ?? "",
    notes: initial.notes ?? "",
  });
  const [batches, setBatches] = useState<Batch[]>(initial.batches ?? []);
  const [newPlace, setNewPlace] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const set = (k: keyof typeof f) => (e: React.ChangeEvent<HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement>) => {
    const value = e.target.value;
    setF((prev) => ({ ...prev, [k]: value }));
  };

  const save = async (e: React.FormEvent) => {
    e.preventDefault();
    const quantity = parseNumber(f.quantity) ?? 0;
    if (isNew && quantity < 0) {
      setErr(t("errBadQuantity"));
      return;
    }
    setBusy(true);
    setErr(null);
    const common = {
      name: f.name,
      unit: f.unit,
      category: f.category,
      place: f.place || null,
      min_quantity: f.min_quantity.trim() ? parseNumber(f.min_quantity) : null,
      barcode: f.barcode || null,
      notes: f.notes || null,
    };
    try {
      if (isNew) await api("/api/items", { json: { ...common, quantity, expiry: f.expiry || null } });
      else await api(`/api/items/${initial.id}`, { method: "PATCH", json: common });
      onDone();
    } catch (ex) {
      if (errCode(ex) === "not_found") onDone(errText(t, ex));
      else setErr(errText(t, ex));
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    if (!initial.id) return;
    try {
      await api(`/api/items/${initial.id}`, { method: "DELETE" });
      onDone();
    } catch (ex) {
      if (errCode(ex) === "not_found") onDone(errText(t, ex));
      else setErr(errText(t, ex));
    }
  };

  const addPlace = async () => {
    if (!newPlace.trim()) return;
    try {
      const p = await api<Place>("/api/places", { json: { name: newPlace } });
      onPlacesChanged();
      setF((prev) => ({ ...prev, place: p.id }));
      setNewPlace("");
    } catch (ex) {
      setErr(errText(t, ex));
    }
  };

  const scanCode = async () => {
    const code = await scan("product", t);
    if (code) setF((prev) => ({ ...prev, barcode: code }));
  };

  return (
    <form className="stack form" onSubmit={save}>
      <div className="page-head">
        <h1>{isNew ? t("addItem") : t("editItem")}</h1>
      </div>
      <label className="field">
        {t("itemName")}
        <input type="text" value={f.name} onChange={set("name")} required maxLength={120} autoFocus={isNew} />
      </label>
      <div className="two">
        {isNew && (
          <label className="field">
            {t("quantity")}
            <input type="text" inputMode="decimal" value={f.quantity} onChange={set("quantity")} />
          </label>
        )}
        <label className="field">
          {t("unit")}
          <select value={f.unit} onChange={set("unit")}>
            {UNITS.map((u) => (
              <option key={u} value={u}>{t(unitKey(u))}</option>
            ))}
          </select>
        </label>
      </div>
      <div className="two">
        <label className="field">
          {t("category")}
          <select value={f.category} onChange={set("category")}>
            {CATEGORIES.map((c) => (
              <option key={c} value={c}>{t(catKey(c))}</option>
            ))}
          </select>
        </label>
        <label className="field">
          {t("place")}
          <select value={f.place} onChange={set("place")}>
            <option value="">—</option>
            {places.map((p) => (
              <option key={p.id} value={p.id}>{placeName(p.id)}</option>
            ))}
            {/* A place written some other way (e.g. by the assistant) still shows. */}
            {f.place && !places.some((p) => p.id === f.place) && <option value={f.place}>{placeName(f.place)}</option>}
          </select>
        </label>
      </div>
      <div className="row">
        <input type="text" value={newPlace} onChange={(e) => setNewPlace(e.target.value)} placeholder={t("newPlace")} aria-label={t("newPlace")} maxLength={40} />
        <button type="button" className="btn secondary" onClick={addPlace} disabled={!newPlace.trim()} aria-label={t("addPlace")}>+</button>
      </div>
      <div className="two">
        {isNew && (
          <label className="field">
            {t("expiry")}
            <input type="date" value={f.expiry} onChange={set("expiry")} min="2000-01-01" max="2100-12-31" />
          </label>
        )}
        <label className="field">
          {t("minQuantity")}
          <input type="text" inputMode="decimal" value={f.min_quantity} onChange={set("min_quantity")} placeholder={t("optional")} />
        </label>
      </div>

      {!isNew && initial.id && (
        <BatchesEditor t={t} itemId={initial.id} unit={f.unit} batches={batches} onChange={setBatches} onError={setErr} />
      )}

      <label className="field">
        {t("barcode")}
        <div className="row">
          <input type="text" inputMode="numeric" value={f.barcode} onChange={set("barcode")} placeholder={t("optional")} />
          {scanner && <button type="button" className="btn secondary" onClick={scanCode}>{t("scan")}</button>}
        </div>
      </label>
      <label className="field">
        {t("notes")}
        <textarea value={f.notes} onChange={set("notes")} rows={2} maxLength={500} />
      </label>
      {!isNew && <p className="muted" style={{ fontSize: 13 }}>{t("confirmDelete")}</p>}
      {err && <p className="error" role="alert">{err}</p>}
      <div className="row actions">
        <button className="btn" disabled={busy || !f.name.trim()}>{t("save")}</button>
        <button type="button" className="btn secondary" onClick={() => onDone()}>{t("cancel")}</button>
        {!isNew && <ConfirmButton label={t("delete")} confirmLabel={t("yesDelete")} cancelLabel={t("keepIt")} onConfirm={remove} />}
      </div>
    </form>
  );
}

/** Batches of an item: each with its own quantity and expiry date. Changes are saved right away. */
function BatchesEditor({
  t,
  itemId,
  unit,
  batches,
  onChange,
  onError,
}: {
  t: T;
  itemId: string;
  unit: string;
  batches: Batch[];
  onChange: (b: Batch[]) => void;
  onError: (e: string | null) => void;
}) {
  const [addQty, setAddQty] = useState("");
  const [addDate, setAddDate] = useState("");
  const total = batches.reduce((s, b) => s + b.quantity, 0);

  const apply = async (p: Promise<Item>) => {
    try {
      const item = await p;
      onChange(item.batches ?? []);
      onError(null);
    } catch (e) {
      onError(errText(t, e));
    }
  };

  const update = (b: Batch, field: "quantity" | "expiry", input: HTMLInputElement) => {
    const value = input.value;
    if (field === "quantity") {
      const q = parseNumber(value);
      if (q === b.quantity) return;
      // Emptying a batch is what "×" is for (it asks first); put the number back.
      if (q === null || q <= 0) {
        input.value = fmtQty(b.quantity);
        return;
      }
      apply(api<Item>(`/api/batches/${b.id}`, { method: "PATCH", json: { quantity: q } }));
    } else {
      if ((value || null) === b.expiry) return;
      apply(api<Item>(`/api/batches/${b.id}`, { method: "PATCH", json: { expiry: value || null } }));
    }
  };

  const add = () => {
    const q = parseNumber(addQty);
    if (q === null || q <= 0) return;
    apply(api<Item>(`/api/items/${itemId}/batches`, { json: { quantity: q, expiry: addDate || null } }));
    setAddQty("");
    setAddDate("");
  };

  return (
    <div className="panel stack left batches">
      <div className="row between">
        <div className="label">{t("batches")}</div>
        <div className="muted" style={{ fontSize: 14 }}>
          {t("total")}: {fmtQty(total)} {unitName(t, unit, total)}
        </div>
      </div>
      {batches.length === 0 && <p className="muted">{t("noBatches")}</p>}
      {batches.map((b) => (
        <div className="batch-row" key={b.id + b.quantity + (b.expiry ?? "")}>
          <input
            type="text"
            inputMode="decimal"
            defaultValue={fmtQty(b.quantity)}
            aria-label={t("quantity")}
            onBlur={(e) => update(b, "quantity", e.target)}
          />
          <input type="date" defaultValue={b.expiry ?? ""} aria-label={t("expiry")} onBlur={(e) => update(b, "expiry", e.target)} />
          <ConfirmButton
            label="×"
            ariaLabel={t("removeBatch")}
            confirmLabel={t("yesRemove")}
            cancelLabel={t("cancel")}
            onConfirm={() => apply(api<Item>(`/api/batches/${b.id}`, { method: "DELETE" }))}
            className="btn danger small"
          />
        </div>
      ))}
      <div className="batch-row">
        <input type="text" inputMode="decimal" value={addQty} onChange={(e) => setAddQty(e.target.value)} placeholder={t("quantity")} aria-label={t("newBatchQuantity")} />
        <input type="date" value={addDate} onChange={(e) => setAddDate(e.target.value)} aria-label={t("newBatchExpiry")} />
        <button type="button" className="btn secondary small" onClick={add} disabled={!parseNumber(addQty)}>{t("addBatch")}</button>
      </div>
    </div>
  );
}

// ---- shopping list -------------------------------------------------------------

function ShoppingView({ t, onChanged }: { t: T; onChanged: () => void }) {
  const [list, setList] = useState<Shopping[] | null>(null);
  const [text, setText] = useState("");
  const [err, setErr] = useState<string | null>(null);

  const load = useCallback(
    () =>
      api<Shopping[]>("/api/shopping")
        .then((l) => {
          setList(l);
          setErr(null);
        })
        .catch((e) => setErr(errText(t, e))),
    [t],
  );
  useEffect(() => {
    load();
  }, [load]);
  // Changes waiting on the phone were sent (or set aside), or the hub is back:
  // show the list as it is now.
  useEffect(() => onOfflineChange(() => void load()), [load]);

  const add = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!text.trim()) return;
    try {
      await api("/api/shopping", { json: { text } });
      setText("");
      load();
    } catch (ex) {
      setErr(errText(t, ex));
    }
  };

  const act = async (s: Shopping, what: "bought" | "dismiss") => {
    try {
      await api(`/api/shopping/${encodeURIComponent(s.id)}/${what}`, { method: "POST" });
      load();
      onChanged();
    } catch (ex) {
      if (errCode(ex) === "not_found") await load();
      setErr(errText(t, ex));
    }
  };

  return (
    <div className="stack">
      <form className="row" onSubmit={add}>
        <input type="text" value={text} onChange={(e) => setText(e.target.value)} placeholder={t("addToList")} aria-label={t("addToList")} maxLength={120} />
        <button className="btn" disabled={!text.trim()} aria-label={t("addToList")}>+</button>
      </form>
      {err && <p className="error" role="alert">{err}</p>}
      {list && list.length === 0 && <p className="muted" style={{ textAlign: "center" }}>{t("listEmpty")}</p>}
      <div className="list">
        {list?.map((s) => (
          <div className="item wrap shop" key={s.id}>
            <div className="shop-text">
              <div>{s.text}</div>
              <div className="muted" style={{ fontSize: 13 }}>
                {s.quantity !== null && s.quantity > 0 && <span>{fmtQty(s.quantity)} {s.unit ? unitName(t, s.unit, s.quantity) : ""}</span>}
                {s.source === "running_low" && <span className="badge warn" style={{ marginLeft: 6 }}>{t("runningLow")}</span>}
              </div>
            </div>
            <div className="row">
              <button className="btn small" onClick={() => act(s, "bought")} aria-label={`${t("bought")}: ${s.text}`}>{t("bought")}</button>
              <button className="btn secondary small" onClick={() => act(s, "dismiss")} aria-label={`${t("delete")}: ${s.text}`}>{t("delete")}</button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

// ---- put away bought things ------------------------------------------------------

function PutAwayView({
  t,
  items,
  places,
  placeName,
  onChanged,
}: {
  t: T;
  items: Item[];
  places: Place[];
  placeName: (p: string | null) => string;
  onChanged: () => void;
}) {
  const [list, setList] = useState<Shopping[] | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const load = useCallback(
    () =>
      api<Shopping[]>("/api/put-away")
        .then((l) => {
          setList(l);
          setErr(null);
        })
        .catch((e) => setErr(errText(t, e))),
    [t],
  );
  useEffect(() => {
    load();
  }, [load]);

  const done = async (message?: string) => {
    await load();
    onChanged();
    if (message) setErr(message);
  };

  return (
    <div className="stack">
      <p className="muted intro">{t("putAwayIntro")}</p>
      {err && <p className="error" role="alert">{err}</p>}
      {list && list.length === 0 && <p className="muted" style={{ textAlign: "center" }}>{t("nothingToPutAway")}</p>}
      {list?.map((s) => (
        <PutAwayCard
          key={s.id}
          t={t}
          entry={s}
          item={items.find((i) => i.id === s.item_id) ?? null}
          places={places}
          placeName={placeName}
          onDone={done}
          onError={setErr}
        />
      ))}
    </div>
  );
}

function PutAwayCard({
  t,
  entry,
  item,
  places,
  placeName,
  onDone,
  onError,
}: {
  t: T;
  entry: Shopping;
  item: Item | null;
  places: Place[];
  placeName: (p: string | null) => string;
  /** Refresh the list; with a message when the entry was gone meanwhile. */
  onDone: (message?: string) => void;
  onError: (e: string | null) => void;
}) {
  const [qty, setQty] = useState(entry.quantity && entry.quantity > 0 ? fmtQty(entry.quantity) : "1");
  const [expiry, setExpiry] = useState("");
  const [place, setPlace] = useState(item?.place ?? "");
  const [category, setCategory] = useState(item?.category ?? "food");
  const [unit, setUnit] = useState(item?.unit ?? entry.unit ?? "pcs");
  const [busy, setBusy] = useState(false);

  const putAway = async () => {
    const q = parseNumber(qty);
    if (q === null || q <= 0) return;
    setBusy(true);
    try {
      await api(`/api/put-away/${entry.id}`, {
        json: { quantity: q, expiry: expiry || null, place: place || null, category, unit, item_id: item?.id ?? null },
      });
      onDone();
    } catch (e) {
      if (errCode(e) === "not_found") onDone(errText(t, e));
      else onError(errText(t, e));
    } finally {
      setBusy(false);
    }
  };

  const discard = async () => {
    try {
      await api(`/api/shopping/${entry.id}/dismiss`, { method: "POST" });
      onDone();
    } catch (e) {
      if (errCode(e) === "not_found") onDone(errText(t, e));
      else onError(errText(t, e));
    }
  };

  return (
    <div className="panel stack left put-away" aria-label={entry.text}>
      <div className="row between wrap">
        <strong>{entry.text}</strong>
        <span className="muted" style={{ fontSize: 13 }}>{item ? t("addsToStock") : t("newItem")}</span>
      </div>
      <div className="two">
        <label className="field">
          {t("quantity")}
          <input type="text" inputMode="decimal" value={qty} onChange={(e) => setQty(e.target.value)} />
        </label>
        {item ? (
          <div className="field">
            {t("unit")}
            <div className="static-value">{unitName(t, unit, parseNumber(qty) ?? 1)}</div>
          </div>
        ) : (
          <label className="field">
            {t("unit")}
            <select value={unit} onChange={(e) => setUnit(e.target.value)}>
              {UNITS.map((u) => (
                <option key={u} value={u}>{t(unitKey(u))}</option>
              ))}
            </select>
          </label>
        )}
      </div>
      <div className="two">
        <label className="field">
          {t("expiry")}
          <input type="date" value={expiry} onChange={(e) => setExpiry(e.target.value)} />
        </label>
        <label className="field">
          {t("place")}
          <select value={place} onChange={(e) => setPlace(e.target.value)}>
            <option value="">—</option>
            {places.map((p) => (
              <option key={p.id} value={p.id}>{placeName(p.id)}</option>
            ))}
          </select>
        </label>
      </div>
      {!item && (
        <label className="field">
          {t("category")}
          <select value={category} onChange={(e) => setCategory(e.target.value)}>
            {CATEGORIES.map((c) => (
              <option key={c} value={c}>{t(catKey(c))}</option>
            ))}
          </select>
        </label>
      )}
      <div className="row actions">
        <button className="btn" onClick={putAway} disabled={busy || !parseNumber(qty)}>{t("putAwayNow")}</button>
        <ConfirmButton label={t("remove")} confirmLabel={t("yesRemove")} cancelLabel={t("cancel")} onConfirm={discard} className="btn secondary" />
      </div>
    </div>
  );
}

// ---- history ---------------------------------------------------------------------

function HistoryView({ t }: { t: T }) {
  const [h, setH] = useState<History[] | null>(null);
  useEffect(() => {
    api<History[]>("/api/history?limit=100")
      .then(setH)
      .catch(() => setH(null));
  }, []);
  const actionKey = (a: string) => ("act_" + a) as Key;
  if (h === null) return <p className="muted">{t("loadingOrUnavailable")}</p>;
  if (h.length === 0) return <p className="muted" style={{ textAlign: "center" }}>{t("nothingYet")}</p>;
  return (
    <div className="list">
      {h.map((e) => {
        const name = e.after?.name ?? e.before?.name ?? "";
        const bq = e.before?.quantity;
        const aq = e.after?.quantity;
        const change = bq !== undefined && aq !== undefined && bq !== aq ? `${fmtQty(bq)} → ${fmtQty(aq)}` : "";
        return (
          <div className="item" key={e.seq} style={{ flexDirection: "column", alignItems: "stretch", gap: 2 }}>
            <div>
              <strong>{name}</strong> · {t(actionKey(e.action))} {change && <span className="muted">({change})</span>}
            </div>
            <div className="muted" style={{ fontSize: 13 }}>
              {fmtDateTime(e.at)} · {e.actor === "laptop" ? t("theLaptop") : e.actor}
            </div>
          </div>
        );
      })}
    </div>
  );
}
