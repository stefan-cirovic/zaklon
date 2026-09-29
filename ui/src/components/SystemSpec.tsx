import { useEffect, useState } from "react";
import { api, clientState, getMode, type AppMode, type LinkSummary } from "../api";
import { copyText } from "../clipboard";
import type { Key, Lang } from "../i18n";
import { countWord, fmtBytes, fmtDateTime } from "../format";
import { offlineState } from "../offline";
import { SPEC_TEXT, type SpecText } from "../spec-text";

/** What the interface is built with and when, from the lock file and the build (vite.config.ts). */
declare const __ZAKLON_BUILD__: { commit: string; date: string; typescript: string; react: string; maplibre: string; basemaps: string };
const UI_BUILD = typeof __ZAKLON_BUILD__ !== "undefined" ? __ZAKLON_BUILD__ : { commit: "", date: "", typescript: "", react: "", maplibre: "", basemaps: "" };

type Program = { version: string; installed: boolean };
type Model = { id: string; title_en: string; title_sr: string; size: number };

/** GET /api/system/spec (crates/zaklon-hub/src/api/spec.rs). */
export type HubSpec = {
  app: { version: string; build_date: string; commit: string; mode: string; os: string; os_build: string | null; webview2: string | null };
  built_with: { rust: string; tauri: string; sqlite: string; llama_cpp: Program | null; kiwix_serve: Program | null; libzim: string | null; comaps: string };
  database: { sqlite: string; size: number; migrations: string[]; last_backup: string | null };
  assistant: { engine: string; models: Model[]; in_use: string | null; ram_total: number; recommended: Model | null; cpu: string; cores: number | null; threads: number };
  library: { packs: number; size: number; folder: string; drive: string; drive_free: number; drive_total: number };
  maps: { world: { build: string; size: number } | null; overview: boolean; overview_build: string | null; comaps_app: string; comaps_app_on_hub: boolean; comaps_maps: number; comaps_size: number };
  network: { addresses: string[]; port: number; profiles: { adapter: string; category: string }[] | null; phones: number; fingerprint: string };
  /** Values the hub is still reading ("network_profile", "libzim"). */
  pending: string[];
};

/** A phone's own side: the app, and the hub it is paired with. */
type Phone = { mode: AppMode | null; link: LinkSummary | null };

type Row = { label: string; value: string; pending?: boolean };
type Group = { title: string; rows: Row[] };

/** The hub is asked again this often while it is still reading something, at most `RETRIES` times. */
const RETRY_MS = 1500;
const RETRIES = 20;

/**
 * Settings › About: everything about this Zaklon and the computer it runs
 * on, as a two-column table in groups, and all of it as plain text for a
 * bug report ("Copy all"). The page never waits for the hub: a phone shows
 * its own part at once, and a value the hub is still reading shows as such
 * until it arrives.
 */
export default function SystemSpec({ t, lang, isHub }: { t: (k: Key) => string; lang: Lang; isHub: boolean }) {
  const w = SPEC_TEXT[lang];
  const [hub, setHub] = useState<HubSpec | null>(null);
  const [failed, setFailed] = useState(false);
  const [reachedAt, setReachedAt] = useState<number | null>(null);
  const [phone, setPhone] = useState<Phone | null>(null);
  const [copied, setCopied] = useState<boolean | null>(null);

  useEffect(() => {
    if (isHub) return;
    let live = true;
    void Promise.all([getMode().catch(() => null), clientState().catch(() => null)]).then(([mode, link]) => {
      if (live) setPhone({ mode, link });
    });
    return () => {
      live = false;
    };
  }, [isHub]);

  useEffect(() => {
    let live = true;
    let timer: number | undefined;
    let tries = 0;
    const load = async () => {
      try {
        const spec = await api<HubSpec>("/api/system/spec");
        if (!live) return;
        setHub(spec);
        setFailed(false);
        setReachedAt(Date.now());
        if (spec.pending.length > 0 && ++tries < RETRIES) timer = window.setTimeout(load, RETRY_MS);
      } catch {
        if (live) setFailed(true);
      }
    };
    void load();
    return () => {
      live = false;
      window.clearTimeout(timer);
    };
  }, []);

  // A phone last reached the hub now, or when it made the copy it shows away from home.
  const since = reachedAt ?? (failed ? offlineState().since : null);
  const reached = since === null ? null : fmtDateTime(new Date(since).toISOString());
  const groups = specGroups(w, lang, hub, isHub ? null : (phone ?? { mode: null, link: null }), reached);

  const copy = async () => {
    const now = fmtDateTime(new Date().toISOString());
    setCopied(await copyText(specText(`${w.title} (${now})`, groups, lang)));
  };

  return (
    <div className="panel stack left spec">
      <div className="spec-head">
        <h2>{t("systemSpec")}</h2>
        <button type="button" className="btn secondary" onClick={copy} disabled={groups.length === 0}>
          {w.copyAll}
        </button>
      </div>
      <p className="muted spec-intro">{w.intro}</p>
      {copied !== null && (
        <p className={copied ? "ok spec-note" : "error spec-note"} role="status">
          {copied ? w.copied : w.copyFailed}
        </p>
      )}
      {groups.map((g, i) => (
        <section key={g.title} className="spec-group" aria-labelledby={`spec-group-${i}`}>
          <h3 id={`spec-group-${i}`}>{g.title}</h3>
          <table className="spec-table">
            <tbody>
              {g.rows.map((r) => (
                <tr key={r.label}>
                  <th scope="row">{r.label}</th>
                  <td className={r.pending ? "pending" : undefined}>{r.value}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      ))}
      {!hub && !failed && <p className="muted spec-note">{w.loading}</p>}
      {failed && !hub && <p className="muted spec-note">{w.hubUnreachable}</p>}
    </div>
  );
}

/** Everything as plain text: the title, then each group in capitals with its rows as "Label: value". */
function specText(title: string, groups: Group[], lang: Lang): string {
  const blocks = groups.map((g) => [g.title.toLocaleUpperCase(lang === "sr" ? "sr-Latn" : "en"), ...g.rows.map((r) => `${r.label}: ${r.value}`)].join("\n"));
  return `${[title, ...blocks].join("\n\n")}\n`;
}

/** The groups and rows: the laptop's from the hub; a phone's own first, then its hub's. */
function specGroups(w: SpecText, lang: Lang, hub: HubSpec | null, phone: Phone | null, reached: string | null): Group[] {
  const na = (v: string | null | undefined) => v || w.na;
  const named = (name: string, version: string | null | undefined) => `${name} ${na(version)}`;
  const build = (date: string, commit: string) => [date, commit && `${w.commit} ${commit}`].filter(Boolean).join(" · ") || w.na;
  const mode = (development: boolean) => (development ? w.development : w.release);
  const system = (a: HubSpec["app"]) => (a.os_build ? `${na(a.os)} (${w.buildNo} ${a.os_build})` : na(a.os));
  const onWindows = !!hub?.app.os.startsWith("Windows");
  const out: Group[] = [];

  if (phone) {
    const ua = typeof navigator === "undefined" ? "" : navigator.userAgent;
    out.push({
      title: w.gApp,
      rows: [
        { label: w.phoneApp, value: na(phone.mode?.version) },
        { label: w.build, value: build(UI_BUILD.date, UI_BUILD.commit) },
        { label: w.mode, value: mode(phone.mode?.debug ?? import.meta.env.DEV) },
        { label: w.android, value: na(/Android ([\d.]+)/.exec(ua)?.[1]) },
        { label: w.webview, value: na(/Chrome\/([\d.]+)/.exec(ua)?.[1]) },
      ],
    });
    const link = phone.link;
    const rows: Row[] = [
      { label: w.connectedHub, value: link && !link.linked ? w.notConnected : na(link?.hub_name) },
      { label: w.lastReached, value: na(reached) },
    ];
    if (hub) {
      rows.push(
        { label: w.hubVersion, value: `${hub.app.version} · ${build(hub.app.build_date, hub.app.commit)} · ${mode(hub.app.mode === "development")}` },
        { label: w.hubComputer, value: system(hub.app) },
      );
    }
    out.push({ title: w.gHub, rows });
  } else if (hub) {
    const a = hub.app;
    const rows: Row[] = [
      { label: w.version, value: a.version },
      { label: w.build, value: build(a.build_date, a.commit) },
      { label: w.mode, value: mode(a.mode === "development") },
      { label: onWindows ? w.windows : w.system, value: system(a) },
    ];
    if (onWindows) rows.push({ label: w.webview2, value: na(a.webview2) });
    out.push({ title: w.gApp, rows });
  }
  if (!hub) return out;

  const b = hub.built_with;
  const notInstalled = (p: Program) => (p.installed ? "" : `, ${w.notInstalled}`);
  const libzim = b.libzim ? `libzim ${b.libzim} / ` : hub.pending.includes("libzim") ? `libzim ${w.reading} / ` : "";
  out.push({
    title: w.gBuiltWith,
    rows: [
      { label: w.hubServer, value: named("Rust", b.rust) },
      { label: w.interface, value: `${named("TypeScript", UI_BUILD.typescript)} + ${named("React", UI_BUILD.react)}` },
      { label: w.apps, value: named("Tauri", b.tauri) },
      { label: w.database, value: named("SQLite", b.sqlite) },
      { label: w.aiEngine, value: b.llama_cpp ? `llama.cpp ${b.llama_cpp.version} (C++)${notInstalled(b.llama_cpp)}` : w.na },
      { label: w.libraryEngine, value: b.kiwix_serve ? `${libzim}kiwix-serve ${b.kiwix_serve.version} (C++)${notInstalled(b.kiwix_serve)}` : w.na },
      { label: w.zaklonMap, value: `${named("MapLibre", UI_BUILD.maplibre)} + ${named("Protomaps basemaps", UI_BUILD.basemaps)}` },
      { label: w.navigation, value: `CoMaps ${na(b.comaps)} (${w.separateApp})` },
    ],
  });

  const db = hub.database;
  out.push({
    title: w.gDatabase,
    rows: [
      { label: w.dbSystem, value: named("SQLite", db.sqlite) },
      { label: w.size, value: fmtBytes(db.size) },
      { label: w.schema, value: `${w.schemaAtStart} · ${w.migrations}: ${db.migrations.join(", ") || w.none}` },
      { label: w.lastBackup, value: db.last_backup ? fmtDateTime(db.last_backup) : w.noBackup },
    ],
  });

  const ai = hub.assistant;
  const title = (m: Model) => (lang === "sr" ? m.title_sr || m.title_en : m.title_en);
  const engine: Record<string, string> = {
    missing: w.engineMissing,
    no_model: w.engineNoModel,
    stopped: w.engineStopped,
    starting: w.engineStarting,
    ready: w.engineReady,
    failed: w.engineFailed,
  };
  const state = engine[ai.engine] ?? ai.engine;
  const inUse = ai.in_use ? ai.models.find((m) => m.id === ai.in_use) : undefined;
  const cores = ai.cores ? `${ai.cores} ${countWord(ai.cores, ["core", "cores"], ["jezgro", "jezgra", "jezgara"])}, ` : "";
  out.push({
    title: w.gAssistant,
    rows: [
      { label: w.engine, value: b.llama_cpp?.installed ? `llama.cpp ${b.llama_cpp.version} · ${state}` : state },
      { label: w.models, value: ai.models.map((m) => `${title(m)}, ${fmtBytes(m.size)}`).join("; ") || w.none },
      { label: w.inUse, value: inUse ? title(inUse) : (ai.in_use ?? w.none) },
      {
        label: w.memory,
        value: `${fmtBytes(ai.ram_total)} · ${ai.recommended ? `${w.recommends} ${title(ai.recommended)}, ${fmtBytes(ai.recommended.size)}` : w.noModelFits}`,
      },
      { label: w.processor, value: `${na(ai.cpu)} · ${cores}${ai.threads} ${countWord(ai.threads, ["thread", "threads"], ["nit", "niti", "niti"])}` },
    ],
  });

  const lib = hub.library;
  out.push({
    title: w.gLibrary,
    rows: [
      { label: w.packs, value: `${lib.packs} ${countWord(lib.packs, ["pack", "packs"], ["paket", "paketa", "paketa"])}${lib.packs ? `, ${fmtBytes(lib.size)}` : ""}` },
      { label: w.folder, value: na(lib.folder) },
      { label: w.drive, value: lib.drive_total ? `${na(lib.drive)} · ${fmtBytes(lib.drive_free)} ${w.free} ${fmtBytes(lib.drive_total)}` : na(lib.drive) },
    ],
  });

  const maps = hub.maps;
  out.push({
    title: w.gMaps,
    rows: [
      { label: w.worldMap, value: maps.world ? `${w.buildNo} ${maps.world.build}, ${fmtBytes(maps.world.size)}` : maps.overview ? w.overviewOnly : w.noMap },
      { label: w.overview, value: maps.overview ? (maps.overview_build ? `${w.included}, ${w.buildNo} ${maps.overview_build}` : w.included) : w.notIncluded },
      { label: w.comapsApp, value: `${na(maps.comaps_app)} · ${maps.comaps_app_on_hub ? w.onHub : w.notOnHub}` },
      {
        label: w.comapsMaps,
        value: maps.comaps_maps ? `${maps.comaps_maps} ${countWord(maps.comaps_maps, ["piece", "pieces"], ["deo", "dela", "delova"])}, ${fmtBytes(maps.comaps_size)}` : w.none,
      },
    ],
  });

  const net = hub.network;
  const category: Record<string, string> = { Private: w.netPrivate, Public: w.netPublic, Domain: w.netDomain };
  const rows: Row[] = [
    { label: w.addresses, value: net.addresses.join(", ") || w.none },
    { label: w.port, value: String(net.port) },
  ];
  if (onWindows) {
    const reading = net.profiles === null && hub.pending.includes("network_profile");
    const profiles = net.profiles?.map((p) => `${category[p.category] ?? p.category}${p.adapter ? ` (${p.adapter})` : ""}`).join(", ");
    rows.push({ label: w.profile, value: reading ? w.reading : net.profiles === null ? w.na : profiles || w.none, pending: reading });
  }
  rows.push(
    { label: w.phones, value: String(net.phones) },
    { label: w.fingerprint, value: net.fingerprint ? `${(net.fingerprint.toUpperCase().match(/.{1,4}/g) ?? []).join(" ")} …` : w.na },
  );
  out.push({ title: w.gNetwork, rows });
  return out;
}
