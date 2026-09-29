import { useCallback, useEffect, useId, useRef, useState, type MouseEvent, type ReactNode } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, inTauri } from "../api";
import type { Key, Lang } from "../i18n";
import { errText } from "../errors";
import { fmtDateTime, fmtNum, parseNumber } from "../format";
import { useVisiblePoll } from "../poll";
import HelpLink from "../components/HelpLink";
import { Icon } from "../components/Icon";
import {
  applyLink,
  CONTAINERS,
  containersFor,
  CROPS,
  DAY_CHOICES,
  defaultPlan,
  drinkingWater,
  dripPlan,
  FLOWS_LH,
  LIMITS,
  linkQuery,
  MAX_BEDS,
  newBed,
  parsePlan,
  roundUp,
  SPACINGS_CM,
  suggestedRows,
  type Bed,
  type Drink,
  type Garden,
  type Part,
  type Tank,
  type WaterPlan,
} from "../water";
import { CITIES, fill, plural, WATER_TEXT, type WaterText } from "../water-text";

type T = (k: Key) => string;
/** What the hub keeps (see the hub's api/water.rs); `rev` is new with every save. */
type Saved = { plan: unknown; updated_at: string | null; updated_by: string | null; rev?: string | null };
/**
 * Numbers that are not the household's saved ones and are not saved as they
 * change: opened from a link, or entered while the hub could not be reached.
 */
type Draft = "link" | "unsaved" | null;

/** A change goes to the hub this long after the last keystroke. */
const SAVE_AFTER = 500;
/** How often changes made on another device are looked for. */
const POLL_EVERY = 15000;

const LINKS = {
  ready: "https://www.ready.gov/water",
  sphere: "https://spherestandards.org/handbook/",
  cdc: "https://www.cdc.gov/water-emergency/about/index.html",
  epa: "https://www.epa.gov/ground-water-and-drinking-water/emergency-disinfection-drinking-water",
  neh: "https://www.wcc.nrcs.usda.gov/ftpref/wntsc/waterMgt/irrigation/NEH15/ch2.pdf",
  worldveg: "https://www.susana.org/_resources/documents/default/2-1094-robertholmereb0086.pdf",
};

/** The part in the address: "#water/drip" is the garden. */
function hashPart(): Part {
  return /^#(tools\/)?water\/drip/.test(location.hash) ? "drip" : "drink";
}

/** The calculator's own address for a part. */
const partHash = (p: Part) => (p === "drip" ? "#water/drip" : "#water");

/** A number as it is typed in a field (no thousands separator, the language's decimal mark). */
function asTyped(v: number | null, lang: Lang): string {
  if (v === null) return "";
  const s = String(Math.round(v * 1000) / 1000);
  return lang === "sr" ? s.replace(".", ",") : s;
}

function clamp(v: number, [lo, hi]: readonly [number, number]): number {
  return Math.min(hi, Math.max(lo, v));
}

/** Opens a source in the system browser (the apps) or a new tab (a browser). */
function ExtLink({ href, children }: { href: string; children: ReactNode }) {
  const open = (e: MouseEvent) => {
    if (!inTauri()) return;
    e.preventDefault();
    openUrl(href).catch(() => window.open(href, "_blank", "noopener"));
  };
  return (
    <a href={href} target="_blank" rel="noopener noreferrer" onClick={open}>
      {children}
    </a>
  );
}

/** A number field that keeps what is being typed ("1," on the way to "1,5") and hands on the number. */
function NumField(props: {
  label: string;
  value: number | null;
  onChange: (v: number | null) => void;
  limits: readonly [number, number];
  lang: Lang;
  int?: boolean;
  hint?: string;
  placeholder?: string;
}) {
  const { label, value, onChange, limits, lang, int, hint, placeholder } = props;
  const hintId = useId();
  const [draft, setDraft] = useState<string | null>(null);
  return (
    <label className="field">
      <span>{label}</span>
      <input
        type="text"
        inputMode={int ? "numeric" : limits[0] < 0 ? "text" : "decimal"}
        value={draft ?? asTyped(value, lang)}
        placeholder={placeholder}
        aria-describedby={hint ? hintId : undefined}
        onFocus={() => setDraft(asTyped(value, lang))}
        onBlur={() => setDraft(null)}
        onChange={(e) => {
          setDraft(e.target.value);
          const n = parseNumber(e.target.value);
          onChange(n === null ? null : clamp(int ? Math.round(n) : n, limits));
        }}
      />
      {hint && (
        <span id={hintId} className="water-hint">
          {hint}
        </span>
      )}
    </label>
  );
}

/** A count with − and + beside it (people, pets). */
function Stepper({ label, value, limits, onChange, w }: { label: string; value: number; limits: readonly [number, number]; onChange: (v: number) => void; w: WaterText }) {
  const id = useId();
  const [draft, setDraft] = useState<string | null>(null);
  const [lo, hi] = limits;
  return (
    <div className="water-stepper">
      <label htmlFor={id}>{label}</label>
      <div className="qty">
        <button type="button" className="qty-btn" aria-label={`${w.less}: ${label}`} disabled={value <= lo} onClick={() => onChange(Math.max(lo, value - 1))}>
          −
        </button>
        <input
          id={id}
          className="water-count"
          type="text"
          inputMode="numeric"
          value={draft ?? String(value)}
          onFocus={() => setDraft(String(value))}
          onBlur={() => setDraft(null)}
          onChange={(e) => {
            setDraft(e.target.value);
            const n = parseNumber(e.target.value);
            onChange(n === null ? lo : clamp(Math.round(n), limits));
          }}
        />
        <button type="button" className="qty-btn" aria-label={`${w.more}: ${label}`} disabled={value >= hi} onClick={() => onChange(Math.min(hi, value + 1))}>
          +
        </button>
      </div>
    </div>
  );
}

/** A few choices side by side, one of them on. */
function Choice<V extends string | number>({
  label,
  options,
  value,
  onChange,
  hideLabel,
}: {
  label: string;
  options: { value: V; label: string }[];
  value: V;
  onChange: (v: V) => void;
  hideLabel?: boolean;
}) {
  const id = useId();
  return (
    <div className="water-choice">
      {!hideLabel && (
        <span id={id} className="water-label">
          {label}
        </span>
      )}
      <div className="segmented water-seg" role="group" aria-labelledby={hideLabel ? undefined : id} aria-label={hideLabel ? label : undefined}>
        {options.map((o) => (
          <button type="button" key={String(o.value)} className={o.value === value ? "active" : ""} aria-pressed={o.value === value} onClick={() => onChange(o.value)}>
            {o.label}
          </button>
        ))}
      </div>
    </div>
  );
}

function BedEditor({ bed, index, w, lang, onChange, onRemove }: { bed: Bed; index: number; w: WaterText; lang: Lang; onChange: (patch: Partial<Bed>) => void; onRemove: () => void }) {
  const name = fill(w.bedName, { n: index + 1 });
  const suggested = bed.width !== null && bed.width > 0 ? suggestedRows(bed.width) : null;
  return (
    <fieldset className="water-bed">
      <legend>{name}</legend>
      <div className="water-bed-head">
        <Choice
          label={`${name}: ${w.sizeBy}`}
          hideLabel
          options={[
            { value: "size" as const, label: w.bySize },
            { value: "area" as const, label: w.byArea },
          ]}
          value={bed.by}
          onChange={(by) => onChange({ by })}
        />
        <button type="button" className="btn secondary small" aria-label={`${w.removeBed}: ${name}`} onClick={onRemove}>
          {w.removeBed}
        </button>
      </div>
      <div className="water-fields">
        {bed.by === "size" ? (
          <>
            <NumField label={w.length} value={bed.length} limits={LIMITS.length} lang={lang} onChange={(length) => onChange({ length })} />
            <NumField label={w.width} value={bed.width} limits={LIMITS.width} lang={lang} onChange={(width) => onChange({ width })} />
            <NumField
              label={w.rows}
              value={bed.rows}
              limits={LIMITS.rows}
              lang={lang}
              int
              placeholder={suggested === null ? undefined : String(suggested)}
              hint={suggested === null ? undefined : fill(w.rowsHint, { n: suggested })}
              onChange={(rows) => onChange({ rows })}
            />
          </>
        ) : (
          <NumField label={w.area} value={bed.area} limits={LIMITS.area} lang={lang} onChange={(area) => onChange({ area })} />
        )}
        <label className="field water-crop">
          <span>{w.crop}</span>
          <select value={bed.crop} onChange={(e) => onChange({ crop: e.target.value as Bed["crop"] })}>
            {CROPS.map((c) => (
              <option key={c.id} value={c.id}>
                {w.crops[c.id]}
              </option>
            ))}
          </select>
        </label>
      </div>
    </fieldset>
  );
}

function tankText(tank: Tank, w: WaterText, nf: (n: number) => string): string {
  if (tank.kind === "bucket") return w.tankBucket;
  if (tank.kind === "drum") return w.tankDrum;
  if (tank.kind === "tank") return w.tankTank;
  return fill(w.tankTanks, { n: nf(tank.count) });
}

/**
 * Tools › Water calculator: drinking water to store, and drip irrigation for
 * a garden (water.ts does the arithmetic). The household's numbers are kept
 * on the hub, the same for everyone; every change is saved a moment after it
 * is made, and changes made on another device show up here. A link can open
 * the calculator with numbers of its own (see water.ts), which are saved only
 * when the person chooses to. It works like the power calculator.
 */
export default function Water({ t, lang }: { t: T; lang: Lang }) {
  const w = WATER_TEXT[lang];
  const nf = fmtNum;
  const [plan, setPlanState] = useState<WaterPlan | null>(null);
  const [part, setPartState] = useState<Part>(hashPart);
  const [draft, setDraftState] = useState<Draft>(null);
  // The household's numbers were heard from the hub (so they can be shown again).
  const [hubKnown, setHubKnown] = useState(false);
  const [meta, setMeta] = useState<{ at: string; by: string | null } | null>(null);
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  // "Other" number of days chosen (the field shows even for 3, 7 or 14).
  const [otherDays, setOtherDays] = useState(false);
  // The city the latitude was taken from, until the latitude is typed.
  const [city, setCity] = useState("");
  const planRef = useRef<WaterPlan | null>(null);
  const draftRef = useRef<Draft>(null);
  const loaded = useRef(false);
  const hubPlan = useRef<WaterPlan | null>(null);
  const hubRev = useRef<string | null>(null);
  // A change not yet on its way to the hub, and whether one is on its way.
  const pending = useRef<WaterPlan | null>(null);
  const sending = useRef(false);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  // Changed since a draft was opened: then it is not replaced by the hub's numbers.
  const touched = useRef(false);
  // Counts changes, so an answer asked for before a change does not undo it.
  const edits = useRef(0);
  const pageRef = useRef<HTMLDivElement>(null);

  const show = useCallback((p: WaterPlan) => {
    planRef.current = p;
    setPlanState(p);
  }, []);
  const setDraft = useCallback((d: Draft) => {
    draftRef.current = d;
    setDraftState(d);
  }, []);

  /** Remember the household's numbers as the hub has them. */
  const heard = useCallback((r: Saved | undefined, p: WaterPlan) => {
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
          const r = await api<Saved>("/api/water", { method: "PUT", json: { plan: next } });
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

  /** Show `saved`, or the numbers in the address when the calculator was opened from a link. */
  const open = useCallback(
    (saved: WaterPlan, otherwise: Draft) => {
      const q = linkQuery(location.hash);
      const link = q !== null ? applyLink(saved, q) : null;
      const p = link?.part ?? hashPart();
      // The link is used once: going back or reloading shows the household's numbers.
      if (q !== null) history.replaceState(history.state, "", partHash(p));
      setPartState(p);
      touched.current = false;
      show(link?.changed ? link.plan : saved);
      setDraft(link?.changed ? "link" : otherwise);
    },
    [show, setDraft],
  );

  const busy = () => pending.current !== null || sending.current;
  // Someone is typing on this screen: the numbers are not replaced under their fingers.
  const typing = () => {
    const el = document.activeElement;
    return !!el && !!pageRef.current?.contains(el) && /^(INPUT|SELECT|TEXTAREA)$/.test(el.tagName);
  };

  // The household's numbers: at first, then now and then for changes made on another device.
  useVisiblePoll(async () => {
    if (loaded.current && (busy() || typing())) return true;
    const before = edits.current;
    let r: Saved | undefined;
    try {
      r = await api<Saved>("/api/water");
    } catch {
      if (!loaded.current) {
        // Neither the hub nor a copy of the numbers: the calculator works, but nothing is saved.
        loaded.current = true;
        open(defaultPlan(), "unsaved");
      }
      return false;
    }
    if (loaded.current && (busy() || typing() || edits.current !== before)) return true;
    const saved = r?.plan ? parsePlan(r.plan) : defaultPlan();
    const changedElsewhere = (r?.rev ?? r?.updated_at ?? null) !== hubRev.current;
    heard(r, saved);
    if (!loaded.current) {
      loaded.current = true;
      open(saved, null);
    } else if (draftRef.current === "unsaved" && !touched.current) {
      show(saved);
      setDraft(null);
    } else if (draftRef.current === null && changedElsewhere) {
      show(saved);
    }
    return true;
  }, POLL_EVERY);

  // A link followed while the calculator is open, and Back between its parts.
  useEffect(() => {
    const onHash = () => {
      if (linkQuery(location.hash) !== null) {
        if (loaded.current) open(hubPlan.current ?? defaultPlan(), draftRef.current === "unsaved" ? "unsaved" : null);
      } else if (/^#water(\/|$)/.test(location.hash)) setPartState(hashPart());
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

  /** Show a change and save it a moment later (a draft is saved only when asked). */
  const change = (f: (p: WaterPlan) => WaterPlan) => {
    if (!planRef.current) return;
    const next = f(planRef.current);
    show(next);
    touched.current = true;
    edits.current++;
    if (draftRef.current) return;
    pending.current = next;
    setSaving(true);
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => void flush(), SAVE_AFTER);
  };
  const setDrink = (patch: Partial<Drink>) => change((p) => ({ ...p, drink: { ...p.drink, ...patch } }));
  const setGarden = (patch: Partial<Garden>) => change((p) => ({ ...p, garden: { ...p.garden, ...patch } }));
  const setBeds = (f: (beds: Bed[]) => Bed[]) => change((p) => ({ ...p, garden: { ...p.garden, beds: f(p.garden.beds) } }));
  const keep = () => {
    if (!planRef.current) return;
    setDraft(null);
    touched.current = false;
    pending.current = planRef.current;
    void flush();
  };
  const showSaved = () => {
    setDraft(null);
    touched.current = false;
    show(hubPlan.current ?? defaultPlan());
  };

  const setPart = (p: Part) => {
    setPartState(p);
    if (location.hash !== partHash(p)) history.replaceState(history.state, "", partHash(p));
  };

  const head = (
    <div className="page-head">
      <div className="title-line">
        <h1>{t("waterCalc")}</h1>
        <HelpLink t={t} topic="water" section={part === "drip" ? "garden" : undefined} />
      </div>
      <p className="muted">{w.intro}</p>
    </div>
  );

  if (!plan) {
    return (
      <div className="stack water" ref={pageRef}>
        {head}
        <p className="muted">{w.loading}</p>
      </div>
    );
  }

  const d = plan.drink;
  const g = plan.garden;
  const drink = drinkingWater(d);
  const drip = dripPlan(g);
  const liters = (n: number) => nf(n, n < 10 ? 1 : 0);
  const daysText = `${nf(d.days)} ${plural(lang, d.days, w.dayWord)}`;
  const dayChoice = otherDays || !(DAY_CHOICES as readonly number[]).includes(d.days) ? "other" : d.days;
  const tempsWrong = g.tmax !== null && g.tmin !== null && g.tmin > g.tmax;
  const sizedBeds = drip.beds.filter((b) => b.area !== null).length;
  const cities = [...CITIES].sort((a, b) => a[lang].localeCompare(b[lang], lang));

  const status = saving ? (
    <p className="water-sync" role="status">
      {w.saving}
    </p>
  ) : err ? (
    <p className="water-sync error" role="alert">
      {w.notSaved} {err}{" "}
      <button type="button" className="link-btn" onClick={() => void flush()}>
        {t("retry")}
      </button>
    </p>
  ) : !draft && hubKnown ? (
    <p className="water-sync" role="status">
      <Icon name="check" size={16} />
      <span>
        {w.saved}
        {meta && ` ${fill(w.changedBy, { who: meta.by === "laptop" ? w.theLaptop : (meta.by ?? ""), when: fmtDateTime(meta.at) })}`}
      </span>
    </p>
  ) : null;

  return (
    <div className="stack water" ref={pageRef}>
      {head}
      <div className="segmented water-parts" role="group" aria-label={w.parts}>
        {(["drink", "drip"] as const).map((p) => (
          <button type="button" key={p} className={part === p ? "active" : ""} aria-pressed={part === p} onClick={() => setPart(p)}>
            {p === "drink" ? w.partDrink : w.partDrip}
          </button>
        ))}
      </div>
      {draft && (
        <div className="panel notice left stack water-draft" role="status">
          <p>{draft === "link" ? w.fromLink : w.unsaved}</p>
          <div className="row wrap">
            <button type="button" className="btn" onClick={keep}>
              {w.keep}
            </button>
            {hubKnown && (
              <button type="button" className="btn secondary" onClick={showSaved}>
                {w.showSaved}
              </button>
            )}
          </div>
        </div>
      )}
      {status}

      {part === "drink" && (
        <>
          <div className="water-cols">
            <div className="stack">
              <section className="panel left stack" aria-label={w.whoTitle}>
                <h2>{w.whoTitle}</h2>
                <div className="water-steppers">
                  <Stepper label={w.adults} value={d.people} limits={LIMITS.people} w={w} onChange={(people) => setDrink({ people })} />
                  <Stepper label={w.children} value={d.children} limits={LIMITS.children} w={w} onChange={(children) => setDrink({ children })} />
                  <Stepper label={w.smallPets} value={d.smallPets} limits={LIMITS.smallPets} w={w} onChange={(smallPets) => setDrink({ smallPets })} />
                  <Stepper label={w.largePets} value={d.largePets} limits={LIMITS.largePets} w={w} onChange={(largePets) => setDrink({ largePets })} />
                </div>
                <p className="muted water-note">{w.needMore}</p>
              </section>
              <section className="panel left stack" aria-label={w.daysTitle}>
                <h2>{w.daysTitle}</h2>
                <Choice<number | "other">
                  label={w.daysTitle}
                  hideLabel
                  options={[
                    { value: 3, label: w.days3 },
                    { value: 7, label: w.days7 },
                    { value: 14, label: w.days14 },
                    { value: "other", label: w.daysOther },
                  ]}
                  value={dayChoice}
                  onChange={(v) => {
                    setOtherDays(v === "other");
                    if (v !== "other") setDrink({ days: v });
                  }}
                />
                {dayChoice === "other" && (
                  <div className="water-fields">
                    <NumField label={w.daysNumber} value={d.days} limits={LIMITS.days} lang={lang} int onChange={(v) => setDrink({ days: v ?? 1 })} />
                  </div>
                )}
              </section>
            </div>

            <div className="stack">
              <section className="panel left stack water-result" aria-label={w.storeTitle}>
                <h2>{w.storeTitle}</h2>
                <div className="water-big">
                  <p className="water-amount">
                    <span className="water-num" data-testid="water-drinking">
                      {nf(drink.drinking)} L
                    </span>{" "}
                    <span>{w.drinkingLabel}</span>
                  </p>
                  <p className="muted water-note">{fill(w.drinkingHow, { days: daysText })}</p>
                </div>
                <div className="water-big">
                  <p className="water-amount">
                    <span className="water-num" data-testid="water-hygiene">
                      {nf(drink.hygiene)} L
                    </span>{" "}
                    <span>{w.hygieneLabel}</span>
                  </p>
                  <p className="muted water-note">{w.hygieneHow}</p>
                </div>
                <div className="water-table-wrap">
                  <table className="water-table">
                    <caption>{w.containersTitle}</caption>
                    <thead>
                      <tr>
                        <td />
                        {CONTAINERS.map((size) => (
                          <th scope="col" key={size}>
                            {size === 200 ? w.drum : `${size} L`}
                          </th>
                        ))}
                      </tr>
                    </thead>
                    <tbody>
                      {(
                        [
                          [w.rowDrinking, drink.drinking],
                          [w.rowHygiene, drink.hygiene],
                        ] as const
                      ).map(([label, amount]) => (
                        <tr key={label}>
                          <th scope="row">{label}</th>
                          {containersFor(amount).map((c) => (
                            <td key={c.size}>{nf(c.count)}</td>
                          ))}
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
                <p className="muted water-note">{w.keepFresh}</p>
              </section>

              <section className="panel left stack water-safe" aria-label={w.safeTitle}>
                <h2>{w.safeTitle}</h2>
                <div>
                  <h3>{w.boilTitle}</h3>
                  <p>{w.boilText}</p>
                </div>
                <div>
                  <h3>{w.bleachTitle}</h3>
                  <p>{w.bleachText}</p>
                </div>
                <p>{w.cloudyText}</p>
                <p className="water-warn">{w.chemicalsWarn}</p>
                <p className="muted water-note">
                  {w.safeSource} <ExtLink href={LINKS.cdc}>CDC</ExtLink> · <ExtLink href={LINKS.epa}>EPA</ExtLink>
                </p>
              </section>
            </div>
          </div>

          <details className="water-how">
            <summary>{w.howTitle}</summary>
            <ul className="plain">
              {w.howDrink.map((x) => (
                <li key={x}>{x}</li>
              ))}
            </ul>
            <p className="muted water-note">
              {w.sources}: <ExtLink href={LINKS.ready}>Ready.gov</ExtLink> · <ExtLink href={LINKS.sphere}>Sphere Handbook</ExtLink> · <ExtLink href={LINKS.cdc}>CDC</ExtLink> ·{" "}
              <ExtLink href={LINKS.epa}>EPA</ExtLink>
            </p>
          </details>
        </>
      )}

      {part === "drip" && (
        <>
          <div className="water-cols">
            <div className="stack">
              <section className="panel left stack" aria-label={w.bedsTitle}>
                <h2>{w.bedsTitle}</h2>
                <p className="muted water-note">{w.bedsIntro}</p>
                {g.beds.map((b, i) => (
                  <BedEditor
                    key={b.id}
                    bed={b}
                    index={i}
                    w={w}
                    lang={lang}
                    onChange={(patch) => setBeds((beds) => beds.map((x) => (x.id === b.id ? { ...x, ...patch } : x)))}
                    onRemove={() => setBeds((beds) => beds.filter((x) => x.id !== b.id))}
                  />
                ))}
                <div>
                  <button type="button" className="btn secondary" disabled={g.beds.length >= MAX_BEDS} onClick={() => setBeds((beds) => [...beds, newBed(beds.length > 0 ? beds[beds.length - 1].crop : undefined)])}>
                    <Icon name="plus" size={16} /> {w.addBed}
                  </button>
                </div>
              </section>

              <section className="panel left stack" aria-label={w.weatherTitle}>
                <h2>{w.weatherTitle}</h2>
                <p className="muted water-note">{w.weatherIntro}</p>
                <div className="water-fields">
                  <label className="field">
                    <span>{w.month}</span>
                    <select value={g.month} onChange={(e) => setGarden({ month: Number(e.target.value) })}>
                      {w.months.map((m, i) => (
                        <option key={m} value={i + 1}>
                          {m}
                        </option>
                      ))}
                    </select>
                  </label>
                  <NumField
                    label={w.lat}
                    value={g.lat}
                    limits={LIMITS.lat}
                    lang={lang}
                    hint={w.latHint}
                    onChange={(lat) => {
                      setCity("");
                      setGarden({ lat });
                    }}
                  />
                  <label className="field">
                    <span>{w.city}</span>
                    <select
                      value={city}
                      onChange={(e) => {
                        const c = CITIES.find((x) => x.id === e.target.value);
                        setCity(c?.id ?? "");
                        if (c) setGarden({ lat: c.lat });
                      }}
                    >
                      <option value="">{w.cityPick}</option>
                      {cities.map((c) => (
                        <option key={c.id} value={c.id}>
                          {c[lang]} ({nf(c.lat, 1)}°)
                        </option>
                      ))}
                    </select>
                  </label>
                  <NumField label={w.tmax} value={g.tmax} limits={LIMITS.temp} lang={lang} onChange={(tmax) => setGarden({ tmax })} />
                  <NumField label={w.tmin} value={g.tmin} limits={LIMITS.temp} lang={lang} onChange={(tmin) => setGarden({ tmin })} />
                  <NumField label={w.rain} value={g.rain} limits={LIMITS.rain} lang={lang} hint={w.rainHint} onChange={(rain) => setGarden({ rain })} />
                </div>
                <p className="muted water-note">{w.tempHint}</p>
                {tempsWrong && (
                  <p className="error" role="alert">
                    {w.tempsWrong}
                  </p>
                )}
                <div className="water-et0" data-testid="water-et0">
                  {drip.et0Estimate !== null ? (
                    <>
                      <p>
                        <strong>{fill(w.et0Is, { n: nf(drip.et0Estimate, 1) })}</strong>
                      </p>
                      <p className="muted water-note">{w.et0Name}</p>
                    </>
                  ) : (
                    !tempsWrong && <p className="muted water-note">{w.et0Missing}</p>
                  )}
                  {g.et0 !== null && <p className="water-note">{fill(w.et0Used, { n: nf(g.et0, 1) })}</p>}
                </div>
                <div className="water-fields">
                  <NumField label={w.et0Own} value={g.et0} limits={LIMITS.et0} lang={lang} hint={w.et0OwnHint} onChange={(et0) => setGarden({ et0 })} />
                </div>
              </section>

              <section className="panel left stack" aria-label={w.dripTitle}>
                <h2>{w.dripTitle}</h2>
                <Choice label={w.spacing} options={SPACINGS_CM.map((s) => ({ value: s, label: `${s} cm` }))} value={g.spacing} onChange={(spacing) => setGarden({ spacing })} />
                <Choice label={w.flow} options={FLOWS_LH.map((f) => ({ value: f, label: `${f} L/h` }))} value={g.flow} onChange={(flow) => setGarden({ flow })} />
              </section>
            </div>

            <div className="stack">
              <section className="panel left stack water-result" aria-label={w.resultTitle}>
                <h2>{w.resultTitle}</h2>
                {sizedBeds === 0 && <p className="muted">{w.needBeds}</p>}
                {sizedBeds > 0 && drip.et0 === null && <p className="muted">{w.et0Missing}</p>}
                {drip.litersPerDay !== null && (
                  <dl className="water-facts">
                    <div>
                      <dt>{w.perDay}</dt>
                      <dd data-testid="water-per-day">{liters(drip.litersPerDay)} L</dd>
                    </div>
                    <div>
                      <dt>{w.perWeek}</dt>
                      <dd>{liters(drip.litersPerWeek ?? 0)} L</dd>
                    </div>
                    <div>
                      <dt>{w.runTime}</dt>
                      <dd data-testid="water-run">{drip.runs ? fill(drip.runs.count === 2 ? w.runTwice : w.runOnce, { n: nf(drip.runs.minutes) }) : "–"}</dd>
                    </div>
                    <div>
                      <dt>{w.drippers}</dt>
                      <dd data-testid="water-drippers">{nf(drip.emitters)}</dd>
                    </div>
                    <div>
                      <dt>{w.totalFlow}</dt>
                      <dd>{nf(drip.flowLh)} L/h</dd>
                    </div>
                    <div>
                      <dt>{w.tank}</dt>
                      <dd>{drip.tank ? `${nf(roundUp(drip.litersPerDay))} L · ${tankText(drip.tank, w, (n) => nf(n))}` : "–"}</dd>
                    </div>
                  </dl>
                )}
                {drip.litersPerDay !== null && drip.beds.length > 1 && (
                  <div>
                    <h3>{w.byBed}</h3>
                    <ul className="water-bed-list">
                      {drip.beds.map((b, i) => (
                        <li key={b.id}>
                          <span>{fill(w.bedName, { n: i + 1 })}</span>
                          <span className="muted">
                            {b.area === null || b.litersPerDay === null
                              ? "–"
                              : fill(w.bedLine, { area: nf(b.area, 1), liters: liters(b.litersPerDay), drippers: `${nf(b.emitters)} ${plural(lang, b.emitters, w.dripperWord)}` })}
                          </span>
                        </li>
                      ))}
                    </ul>
                  </div>
                )}
                <p className="muted water-note">{fill(w.pressureNote, { flow: g.flow, ml: nf((g.flow * 1000) / 6) })}</p>
              </section>

              <section className="panel left stack" aria-label={w.roofTitle}>
                <h2>{w.roofTitle}</h2>
                <div className="water-fields">
                  <NumField label={w.roofArea} value={g.roof} limits={LIMITS.roof} lang={lang} hint={w.roofHint} onChange={(roof) => setGarden({ roof })} />
                </div>
                {g.roof !== null && g.roof > 0 && (
                  <p data-testid="water-roof">
                    {drip.roofLiters === null ? (
                      <span className="muted">{w.roofNeedRain}</span>
                    ) : (
                      <>
                        {fill(w.roofResult, { rain: nf(g.rain ?? 0), liters: nf(drip.roofLiters) })}
                        {drip.roofDays !== null && ` ${fill(w.roofDays, { days: `${nf(drip.roofDays)} ${plural(lang, drip.roofDays, w.dayWord)}` })}`}
                      </>
                    )}
                  </p>
                )}
                <p className="water-warn">{w.roofWarn}</p>
              </section>
            </div>
          </div>

          <details className="water-how">
            <summary>{w.howTitle}</summary>
            <ul className="plain">
              {w.howDrip.map((x) => (
                <li key={x}>{x}</li>
              ))}
            </ul>
            <h3>{w.kcTitle}</h3>
            <table className="water-table water-kc">
              <tbody>
                {CROPS.map((c) => (
                  <tr key={c.id}>
                    <th scope="row">{w.crops[c.id]}</th>
                    <td>{nf(c.kc, 2, 2)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <p className="muted water-note">
              {w.dripSources} <ExtLink href={LINKS.neh}>NEH 623</ExtLink> · <ExtLink href={LINKS.worldveg}>More Crop Per Drop</ExtLink>
            </p>
          </details>
        </>
      )}
    </div>
  );
}
