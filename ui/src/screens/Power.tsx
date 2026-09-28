import { useCallback, useEffect, useId, useMemo, useRef, useState, type ReactNode } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, inTauri } from "../api";
import type { Key, Lang } from "../i18n";
import { errText } from "../errors";
import { countWord, fmtDateTime, fmtNum, parseNumber } from "../format";
import { useVisiblePoll } from "../poll";
import ConfirmButton from "../components/ConfirmButton";
import HelpLink from "../components/HelpLink";
import { Icon } from "../components/Icon";
import {
  APPLIANCES,
  applianceOf,
  calculate,
  CHARGE_EFF,
  DAY_CHOICES,
  defaultPlan,
  INVERTER_HEADROOM,
  lineFor,
  linkQuery,
  MAX_LINES,
  normalizePlan,
  PANEL_DERATE,
  PANEL_STEP,
  panelsByMonth,
  planFromLink,
  regionOf,
  SOURCES,
  SUN_REGIONS,
  sunOf,
  typicalLines,
  USABLE,
  VOLTS,
  type ApplianceGroup,
  type BatteryType,
  type Line,
  type Plan,
} from "../power";

type T = (k: Key) => string;
type Props = { t: T; lang: Lang };
/** What the hub keeps (see the hub's api/power.rs); `rev` is new with every save. */
type Saved = { plan: unknown; updated_at: string | null; updated_by: string | null; rev?: string | null };
/**
 * A list that is not the household's saved one and is not saved as it
 * changes: opened from a link, or made while the hub could not be reached.
 */
type Draft = "link" | "unsaved" | null;

const GROUPS: { id: ApplianceGroup; title: Key }[] = [
  { id: "kitchen", title: "pwGroupKitchen" },
  { id: "devices", title: "pwGroupDevices" },
  { id: "health", title: "pwGroupHealth" },
  { id: "water", title: "pwGroupWater" },
];
const SAFETY: Key[] = ["pwSafety1", "pwSafety2", "pwSafety3", "pwSafety4", "pwSafety5"];
/** A change goes to the hub this long after the last keystroke. */
const SAVE_AFTER = 500;
/** How often changes made on another device are looked for. */
const POLL_EVERY = 15000;

/** "{name}" places in a text, filled in. */
function fill(s: string, vars: Record<string, string | number>): string {
  return s.replace(/\{(\w+)\}/g, (m, k: string) => (k in vars ? String(vars[k]) : m));
}

/** Up to the next whole `step` (a rounding error is not a step more). */
const up = (x: number, step = 1) => Math.max(0, Math.ceil(x / step - 1e-9) * step);

/** The word for n from "battery,batteries" (English) or "baterija,baterije,baterija" (Serbian). */
function plural(n: number, forms: string): string {
  const [one, few = one, many = few] = forms.split(",");
  return countWord(n, [one, few], [one, few, many]);
}

/** A number as it is typed in a field: no thousands separator, a decimal comma in Serbian. */
function inputText(v: number, lang: Lang): string {
  const s = String(Math.round(v * 100) / 100);
  return lang === "sr" ? s.replace(".", ",") : s;
}

/** A number field that takes "1,5" as well as "1.5", and keeps what is typed until it is left. */
function NumInput(props: {
  value: number;
  onChange: (v: number) => void;
  label: string;
  lang: Lang;
  min: number;
  max: number;
  unit?: string;
  integer?: boolean;
}) {
  const { value, onChange, label, lang, min, max, unit, integer } = props;
  // What is being typed; null while the field is not being edited.
  const [text, setText] = useState<string | null>(null);
  return (
    <span className="pw-num-field">
      <input
        type="text"
        inputMode={integer ? "numeric" : "decimal"}
        aria-label={label}
        value={text ?? inputText(value, lang)}
        onFocus={() => setText(inputText(value, lang))}
        onChange={(e) => {
          setText(e.target.value);
          const n = parseNumber(e.target.value);
          if (n !== null) onChange(Math.min(max, Math.max(min, integer ? Math.round(n) : n)));
        }}
        onBlur={() => setText(null)}
      />
      {unit && (
        <span className="pw-unit" aria-hidden="true">
          {unit}
        </span>
      )}
    </span>
  );
}

/** A link to a source, in the system browser (desktop and phone apps) or a new tab. */
function ExtLink({ href, children }: { href: string; children: ReactNode }) {
  return (
    <a
      href={href}
      target="_blank"
      rel="noopener noreferrer"
      onClick={(e) => {
        if (!inTauri()) return;
        e.preventDefault();
        openUrl(href).catch(() => window.open(href, "_blank", "noopener"));
      }}
    >
      {children}
    </a>
  );
}

function Stat({ label, value, sub, children }: { label: string; value: string; sub?: string; children: ReactNode }) {
  return (
    <div className="pw-stat">
      <div className="label">{label}</div>
      <div className="pw-value">
        {value}
        {sub && <span className="pw-value-sub"> {sub}</span>}
      </div>
      <div className="pw-stat-more">{children}</div>
    </div>
  );
}

/**
 * The power calculator: the household's list of appliances to keep running
 * in a power cut, and the battery, panels and inverter it needs (power.ts
 * does the arithmetic). The list is kept on the hub, the same for everyone;
 * every change is saved a moment after it is made. A link can open the
 * calculator with a list of its own (see power.ts), which is saved only when
 * the person chooses to.
 */
export default function Power({ t, lang }: Props) {
  const id = useId();
  const [plan, setPlan] = useState<Plan | null>(null);
  const [draft, setDraftState] = useState<Draft>(null);
  // The household's list was heard from the hub (so it can be shown again).
  const [hubKnown, setHubKnown] = useState(false);
  const [meta, setMeta] = useState<{ at: string; by: string | null } | null>(null);
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [pick, setPick] = useState(APPLIANCES[0].id);
  const draftRef = useRef<Draft>(null);
  const loaded = useRef(false);
  const hubPlan = useRef<Plan | null>(null);
  const hubRev = useRef<string | null>(null);
  // A change not yet on its way to the hub, and whether one is on its way.
  const pending = useRef<Plan | null>(null);
  const sending = useRef(false);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  // Changed since a draft was opened: then it is not replaced by the hub's list.
  const touched = useRef(false);
  // Counts changes, so an answer asked for before a change does not undo it.
  const edits = useRef(0);
  const pageRef = useRef<HTMLDivElement>(null);

  const setDraft = useCallback((d: Draft) => {
    draftRef.current = d;
    setDraftState(d);
  }, []);

  /** Remember the household's list as the hub has it. */
  const heard = useCallback((r: Saved | undefined, p: Plan) => {
    hubPlan.current = p;
    hubRev.current = r?.rev ?? r?.updated_at ?? null;
    setMeta(r?.updated_at ? { at: r.updated_at, by: r.updated_by } : null);
    setHubKnown(true);
  }, []);

  /** Send what waits, one request at a time; a change made meanwhile goes right after. */
  const flush = useCallback(async () => {
    if (timer.current) clearTimeout(timer.current);
    timer.current = undefined;
    if (sending.current) return;
    sending.current = true;
    setSaving(true);
    try {
      while (pending.current) {
        const next = pending.current;
        pending.current = null;
        try {
          const r = await api<Saved>("/api/power", { method: "PUT", json: { plan: next } });
          heard(r, next);
          setErr(null);
        } catch (e) {
          // Kept: it goes with the next change, or with "Retry".
          if (!pending.current) pending.current = next;
          setErr(errText(t, e));
          break;
        }
      }
    } finally {
      sending.current = false;
      setSaving(false);
    }
  }, [t, heard]);

  /** Show `saved`, or the list in the address when the calculator was opened from a link. */
  const open = useCallback(
    (saved: Plan, otherwise: Draft) => {
      const q = linkQuery(location.hash);
      // The link is used once: going back or reloading shows the household's list.
      if (q !== null) history.replaceState(history.state, "", "#power");
      const linked = q !== null ? planFromLink(q, saved) : null;
      touched.current = false;
      setPlan(linked ?? saved);
      setDraft(linked ? "link" : otherwise);
    },
    [setDraft],
  );

  const busy = () => pending.current !== null || sending.current;
  // Someone is typing on this screen: the list is not replaced under their fingers.
  const typing = () => {
    const el = document.activeElement;
    return !!el && !!pageRef.current?.contains(el) && /^(INPUT|SELECT|TEXTAREA)$/.test(el.tagName);
  };

  // The household's list: at first, then now and then for changes made on another device.
  useVisiblePoll(async () => {
    if (loaded.current && (busy() || typing())) return true;
    const before = edits.current;
    let r: Saved | undefined;
    try {
      r = await api<Saved>("/api/power");
    } catch {
      if (!loaded.current) {
        // Neither the hub nor a copy of the list: the calculator works, but nothing is saved.
        loaded.current = true;
        open(defaultPlan(), "unsaved");
      }
      return false;
    }
    if (loaded.current && (busy() || typing() || edits.current !== before)) return true;
    const saved = r?.plan ? normalizePlan(r.plan) : defaultPlan();
    const changedElsewhere = (r?.rev ?? r?.updated_at ?? null) !== hubRev.current;
    heard(r, saved);
    if (!loaded.current) {
      loaded.current = true;
      open(saved, null);
    } else if (draftRef.current === "unsaved" && !touched.current) {
      setPlan(saved);
      setDraft(null);
    } else if (draftRef.current === null && changedElsewhere) {
      setPlan(saved);
    }
    return true;
  }, POLL_EVERY);

  // A link followed while the calculator is open.
  useEffect(() => {
    const onHash = () => {
      if (loaded.current && linkQuery(location.hash) !== null) open(hubPlan.current ?? defaultPlan(), draftRef.current === "unsaved" ? "unsaved" : null);
    };
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, [open]);

  // Leaving the screen or the app sends what waits at once.
  useEffect(() => {
    const onHide = () => {
      if (document.hidden && pending.current) void flush();
    };
    document.addEventListener("visibilitychange", onHide);
    return () => {
      document.removeEventListener("visibilitychange", onHide);
      if (pending.current) void flush();
    };
  }, [flush]);

  const result = useMemo(() => (plan ? calculate(plan) : null), [plan]);
  const byMonth = useMemo(() => (plan ? panelsByMonth(plan) : []), [plan]);

  const head = (
    <>
      <div className="page-head">
        <div className="title-line">
          <h1>{t("powerCalc")}</h1>
          <HelpLink t={t} topic="power" />
        </div>
        <p className="muted">{t("pwIntro")}</p>
      </div>
      <section className="panel left pw-safety" aria-labelledby={`${id}-safety`}>
        <h2 id={`${id}-safety`}>
          <Icon name="power" size={18} />
          {t("pwSafetyTitle")}
        </h2>
        <ul>
          {SAFETY.map((k) => (
            <li key={k}>{t(k)}</li>
          ))}
        </ul>
      </section>
    </>
  );

  if (!plan || !result) {
    return (
      <div className="stack power" ref={pageRef}>
        {head}
        <p className="muted">{t("pwLoading")}</p>
      </div>
    );
  }
  const p: Plan = plan;
  const res = result;
  const inv = res.inverter;
  const months = t("pwMonths").split(",");
  const monthsShort = t("pwMonthsShort").split(",");
  const region = regionOf(p.region);
  const tableSun = sunOf(p.region, p.month);
  /** Sun hours always with their tenths: "2.0 h" beside "2.7 h". */
  const hrs = (h: number) => fmtNum(h, 2, 1);

  /** Show a change and save it a moment later (a draft is saved only when asked). */
  const change = (next: Plan) => {
    setPlan(next);
    touched.current = true;
    edits.current++;
    if (draftRef.current) return;
    pending.current = next;
    setSaving(true);
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => void flush(), SAVE_AFTER);
  };
  const setLine = (k: string, patch: Partial<Line>) => change({ ...p, lines: p.lines.map((l) => (l.k === k ? { ...l, ...patch } : l)) });
  const nameOf = (l: Line) => {
    const a = applianceOf(l.id);
    return a ? t(a.name) : l.name?.trim() || t("pwOwn");
  };
  const add = () => {
    const line = lineFor(pick);
    change({ ...p, lines: [...p.lines, line] });
    // A line of one's own is named next.
    if (line.id === "custom") setTimeout(() => document.getElementById(`${id}-name-${line.k}`)?.focus(), 0);
  };
  const pickMonth = (m: number) => change({ ...p, month: m, sunHours: sunOf(p.region, m) });
  const keep = () => {
    setDraft(null);
    touched.current = false;
    pending.current = p;
    void flush();
  };
  const showSaved = () => {
    setDraft(null);
    touched.current = false;
    setPlan(hubPlan.current ?? defaultPlan());
  };

  // The answer in one sentence.
  const pk = res.pack;
  const packText =
    `${pk.count} × ${fmtNum(pk.unitAh)} Ah ${pk.unitVolts} V ${p.battery === "lifepo4" ? "LiFePO4" : "AGM"} ${plural(pk.count, t("pwBatteryWords"))}` +
    (pk.series > 1 ? ` ${fill(t("pwInSeries"), { n: pk.series, v: p.volts })}` : "");
  const summary = inv?.sizeW
    ? fill(t("pwSummaryInv"), { batteries: packText, panels: fmtNum(res.panelsW), inverter: fmtNum(inv.sizeW) })
    : fill(t("pwSummary"), { batteries: packText, panels: fmtNum(res.panelsW) });
  const notes: string[] = [];
  if (inv?.motor) notes.push(fill(t("pwSurgeNote"), { name: t(applianceOf(inv.motor)!.name), p: fmtNum(up(inv.peakW)) }));
  if (res.maxCurrentA > 0) {
    const higher = res.betterVolts
      ? ` ${fill(t("pwHigherVolts"), { v: p.volts, v2: res.betterVolts, a: fmtNum(up((res.maxCurrentA * p.volts) / res.betterVolts)) })}`
      : "";
    notes.push(fill(t("pwCurrentNote"), { v: p.volts, a: fmtNum(up(res.maxCurrentA)) }) + higher);
  }
  if (inv) notes.push(t("pwIdleNote"));

  // The arithmetic with this list's numbers, for "How this is calculated".
  const n0 = (x: number) => fmtNum(x);
  const n2 = (x: number) => fmtNum(x, 2);
  const used = res.lineWh.filter((w) => w > 0);
  const perWatt = `(${hrs(p.sunHours)} × ${n2(PANEL_DERATE)} × ${n2(CHARGE_EFF[p.battery])})`;
  const formulas: [Key, string][] = [
    ["pwF1", `${used.length > 1 && used.length <= 8 ? `${used.map(n0).join(" + ")} = ` : ""}${n0(res.loadWh)} Wh`],
    ["pwF2", `${n0(res.acWh)} ÷ ${n2(p.inverterEff)} + ${n0(res.dcWh)} = ${n0(res.fromBatteryWh)} Wh`],
    ["pwF3", `${n0(res.fromBatteryWh)} × ${n2(p.days)} ÷ ${n2(p.usable)} = ${n0(res.batteryWh)} Wh; ${n0(res.batteryWh)} ÷ ${p.volts} V = ${n0(up(res.batteryAh))} Ah`],
    ["pwF4", `${n0(res.fromBatteryWh)} ÷ ${perWatt} = ${n0(res.keepUpW)} W`],
    ["pwF5", `${n0(res.fromBatteryWh)} × ${n2(p.days + 1)} ÷ ${perWatt} = ${n0(res.refillW)} W`],
    [
      "pwF6",
      inv
        ? `${n2(INVERTER_HEADROOM)} × ${n0(inv.loadW)} = ${n0(up(inv.continuousW))} W; ${n0(inv.loadW)} + ${n0(inv.peakW - inv.loadW)} = ${n0(up(inv.peakW))} W → ${inv.sizeW ? `${n0(inv.sizeW)} W` : "–"}`
        : t("pwNoInverter"),
    ],
  ];

  const status = saving ? (
    <p className="pw-status muted" role="status">
      {t("pwSaving")}
    </p>
  ) : err ? (
    <p className="pw-status error" role="alert">
      {t("pwNotSaved")} {err}{" "}
      <button type="button" className="link-btn" onClick={() => void flush()}>
        {t("retry")}
      </button>
    </p>
  ) : !draft && hubKnown ? (
    <p className="pw-status muted" role="status">
      {t("pwSaved")}
      {meta && ` ${fill(t("pwChangedBy"), { who: meta.by === "laptop" ? t("pwTheLaptop") : (meta.by ?? ""), when: fmtDateTime(meta.at) })}`}
    </p>
  ) : null;

  return (
    <div className="stack power" ref={pageRef}>
      {head}
      {draft && (
        <div className="panel notice stack pw-draft" role="status">
          <p>{t(draft === "link" ? "pwFromLink" : "pwUnsaved")}</p>
          <div className="row wrap">
            <button type="button" className="btn" onClick={keep}>
              {t("pwKeep")}
            </button>
            {hubKnown && (
              <button type="button" className="btn secondary" onClick={showSaved}>
                {t("pwShowSaved")}
              </button>
            )}
          </div>
        </div>
      )}
      {res.loadWh > 0 && (
        // The answer first where it would otherwise be far below (one column: phones and smaller laptops).
        <div className="pw-answer-top">
          <p className="pw-summary">{summary}</p>
          <button
            type="button"
            className="link-btn"
            onClick={() => document.getElementById(`${id}-results`)?.scrollIntoView({ block: "start", behavior: "smooth" })}
          >
            {t("pwSeeDetails")}
          </button>
        </div>
      )}
      <div className="pw-layout">
        <div className="stack pw-main">
          <section className="panel left stack" aria-labelledby={`${id}-list`}>
            <div className="pw-panel-head">
              <h2 id={`${id}-list`}>{t("pwAppliances")}</h2>
              {p.lines.length > 0 && (
                <ConfirmButton
                  label={t("pwClear")}
                  confirmLabel={t("pwYesClear")}
                  cancelLabel={t("cancel")}
                  className="btn danger small"
                  onConfirm={() => change({ ...p, lines: [] })}
                />
              )}
            </div>
            {status}
            {p.lines.length === 0 ? (
              <div className="pw-empty">
                <p className="muted">{t("pwEmpty")}</p>
                <button type="button" className="btn secondary" onClick={() => change({ ...p, lines: typicalLines() })}>
                  {t("pwTypical")}
                </button>
              </div>
            ) : (
              <div className="pw-table-wrap">
              <table className="pw-table">
                <thead>
                  <tr>
                    <th scope="col">{t("pwColAppliance")}</th>
                    <th scope="col">{t("pwColQty")}</th>
                    <th scope="col">{t("pwColWatts")}</th>
                    <th scope="col">{t("pwColUse")}</th>
                    <th scope="col">{t("pwColRunsOn")}</th>
                    <th scope="col" className="pw-right">
                      {t("pwColEnergy")}
                    </th>
                    <th scope="col">
                      <span className="sr-only">{t("remove")}</span>
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {p.lines.map((l, i) => {
                    const name = nameOf(l);
                    return (
                      <tr key={l.k}>
                        <td className="pw-name">
                          {l.id === "custom" ? (
                            <input
                              id={`${id}-name-${l.k}`}
                              type="text"
                              className="pw-own"
                              aria-label={t("pwOwnName")}
                              placeholder={t("pwOwnPlaceholder")}
                              maxLength={60}
                              value={l.name ?? ""}
                              onChange={(e) => setLine(l.k, { name: e.target.value })}
                            />
                          ) : (
                            <span className="pw-title">{name}</span>
                          )}
                          {l.whDay != null && <span className="pw-sub">{t("pwCycles")}</span>}
                        </td>
                        <td data-label={t("pwColQty")}>
                          <NumInput label={`${t("pwColQty")}: ${name}`} lang={lang} value={l.qty} min={0} max={100} integer onChange={(v) => setLine(l.k, { qty: v })} />
                        </td>
                        <td data-label={t("pwColWatts")}>
                          <NumInput label={`${t("pwColWatts")}: ${name}`} lang={lang} unit="W" value={l.watts} min={0} max={10000} onChange={(v) => setLine(l.k, { watts: v })} />
                        </td>
                        <td data-label={t("pwColUse")}>
                          {l.whDay != null ? (
                            <NumInput label={`${t("pwWhEach")}: ${name}`} lang={lang} unit="Wh" value={l.whDay} min={0} max={50000} onChange={(v) => setLine(l.k, { whDay: v })} />
                          ) : (
                            <NumInput label={`${t("pwHoursA")}: ${name}`} lang={lang} unit="h" value={l.hours} min={0} max={24} onChange={(v) => setLine(l.k, { hours: v })} />
                          )}
                        </td>
                        <td className="pw-runs" data-label={t("pwColRunsOn")}>
                          <select
                            aria-label={`${t("pwColRunsOn")}: ${name}`}
                            value={l.dc ? "dc" : "ac"}
                            onChange={(e) => setLine(l.k, { dc: e.target.value === "dc" ? true : undefined })}
                          >
                            <option value="ac">{t("pwAc")}</option>
                            <option value="dc">{t("pwDc")}</option>
                          </select>
                        </td>
                        <td className="pw-right pw-energy" data-label={t("pwColEnergy")}>
                          {fmtNum(res.lineWh[i])} Wh
                        </td>
                        <td className="pw-del">
                          <button
                            type="button"
                            className="btn secondary small icon-btn"
                            aria-label={`${t("remove")}: ${name}`}
                            title={t("remove")}
                            onClick={() => change({ ...p, lines: p.lines.filter((x) => x.k !== l.k) })}
                          >
                            <Icon name="close" size={16} />
                          </button>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
                <tfoot>
                  <tr>
                    <th scope="row" colSpan={5}>
                      {t("pwTotal")}
                    </th>
                    <td className="pw-right">{fmtNum(res.loadWh)} Wh</td>
                    <td />
                  </tr>
                </tfoot>
              </table>
              </div>
            )}
            <div className="pw-add">
              <select aria-label={t("pwAddLabel")} value={pick} onChange={(e) => setPick(e.target.value)}>
                {GROUPS.map((g) => (
                  <optgroup key={g.id} label={t(g.title)}>
                    {APPLIANCES.filter((a) => a.group === g.id).map((a) => (
                      <option key={a.id} value={a.id}>
                        {`${t(a.name)} (${a.whDay ? `${fmtNum(a.whDay)} Wh ${t("pwPerDay")}` : `${fmtNum(a.watts)} W`})`}
                      </option>
                    ))}
                  </optgroup>
                ))}
                <optgroup label={t("pwGroupOther")}>
                  <option value="custom">{t("pwOwn")}</option>
                </optgroup>
              </select>
              <button type="button" className="btn" onClick={add} disabled={p.lines.length >= MAX_LINES}>
                <Icon name="plus" size={18} />
                {t("pwAdd")}
              </button>
            </div>
          </section>

          <section className="panel left stack" aria-labelledby={`${id}-system`}>
            <h2 id={`${id}-system`}>{t("pwSystem")}</h2>
            <div className="pw-fields">
              <div className="pw-field pw-wide">
                <span className="pw-label" id={`${id}-days`}>
                  {t("pwDays")}
                </span>
                <div className="pw-inline" role="group" aria-labelledby={`${id}-days`}>
                  {DAY_CHOICES.map((n) => (
                    <button key={n} type="button" className={"chip" + (p.days === n ? " active" : "")} aria-pressed={p.days === n} onClick={() => change({ ...p, days: n })}>
                      {n} {plural(n, t("pwDayWords"))}
                    </button>
                  ))}
                  <NumInput label={t("pwDaysOwn")} lang={lang} unit={plural(p.days, t("pwDayWords"))} value={p.days} min={0.5} max={30} onChange={(v) => change({ ...p, days: v })} />
                </div>
                <p className="pw-hint">{t("pwDaysHint")}</p>
              </div>
              <label className="field pw-field">
                {t("pwBattery")}
                <select
                  value={p.battery}
                  onChange={(e) => {
                    const b = e.target.value as BatteryType;
                    change({ ...p, battery: b, usable: USABLE[b] });
                  }}
                >
                  <option value="lifepo4">{t("pwLifepo4")}</option>
                  <option value="lead">{t("pwLead")}</option>
                </select>
              </label>
              <div className="pw-field">
                <span className="pw-label">{t("pwUsable")}</span>
                <NumInput label={t("pwUsable")} lang={lang} unit="%" value={Math.round(p.usable * 100)} min={10} max={100} onChange={(v) => change({ ...p, usable: v / 100 })} />
                <p className="pw-hint">{t("pwUsableHint")}</p>
              </div>
              <div className="pw-field">
                <span className="pw-label" id={`${id}-volts`}>
                  {t("pwVolts")}
                </span>
                <div className="segmented" role="group" aria-labelledby={`${id}-volts`}>
                  {VOLTS.map((v) => (
                    <button key={v} type="button" className={p.volts === v ? "active" : ""} aria-pressed={p.volts === v} onClick={() => change({ ...p, volts: v })}>
                      {v} V
                    </button>
                  ))}
                </div>
                <p className="pw-hint">{t("pwVoltsHint")}</p>
              </div>
              <div className="pw-field">
                <span className="pw-label">{t("pwInverterEff")}</span>
                <NumInput
                  label={t("pwInverterEff")}
                  lang={lang}
                  unit="%"
                  value={Math.round(p.inverterEff * 100)}
                  min={50}
                  max={100}
                  onChange={(v) => change({ ...p, inverterEff: v / 100 })}
                />
                <p className="pw-hint">{t("pwInverterEffHint")}</p>
              </div>
            </div>
            <h3 className="pw-h3">{t("pwSun")}</h3>
            <div className="pw-fields">
              <label className="field pw-field">
                {t("pwRegion")}
                <select value={p.region} onChange={(e) => change({ ...p, region: e.target.value, sunHours: sunOf(e.target.value, p.month) })}>
                  <optgroup label={t("pwRegionsSerbia")}>
                    {SUN_REGIONS.filter((r) => r.serbia).map((r) => (
                      <option key={r.id} value={r.id}>
                        {t(r.name)}
                      </option>
                    ))}
                  </optgroup>
                  <optgroup label={t("pwRegionsOther")}>
                    {SUN_REGIONS.filter((r) => !r.serbia).map((r) => (
                      <option key={r.id} value={r.id}>
                        {t(r.name)}
                      </option>
                    ))}
                  </optgroup>
                </select>
              </label>
              <label className="field pw-field">
                {t("pwMonth")}
                <select value={p.month} onChange={(e) => pickMonth(Number(e.target.value))}>
                  {months.map((m, i) => (
                    <option key={m} value={i + 1}>
                      {m}
                    </option>
                  ))}
                </select>
              </label>
              <div className="pw-field">
                <span className="pw-label">{t("pwSunHours")}</span>
                <NumInput label={t("pwSunHours")} lang={lang} unit="h" value={p.sunHours} min={0.1} max={12} onChange={(v) => change({ ...p, sunHours: v })} />
                {p.sunHours !== tableSun && (
                  <button type="button" className="link-btn pw-hint" onClick={() => change({ ...p, sunHours: tableSun })}>
                    {fill(t("pwSunTable"), { h: hrs(tableSun) })}
                  </button>
                )}
              </div>
            </div>
            <p className="pw-hint">{t("pwSunHint")}</p>
          </section>
        </div>

        <section className="panel left stack pw-results" aria-labelledby={`${id}-results`}>
          <h2 id={`${id}-results`}>{t("pwResults")}</h2>
          {res.loadWh <= 0 ? (
            <p className="muted">{t("pwNoResults")}</p>
          ) : (
            <>
              <p className="pw-summary">{summary}</p>
              <div className="pw-stats">
                <Stat label={t("pwBatteryNeed")} value={`${fmtNum(up(res.batteryAh))} Ah`} sub={fill(t("pwBatteryAt"), { v: p.volts })}>
                  <span>{fill(t("pwBatteryMore"), { wh: fmtNum(up(res.batteryWh)), used: fmtNum(up(res.usableWh)), pct: fmtNum(p.usable * 100) })}</span>
                  <span>{fill(t("pwBatteryPack"), { pack: packText })}</span>
                </Stat>
                <Stat label={t("pwPanels")} value={`${fmtNum(res.panelsW)} W`}>
                  <span>{fill(t("pwPanelsKeep"), { month: months[p.month - 1], h: hrs(p.sunHours) })}</span>
                  <span>{fill(t("pwPanelsRefill"), { w: fmtNum(up(res.refillW, PANEL_STEP)) })}</span>
                </Stat>
                <Stat label={t("pwInverter")} value={inv ? (inv.sizeW ? `${fmtNum(inv.sizeW)} W` : `> ${fmtNum(5000)} W`) : "–"}>
                  <span>{inv ? fill(t("pwInverterMore"), { c: fmtNum(up(inv.continuousW)), p: fmtNum(up(inv.peakW)) }) : t("pwNoInverter")}</span>
                  {inv && !inv.sizeW && <span className="warn">{t("pwInverterTooBig")}</span>}
                </Stat>
                <Stat label={t("pwEnergy")} value={`${fmtNum(res.loadWh)} Wh`}>
                  <span>{fill(t("pwEnergyMore"), { wh: fmtNum(up(res.fromBatteryWh)) })}</span>
                </Stat>
              </div>
              {res.big && <p className="warn pw-big">{t("pwBigSystem")}</p>}
              {notes.length > 0 && (
                <ul className="pw-notes">
                  {notes.map((n) => (
                    <li key={n}>{n}</li>
                  ))}
                </ul>
              )}
              <div className="pw-year">
                <h3 className="pw-h3">{t("pwThroughYear")}</h3>
                <p className="pw-hint">{fill(t("pwThroughYearHint"), { region: t(region.name) })}</p>
                <div className="pw-months">
                  {region.hours.map((h, i) => (
                    <button
                      key={i}
                      type="button"
                      className={"pw-month" + (p.month === i + 1 ? " active" : "")}
                      aria-pressed={p.month === i + 1}
                      aria-label={`${months[i]}: ${hrs(h)} h, ${fmtNum(up(byMonth[i], PANEL_STEP))} W`}
                      onClick={() => pickMonth(i + 1)}
                    >
                      <span className="pw-month-name">{monthsShort[i]}</span>
                      <span>{hrs(h)} h</span>
                      <span className="pw-month-w">{fmtNum(up(byMonth[i], PANEL_STEP))} W</span>
                    </button>
                  ))}
                </div>
              </div>
              <details className="pw-how">
                <summary>{t("pwHow")}</summary>
                <ol>
                  {formulas.map(([k, sum]) => (
                    <li key={k}>
                      <span>{t(k)}</span>
                      <code>{sum}</code>
                      {k === "pwF4" && <span className="pw-hint">{t("pwF4Note")}</span>}
                    </li>
                  ))}
                </ol>
                <p className="pw-hint">{t("pwHowNote")}</p>
              </details>
            </>
          )}
          <p className="pw-sources">
            {t("pwSources")}:{" "}
            {SOURCES.map((s, i) => (
              <span key={s.name}>
                {i > 0 && " · "}
                {s.url ? <ExtLink href={s.url}>{s.name}</ExtLink> : s.name} ({t(s.what)})
              </span>
            ))}
          </p>
        </section>
      </div>
    </div>
  );
}
