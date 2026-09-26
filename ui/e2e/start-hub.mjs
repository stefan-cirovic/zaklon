// Starts a throwaway hub for the interface tests: fresh data folder, fixed
// test ports, the debug build of zaklon-hub. Playwright stops it afterwards.
import { spawn } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const root = mkdtempSync(join(tmpdir(), "zaklon-ui-e2e-"));
const exe = resolve(import.meta.dirname, "../../target/debug/zaklon-hub" + (process.platform === "win32" ? ".exe" : ""));

const child = spawn(exe, ["--root", root], {
  stdio: "inherit",
  env: {
    ...process.env,
    ZAKLON_LOCAL_PORT: "28481",
    ZAKLON_TLS_PORT: "28484",
    ZAKLON_INSTALL_PORT: "28480",
    ZAKLON_BEACON_PORT: "28485",
    ZAKLON_IGNORE_BATTERY: "1",
  },
});
const stop = () => child.kill();
process.on("SIGINT", stop);
process.on("SIGTERM", stop);
process.on("exit", stop);
child.on("exit", (code) => process.exit(code ?? 0));
