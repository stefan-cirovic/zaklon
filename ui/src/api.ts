import { invoke } from "@tauri-apps/api/core";

export type AppMode = {
  mode: "hub" | "client";
  api_base: string | null;
  platform: string;
  version: string;
};

export type Status = {
  hub_id: string;
  hub_name: string;
  version: string;
  port: number;
  fingerprint: string;
  set_up: boolean;
  language: "en" | "sr";
  devices?: number;
  uptime_secs?: number;
  addresses?: string[];
  root?: string;
};

export type Device = { id: string; name: string; platform: string; created_at: string; last_seen: string | null };

export type PairPayload = {
  v: number;
  hosts: string[];
  port: number;
  fp: string;
  code: string;
  name: string;
  install_port: number;
};

export type PairStart = { code: string; expires_in_secs: number; payload: PairPayload };

export type LinkSummary = {
  linked: boolean;
  hub_id: string | null;
  hub_name: string | null;
  device_id: string | null;
  hosts: string[];
  port: number | null;
  last_host: string | null;
};

export type DiscoveredHub = { host: string; port: number; fp: string; id: string; name: string };

let modePromise: Promise<AppMode> | null = null;

export function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * Outside the Tauri app shell the page is always served by the hub itself
 * (the desktop window or a browser on the laptop), so the API is on the same
 * origin. VITE_API_BASE points the Vite dev server at a running hub.
 */
export function getMode(): Promise<AppMode> {
  if (!modePromise) {
    modePromise = inTauri()
      ? invoke<AppMode>("app_mode").catch(() => sameOrigin())
      : Promise.resolve(sameOrigin());
  }
  return modePromise;
}

function sameOrigin(): AppMode {
  return { mode: "hub", api_base: import.meta.env.VITE_API_BASE ?? "", platform: "windows", version: "" };
}

export class ApiError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

function errorFromBody(status: number, text: string, fallback: string): ApiError {
  let msg = fallback;
  try {
    msg = (JSON.parse(text) as { error?: string }).error ?? fallback;
  } catch {
    /* not JSON */
  }
  return new ApiError(status, msg);
}

/** Call the hub. On the laptop this goes to 127.0.0.1; on phones it goes through the pinned-TLS client in Rust. */
export async function api<T = unknown>(path: string, init: { method?: string; json?: unknown } = {}): Promise<T> {
  const mode = await getMode();
  const method = init.method ?? (init.json !== undefined ? "POST" : "GET");
  const body = init.json !== undefined ? JSON.stringify(init.json) : undefined;

  if (mode.mode === "client") {
    const res = await invoke<{ status: number; body: string }>("client_request", { method, path, body: body ?? null });
    if (res.status >= 400) throw errorFromBody(res.status, res.body, `hub replied ${res.status}`);
    if (res.status === 204 || !res.body) return undefined as T;
    return JSON.parse(res.body) as T;
  }

  if (mode.api_base === null) throw new ApiError(0, "not connected to a hub");
  const res = await fetch(mode.api_base + path, {
    method,
    headers: body !== undefined ? { "content-type": "application/json" } : undefined,
    body,
  });
  if (!res.ok) throw errorFromBody(res.status, await res.text(), res.statusText);
  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
}

// ---- phone-side link management ------------------------------------------

export function clientState(): Promise<LinkSummary> {
  return invoke<LinkSummary>("client_state");
}

export function clientPair(payload: PairPayload, password: string, deviceName: string): Promise<LinkSummary> {
  return invoke<LinkSummary>("client_pair", { payload, password, deviceName });
}

export function clientForget(): Promise<void> {
  return invoke("client_forget");
}

export function clientDiscover(): Promise<DiscoveredHub[]> {
  return invoke<DiscoveredHub[]>("client_discover");
}

/** Where library articles are loaded from: the hub itself on the laptop, the loopback proxy on phones. */
export async function contentBase(): Promise<string> {
  const mode = await getMode();
  if (mode.mode === "client") return invoke<string>("client_content_base");
  return mode.api_base ?? "";
}
