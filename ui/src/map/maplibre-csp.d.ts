// MapLibre's CSP build (see lib.ts) has the same interface as its main one.
declare module "maplibre-gl/dist/maplibre-gl-csp.js" {
  import * as maplibregl from "maplibre-gl";
  export default maplibregl;
}
