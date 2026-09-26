import { useCallback, useEffect, useMemo, useState } from "react";
import { api } from "../api";
import type { Key } from "../i18n";
import { canScan, scan } from "../scan";
import { errText } from "../errors";
import { fmtDateTime, fmtQty, parseNumber } from "../format";
import ConfirmButton from "../components/ConfirmButton";
import ExpiryBadge from "../components/ExpiryBadge";

type T = (k: Key) => string;

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
};
type Place = { id: string; name: string; preset: boolean };
type Shopping = { id: string; item_id: string | null; text: string; quantity: number | null; unit: string | null; done: boolean; source: "manual" | "running_low" };
type History = { seq: number; at: string; actor: string | null; entity: string; entity_id: string; action: string; before: Partial<Item> | null; after: Partial<Item> | null };
type BarcodeReply = { barcode: string; item: Item | null; known: { name: string; unit: string | null; category: string | null } | null };

const CATEGORIES = ["food", "drink", "medicine", "hygiene", "equipment", "fuel", "other"] as const;
const UNITS = ["pcs", "kg", "g", "l", "ml", "pack"] as const;

const catKey = (c: string) => ("cat_" + c) as Key;
const unitKey = (u: string) => ("unit_" + u) as Key;
const placeKey = (p: string) => ("place_" + p) as Key;

export default function Supplies({ t }: { t: T }) {
  const [view, setView] = useState<"items" | "shopping" | "history">("items");
  const [items, setItems] = useState<Item[] | null>(null);
  const [places, setPlaces] = useState<Place[]>([]);
  const [err, setErr] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const [cat, setCat] = useState<string>("all");
  const [editing, setEditing] = useState<Partial<Item> | null>(null);
  const [scanner, setScanner] = useState(false);

  const load = useCallback(async () => {
    try {
      const [i, p] = await Promise.all([api<Item[]>("/api/items"), api<Place[]>("/api/places")]);
      setItems(i);
      setPlaces(p);
      setErr(null);
    } catch (e) {
      setErr(errText(t, e));
    }
  }, [t]);

  useEffect(() => {
    load();
    canScan().then(setScanner).catch(() => setScanner(false));
  }, [load]);

  const placeName = (p: string | null) => {
    if (!p) return "";
    const found = places.find((x) => x.id === p || x.name === p);
    if (found?.preset) return t(placeKey(found.id));
    return found?.name ?? p;
  };

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
      setErr(errText(t, e));
    }
  };

  const scanAndOpen = async () => {
    const code = await scan("product");
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
        onDone={() => {
          setEditing(null);
          load();
        }}
      />
    );
  }

  return (
    <div className="stack">
      <div className="page-head">
        <h1>{t("supplies")}</h1>
      </div>
      <div className="segmented">
        {(["items", "shopping", "history"] as const).map((v) => (
          <button key={v} className={view === v ? "active" : ""} onClick={() => setView(v)}>
            {t(v === "items" ? "suppliesItems" : v === "shopping" ? "shoppingList" : "history")}
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
              <button key={c} className={"chip" + (cat === c ? " active" : "")} onClick={() => setCat(c)}>
                {c === "all" ? t("all") : t(catKey(c))}
              </button>
            ))}
          </div>
          {items === null ? (
            <p className="muted">…</p>
          ) : shown.length === 0 ? (
            <p className="muted" style={{ textAlign: "center" }}>{items.length === 0 ? t("noItemsYet") : t("noResults")}</p>
          ) : (
            <div className="list">
              {shown.map((i) => {
                const low = i.min_quantity !== null && i.quantity < i.min_quantity;
                return (
                  <div className="item supply" key={i.id}>
                    <button className="supply-main" onClick={() => setEditing(i)}>
                      <div className="supply-name">{i.name}</div>
                      <div className="muted supply-meta">
                        {t(catKey(i.category))}
                        {i.place ? ` · ${placeName(i.place)}` : ""}
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
                        <div className="muted" style={{ fontSize: 12 }}>{UNITS.includes(i.unit as (typeof UNITS)[number]) ? t(unitKey(i.unit)) : i.unit}</div>
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

      {view === "shopping" && <ShoppingView t={t} />}
      {view === "history" && <HistoryView t={t} />}
    </div>
  );
}

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
  onDone: () => void;
}) {
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
  const [newPlace, setNewPlace] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const set = (k: keyof typeof f) => (e: React.ChangeEvent<HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement>) => {
    const value = e.target.value;
    setF((prev) => ({ ...prev, [k]: value }));
  };
  const num = parseNumber;

  const save = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setErr(null);
    const body = {
      name: f.name,
      quantity: num(f.quantity) ?? 0,
      unit: f.unit,
      category: f.category,
      place: f.place || null,
      expiry: f.expiry || null,
      min_quantity: f.min_quantity.trim() ? num(f.min_quantity) : null,
      barcode: f.barcode || null,
      notes: f.notes || null,
    };
    try {
      if (initial.id) await api(`/api/items/${initial.id}`, { method: "PATCH", json: body });
      else await api("/api/items", { json: body });
      onDone();
    } catch (ex) {
      setErr(errText(t, ex));
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
      setErr(errText(t, ex));
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
    const code = await scan("product");
    if (code) setF((prev) => ({ ...prev, barcode: code }));
  };

  return (
    <form className="stack form" onSubmit={save}>
      <div className="page-head">
        <h1>{initial.id ? t("editItem") : t("addItem")}</h1>
      </div>
      <label className="field">
        {t("itemName")}
        <input type="text" value={f.name} onChange={set("name")} required maxLength={120} autoFocus={!initial.id} />
      </label>
      <div className="two">
        <label className="field">
          {t("quantity")}
          <input type="text" inputMode="decimal" value={f.quantity} onChange={set("quantity")} />
        </label>
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
          </select>
        </label>
      </div>
      <div className="row">
        <input type="text" value={newPlace} onChange={(e) => setNewPlace(e.target.value)} placeholder={t("newPlace")} aria-label={t("newPlace")} maxLength={40} />
        <button type="button" className="btn secondary" onClick={addPlace} disabled={!newPlace.trim()} aria-label={t("addPlace")}>+</button>
      </div>
      <div className="two">
        <label className="field">
          {t("expiry")}
          <input type="date" value={f.expiry} onChange={set("expiry")} min="2000-01-01" max="2100-12-31" />
        </label>
        <label className="field">
          {t("minQuantity")}
          <input type="text" inputMode="decimal" value={f.min_quantity} onChange={set("min_quantity")} placeholder={t("optional")} />
        </label>
      </div>
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
      {initial.id && <p className="muted" style={{ fontSize: 13 }}>{t("confirmDelete")}</p>}
      {err && <p className="error" role="alert">{err}</p>}
      <div className="row actions">
        <button className="btn" disabled={busy || !f.name.trim()}>{t("save")}</button>
        <button type="button" className="btn secondary" onClick={onDone}>{t("cancel")}</button>
        {initial.id && <ConfirmButton label={t("delete")} confirmLabel={t("yesDelete")} cancelLabel={t("cancel")} onConfirm={remove} />}
      </div>
    </form>
  );
}

function ShoppingView({ t }: { t: T }) {
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

  const toggle = async (s: Shopping) => {
    try {
      if (s.source === "running_low") {
        // Tick a computed entry: keep it on the list as a real, done entry.
        const e = await api<Shopping>("/api/shopping", { json: { text: s.text, quantity: s.quantity, unit: s.unit, item_id: s.item_id } });
        await api(`/api/shopping/${e.id}`, { method: "PATCH", json: { done: true } });
      } else {
        await api(`/api/shopping/${s.id}`, { method: "PATCH", json: { done: !s.done } });
      }
      load();
    } catch (ex) {
      setErr(errText(t, ex));
    }
  };

  const clearDone = async () => {
    await api("/api/shopping/clear-done", { method: "POST" }).catch(() => {});
    load();
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
          <label className={"item check" + (s.done ? " done" : "")} key={s.id}>
            <input type="checkbox" checked={s.done} onChange={() => toggle(s)} />
            <span className="check-text">
              {s.text}
              {s.quantity !== null && <span className="muted"> · {fmtQty(s.quantity)} {s.unit ? t(unitKey(s.unit)) : ""}</span>}
            </span>
            {s.source === "running_low" && <span className="badge warn">{t("runningLow")}</span>}
          </label>
        ))}
      </div>
      {list?.some((s) => s.done) && (
        <div>
          <button className="btn secondary" onClick={clearDone}>{t("clearDone")}</button>
        </div>
      )}
    </div>
  );
}

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
