import { useEffect, useImperativeHandle, useRef, useState, type ReactNode, type Ref } from "react";
import type { GeoJSONSource, Map as MapLibreMap, Marker } from "maplibre-gl";
import { openUrl } from "@tauri-apps/plugin-opener";
import { inTauri } from "../api";
import type { Key, Lang } from "../i18n";
import { hasTiles, homeZoom, mapBase, type LatLon, type MapInfo } from "../map/info";
import type { MapLibreModule } from "../map/lib";
import { IconFrame } from "./Icon";

type T = (k: Key) => string;

/** The credit the map's data needs, always in view (ODbL). */
const ATTRIBUTION = { text: "Protomaps © OpenStreetMap", href: "https://www.openstreetmap.org/copyright" };
/** Where the whole world is in view. */
const WORLD_CENTER: [number, number] = [15, 28];
/** The color under the map while its tiles load, the same as the map's background. */
const BACKGROUND = "#1b1d20";

export type ZaklonMapHandle = {
  /** Move the map to a place, at about a town's size unless told otherwise. */
  flyTo: (p: LatLon, zoom?: number) => void;
  showWorld: () => void;
};

type Props = {
  t: T;
  lang: Lang;
  /** What the hub says about the map (null until it answered). */
  info: MapInfo | null;
  /** The household's home: a marker in the accent color. */
  home: LatLon | null;
  /** A place chosen but not saved yet: a ring marker. */
  pending?: LatLon | null;
  /** A tap or click on the map, when choosing a place on it. */
  onPick?: (p: LatLon) => void;
  /** Where the map opens: at the home when there is one, or on the whole world. */
  start: "home" | "world";
  /** A small map inside a page (Home): the mouse wheel scrolls the page, not the map. */
  compact?: boolean;
  /** What the map region is called for screen readers. */
  label: string;
  ref?: Ref<ZaklonMapHandle>;
  /** Shown over the map, bottom left (notes, a button). */
  children?: ReactNode;
};

type Phase = "loading" | "ready" | "nowebgl" | "failed";

function worldZoom(width: number): number {
  return Math.max(0, Math.log2(Math.max(width, 256) / 512));
}

/** The ring on a place chosen but not saved yet (it lives outside React, on the map). */
function pendingElement(label: string): HTMLElement {
  const el = document.createElement("div");
  el.className = "zmap-marker pending";
  el.setAttribute("role", "img");
  el.setAttribute("aria-label", label);
  return el;
}

/** The home's mark and the map layer that draws it. */
const HOME = "zaklon-home";

/**
 * The home's mark as an image: a disc in the accent color with a house, drawn
 * on the map itself (not over it), so the map's labels make room for it.
 */
function homeIcon(): ImageData | null {
  const px = 2;
  const size = 36 * px;
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = size;
  const g = canvas.getContext("2d");
  if (!g) return null;
  const accent = getComputedStyle(document.documentElement).getPropertyValue("--accent").trim() || "#f2b366";
  g.beginPath();
  g.arc(size / 2, size / 2, size / 2 - 3 * px, 0, Math.PI * 2);
  g.fillStyle = accent;
  g.fill();
  g.lineWidth = 2 * px;
  g.strokeStyle = "#161616";
  g.stroke();
  // The house of the app's icons (a 24 px grid), 18 px wide in the middle.
  g.save();
  g.translate(size / 2 - 9 * px, size / 2 - 9 * px);
  g.scale((18 * px) / 24, (18 * px) / 24);
  g.lineWidth = 2.2;
  g.lineCap = "round";
  g.lineJoin = "round";
  g.strokeStyle = "#1f1f1f";
  g.stroke(new Path2D("M3.5 11 12 4l8.5 7"));
  g.stroke(new Path2D("M5.5 9.5V20H10v-5.5h4V20h4.5V9.5"));
  g.restore();
  return g.getImageData(0, 0, size, size);
}

/** The home as map data (nothing when there is none). */
function homeData(home: LatLon | null) {
  return {
    type: "FeatureCollection" as const,
    features: home ? [{ type: "Feature" as const, properties: {}, geometry: { type: "Point" as const, coordinates: [home.lon, home.lat] } }] : [],
  };
}

/** Put the home's mark on the map's current style (every new style starts without it). */
function addHome(m: MapLibreMap, home: LatLon | null) {
  if (!m.hasImage(HOME)) {
    const icon = homeIcon();
    if (icon) m.addImage(HOME, icon, { pixelRatio: 2 });
  }
  const source = m.getSource(HOME) as GeoJSONSource | undefined;
  if (source) source.setData(homeData(home));
  else m.addSource(HOME, { type: "geojson", data: homeData(home) });
  if (!m.getLayer(HOME)) {
    // The top layer: placed first, so the labels below keep clear of it.
    m.addLayer({ id: HOME, type: "symbol", source: HOME, layout: { "icon-image": HOME, "icon-allow-overlap": true, "icon-ignore-placement": false } });
  }
}

/**
 * The Zaklon map: the world from the hub's map archives, drawn with
 * MapLibre (loaded the first time a map shows). Dark and calm, labels in the
 * app's language, the home as the one colored mark, and the data's credit
 * always in view. Where the web view cannot draw it (no WebGL), a message
 * says so instead of an empty box.
 */
export default function ZaklonMap({ t, lang, info, home, pending = null, onPick, start, compact = false, label, ref, children }: Props) {
  const box = useRef<HTMLDivElement>(null);
  const map = useRef<MapLibreMap | null>(null);
  const lib = useRef<MapLibreModule | null>(null);
  const base = useRef("");
  // The home as last given, for each new style.
  const homeNow = useRef<LatLon | null>(home);
  const pendingMarker = useRef<Marker | null>(null);
  const pick = useRef(onPick);
  // The person moved the map: it no longer jumps to the home by itself.
  const moved = useRef(false);
  const [phase, setPhase] = useState<Phase>("loading");
  // The map exists (created, style still loading).
  const [made, setMade] = useState(false);

  useEffect(() => {
    pick.current = onPick;
  });

  // Make the map once; the drawing library and the style come with it.
  useEffect(() => {
    let alive = true;
    let created: MapLibreMap | null = null;
    (async () => {
      const [{ loadMapLibre, webglAvailable }, hubBase] = await Promise.all([import("../map/lib"), mapBase()]);
      if (!alive) return;
      if (!webglAvailable()) {
        setPhase("nowebgl");
        return;
      }
      const maplibregl = await loadMapLibre();
      if (!alive || !box.current) return;
      lib.current = maplibregl;
      base.current = hubBase;
      const width = box.current.clientWidth;
      created = new maplibregl.Map({
        container: box.current,
        style: { version: 8, sources: {}, layers: [{ id: "background", type: "background", paint: { "background-color": BACKGROUND } }] },
        center: WORLD_CENTER,
        zoom: worldZoom(width),
        minZoom: 0,
        attributionControl: false,
        dragRotate: false,
        pitchWithRotate: false,
        touchPitch: false,
        maxPitch: 0,
        fadeDuration: 150,
      });
      created.touchZoomRotate.disableRotation();
      created.keyboard.disableRotation();
      if (compact) created.scrollZoom.disable();
      created.on("click", (e) => pick.current?.({ lat: e.lngLat.lat, lon: e.lngLat.lng }));
      created.on("dragstart", () => (moved.current = true));
      created.on("zoomstart", (e) => {
        if ((e as { originalEvent?: unknown }).originalEvent) moved.current = true;
      });
      created.on("error", (e) => console.warn("map:", e.error?.message ?? e));
      created.on("style.load", () => addHome(created!, homeNow.current));
      created.once("load", () => alive && setPhase("ready"));
      map.current = created;
      setMade(true);
    })().catch((e) => {
      console.warn("map:", e);
      if (alive) setPhase(/webgl/i.test(String(e instanceof Error ? e.message : e)) ? "nowebgl" : "failed");
    });
    return () => {
      alive = false;
      created?.remove();
      map.current = null;
      pendingMarker.current = null;
    };
  }, [compact]);

  // The style, whenever what the hub has or the language changes.
  const key = info ? `${info.tiles.key}|${info.tiles.max_zoom}|${info.glyphs}|${info.sprites}|${lang}` : "";
  useEffect(() => {
    const m = map.current;
    if (!made || !m || !info || !hasTiles(info)) return;
    let alive = true;
    import("../map/style")
      .then(({ mapStyle }) => {
        if (!alive || map.current !== m) return;
        m.setMaxZoom(info.tiles.detailed ? 18 : Math.max(info.tiles.max_zoom + 3, 6));
        m.setStyle(mapStyle(base.current, info, lang));
      })
      .catch((e) => console.warn("map style:", e));
    return () => {
      alive = false;
    };
    // `key` stands for everything of `info` the style uses.
  }, [made, key]);

  // The home's mark.
  const homeLat = home?.lat;
  const homeLon = home?.lon;
  useEffect(() => {
    homeNow.current = homeLat === undefined || homeLon === undefined ? null : { lat: homeLat, lon: homeLon };
    const m = map.current;
    if (made && m?.isStyleLoaded()) addHome(m, homeNow.current);
  }, [made, homeLat, homeLon]);

  // Open at the home once it is known, unless the person moved the map already.
  const detailed = !!info?.tiles.detailed;
  useEffect(() => {
    const m = map.current;
    if (!made || !m || start !== "home" || moved.current || homeLat === undefined || homeLon === undefined) return;
    m.jumpTo({ center: [homeLon, homeLat], zoom: homeZoom(info) });
    // Only the home and how detailed the map is decide where it opens.
  }, [made, start, homeLat, homeLon, detailed]);

  // The place chosen but not saved.
  useEffect(() => {
    const m = map.current;
    const maplibregl = lib.current;
    if (!made || !m || !maplibregl) return;
    if (!pending) {
      pendingMarker.current?.remove();
      pendingMarker.current = null;
      return;
    }
    if (!pendingMarker.current) pendingMarker.current = new maplibregl.Marker({ element: pendingElement(t("mapChosenPlace")) }).setLngLat([pending.lon, pending.lat]).addTo(m);
    else pendingMarker.current.setLngLat([pending.lon, pending.lat]);
  }, [made, pending, t]);

  // The box changes size with the window and the layout.
  useEffect(() => {
    const el = box.current;
    if (!made || !el || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(() => map.current?.resize());
    ro.observe(el);
    return () => ro.disconnect();
  }, [made]);

  const flyTo = (p: LatLon, zoom?: number) => {
    moved.current = true;
    map.current?.flyTo({ center: [p.lon, p.lat], zoom: zoom ?? homeZoom(info), essential: true });
  };
  const showWorld = () => {
    moved.current = true;
    map.current?.flyTo({ center: WORLD_CENTER, zoom: worldZoom(box.current?.clientWidth ?? 800), essential: true });
  };
  useImperativeHandle(ref, () => ({ flyTo, showWorld }));

  const openCredit = (e: React.MouseEvent) => {
    if (!inTauri()) return;
    e.preventDefault();
    openUrl(ATTRIBUTION.href).catch(() => window.open(ATTRIBUTION.href, "_blank", "noopener"));
  };

  const broken = phase === "nowebgl" || phase === "failed";
  const zoomBy = (d: number) => {
    moved.current = true;
    if (d > 0) map.current?.zoomIn();
    else map.current?.zoomOut();
  };

  return (
    <div
      className={"zmap" + (compact ? " compact" : "") + (onPick ? " picking" : "")}
      role="region"
      aria-label={label}
      data-home={home ? `${home.lat},${home.lon}` : undefined}
    >
      <div className="zmap-canvas" ref={box} />
      {broken && (
        <div className="zmap-message" role="status">
          <MapGlyph />
          <p>{phase === "nowebgl" ? t("mapNoWebgl") : t("mapFailed")}</p>
        </div>
      )}
      {!broken && phase === "loading" && <p className="zmap-loading muted">{t("mapLoading")}</p>}
      {!broken && (
        <div className="zmap-controls">
          <button type="button" className="zmap-btn" onClick={() => zoomBy(1)} aria-label={t("mapZoomIn")} title={t("mapZoomIn")}>
            <IconFrame size={18}>
              <path d="M12 5v14M5 12h14" />
            </IconFrame>
          </button>
          <button type="button" className="zmap-btn" onClick={() => zoomBy(-1)} aria-label={t("mapZoomOut")} title={t("mapZoomOut")}>
            <IconFrame size={18}>
              <path d="M5 12h14" />
            </IconFrame>
          </button>
          {home && (
            <button type="button" className="zmap-btn" onClick={() => flyTo(home)} aria-label={t("mapGoHome")} title={t("mapGoHome")}>
              <IconFrame size={18}>
                <path d="M3.5 11 12 4l8.5 7" />
                <path d="M5.5 9.5V20H10v-5.5h4V20h4.5V9.5" />
              </IconFrame>
            </button>
          )}
          {!compact && (
            <button type="button" className="zmap-btn" onClick={showWorld} aria-label={t("mapShowWorld")} title={t("mapShowWorld")}>
              <IconFrame size={18}>
                <circle cx="12" cy="12" r="8.5" />
                <path d="M3.5 12h17M12 3.5c2.5 2.6 3.5 5.5 3.5 8.5s-1 5.9-3.5 8.5c-2.5-2.6-3.5-5.5-3.5-8.5s1-5.9 3.5-8.5z" />
              </IconFrame>
            </button>
          )}
        </div>
      )}
      {children && <div className="zmap-overlay">{children}</div>}
      <a className="zmap-credit" href={ATTRIBUTION.href} target="_blank" rel="noopener" onClick={openCredit}>
        {ATTRIBUTION.text}
      </a>
    </div>
  );
}

/** A folded map, for the message where no map can be drawn. */
function MapGlyph() {
  return (
    <IconFrame size={32}>
      <path d="M9 4.5 4 6.5v13l5-2 6 2 5-2v-13l-5 2z" />
      <path d="M9 4.5v13M15 6.5v13" />
    </IconFrame>
  );
}
