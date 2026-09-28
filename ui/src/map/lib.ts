// The map's drawing library, MapLibre GL JS (BSD-3-Clause), loaded only
// when a map is first shown: it is large, and most screens never need it.
//
// Its "CSP" build is used, with the part that runs in the background (the
// worker) as a file of its own: the worker then starts with the same
// additions for older web views as the page (MapLibre 5 uses Array.at and
// Object.hasOwn, which Chromium 90 lacks; the build targets Chromium 90).
import type * as MapLibre from "maplibre-gl";

export type MapLibreModule = typeof MapLibre;

/** What MapLibre uses and older web views lack. Serialized into the worker as well. */
function addMissing(g: typeof globalThis) {
  const define = (target: object, name: string, value: unknown) => {
    if (!(name in target)) Object.defineProperty(target, name, { value, writable: true, configurable: true });
  };
  function at(this: ArrayLike<unknown>, index: number) {
    let i = Math.trunc(index) || 0;
    if (i < 0) i += this.length;
    return i < 0 || i >= this.length ? undefined : this[i];
  }
  define(g.Array.prototype, "at", at);
  define(g.String.prototype, "at", at);
  for (const typed of [g.Int8Array, g.Uint8Array, g.Uint8ClampedArray, g.Int16Array, g.Uint16Array, g.Int32Array, g.Uint32Array, g.Float32Array, g.Float64Array]) {
    if (typed) define(typed.prototype, "at", at);
  }
  define(g.Object, "hasOwn", (o: object, k: PropertyKey) => Object.prototype.hasOwnProperty.call(o, k));
}

/** The web view can draw with WebGL (MapLibre needs it; WebGL 1 is enough). */
export function webglAvailable(): boolean {
  try {
    const canvas = document.createElement("canvas");
    return !!(canvas.getContext("webgl2") || canvas.getContext("webgl"));
  } catch {
    return false;
  }
}

let loading: Promise<MapLibreModule> | null = null;

export function loadMapLibre(): Promise<MapLibreModule> {
  if (!loading) {
    addMissing(globalThis);
    loading = Promise.all([
      import("maplibre-gl/dist/maplibre-gl-csp.js"),
      import("maplibre-gl/dist/maplibre-gl-csp-worker.js?url"),
      import("maplibre-gl/dist/maplibre-gl.css"),
    ])
      .then(([lib, worker]) => {
        const maplibregl = ((lib as { default?: MapLibreModule }).default ?? lib) as MapLibreModule;
        const workerUrl = new URL(worker.default, location.href).href;
        const source = `(${addMissing.toString()})(self);\nimportScripts(${JSON.stringify(workerUrl)});\n`;
        maplibregl.setWorkerUrl(URL.createObjectURL(new Blob([source], { type: "text/javascript" })));
        return maplibregl;
      })
      .catch((e) => {
        loading = null;
        throw e;
      });
  }
  return loading;
}
