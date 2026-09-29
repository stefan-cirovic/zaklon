// Starts a throwaway hub for the interface tests: fresh data folder, fixed
// test ports, the debug build of zaklon-hub. Playwright stops it afterwards.
// ZAKLON_HUB_EXE points at another build (e.g. with a different CARGO_TARGET_DIR).
// ZAKLON_E2E_PORT_BASE moves the ports (default 28480: install 28480, local
// 28481, TLS 28484, beacon 28485), so two test runs on one machine do not meet.
// The map's assets are a tiny stand-in for what scripts/fetch-map-assets.sh
// makes (e2e/fixtures/map-assets: the world at zoom 0-1 from the Protomaps
// basemap, map data © OpenStreetMap contributors, ODbL; one range of Noto Sans
// glyphs, OFL; a few places from GeoNames, CC BY 4.0), unless ZAKLON_MAP_ASSETS
// names the real ones (for screenshots).
import { spawn } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const root = mkdtempSync(join(tmpdir(), "zaklon-ui-e2e-"));
const exe = process.env.ZAKLON_HUB_EXE || resolve(import.meta.dirname, "../../target/debug/zaklon-hub" + (process.platform === "win32" ? ".exe" : ""));
const base = Number(process.env.ZAKLON_E2E_PORT_BASE || 28480);

const child = spawn(exe, ["--root", root], {
  stdio: "inherit",
  env: {
    ...process.env,
    ZAKLON_LOOPBACK_ONLY: "1",
    ZAKLON_LOCAL_PORT: String(base + 1),
    ZAKLON_TLS_PORT: String(base + 4),
    ZAKLON_INSTALL_PORT: String(base),
    ZAKLON_BEACON_PORT: String(base + 5),
    ZAKLON_IGNORE_BATTERY: "1",
    // Never read Protomaps' list of world map builds: the tests stay offline
    // and see the build that comes with the app (or simulate the hub's answers).
    ZAKLON_WORLD_BUILDS_URL: "off",
    ZAKLON_MAP_ASSETS: process.env.ZAKLON_MAP_ASSETS || resolve(import.meta.dirname, "fixtures/map-assets"),
  },
});
const stop = () => child.kill();
process.on("SIGINT", stop);
process.on("SIGTERM", stop);
process.on("exit", stop);
child.on("exit", (code) => process.exit(code ?? 0));
