import { invoke } from "@tauri-apps/api/core";
import {
  adoptParked,
  clearOffline,
  flush,
  fromCache,
  isCacheable,
  isQueueable,
  knownAway,
  markMissed,
  markOnline,
  offlineReady,
  queue,
  remember,
  requestProbe,
  setStore,
  settle,
  withClientId,
  withWaiting,
  type StoreName,
} from "./offline";

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

/**
 * What the pairing QR code holds (version 2): the hub's addresses and
 * certificate fingerprint, and a long secret the phone pairs with. The
 * 6-digit code is not in it; that one is typed after "Find hubs".
 */
export type PairPayload = {
  v: number;
  hosts: string[];
  port: number;
  fp: string;
  secret: string;
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
      ? invoke<AppMode>("app_mode")
          .then((m) => {
            // Phones keep waiting shopping list changes in files the app
            // writes to the disk at once (the web view's storage may lose the
            // last seconds when Android kills the app).
            if (m.mode === "client") {
              setStore({
                load: (name: StoreName) => invoke<string | null>("outbox_read", { name }),
                save: (name: StoreName, data: string) => invoke<void>("outbox_write", { name, data }),
              });
            }
            return m;
          })
          .catch(() => sameOrigin())
      : Promise.resolve(sameOrigin());
  }
  return modePromise;
}

function sameOrigin(): AppMode {
  return { mode: "hub", api_base: import.meta.env.VITE_API_BASE ?? "", platform: "", version: "" };
}

export class ApiError extends Error {
  status: number;
  /** Stable code from the hub (e.g. "no_disk_space"), what the app translates. */
  code?: string;
  constructor(status: number, message: string, code?: string) {
    super(message);
    this.status = status;
    this.code = code;
  }
}

function errorFromBody(status: number, text: string, fallback: string): ApiError {
  let msg = fallback;
  let code: string | undefined;
  try {
    const body = JSON.parse(text) as { error?: string; code?: string };
    msg = body.error ?? fallback;
    code = body.code;
  } catch {
    /* not JSON */
  }
  return new ApiError(status, msg, code);
}

/** Call the hub. On the laptop this goes to 127.0.0.1; on phones it goes through the pinned-TLS client in Rust. */
export async function api<T = unknown>(path: string, init: { method?: string; json?: unknown } = {}): Promise<T> {
  const mode = await getMode();
  const method = init.method ?? (init.json !== undefined ? "POST" : "GET");
  let body = init.json !== undefined ? JSON.stringify(init.json) : undefined;

  if (mode.mode === "client") {
    // Everything but keeping a change works without the saved outbox (queue() reports that).
    await offlineReady().catch(() => {});
    body = withClientId(method, path, body);
    // The hub was out of reach a moment ago (the phone is at the shop): answer
    // from the copy, or keep the change, right away instead of waiting for
    // every address to time out; the app looks for the hub meanwhile.
    if (knownAway()) {
      const answer = await offlineAnswer(method, path, body);
      if (answer !== null) {
        requestProbe();
        return answer.value as T;
      }
    }
    let res: { status: number; body: string };
    try {
      res = await invoke<{ status: number; body: string }>("client_request", { method, path, body: body ?? null });
    } catch (e) {
      // The hub is out of reach (the phone is not at home): use the last copy,
      // and let shopping list changes wait for the hub. A shopping list add
      // may have reached the hub even so; its client id makes a second copy
      // harmless.
      markMissed();
      const answer = await offlineAnswer(method, path, body);
      if (answer !== null) return answer.value as T;
      throw e;
    }
    markOnline();
    if (res.status < 300 && res.body && isCacheable(method, path)) remember(path, res.body);
    if (res.status >= 400) throw errorFromBody(res.status, res.body, `hub replied ${res.status}`);
    if (!res.body || !res.body.trim()) return undefined as T;
    // Shopping list changes still waiting on this phone stay on the list.
    const text = method === "GET" && path === "/api/shopping" ? withWaiting(res.body) : res.body;
    return JSON.parse(text) as T;
  }

  if (mode.api_base === null) throw new ApiError(0, "not connected to a hub");
  const res = await fetch(mode.api_base + path, {
    method,
    headers: body !== undefined ? { "content-type": "application/json" } : undefined,
    body,
  });
  const text = await res.text();
  if (!res.ok) throw errorFromBody(res.status, text, res.statusText);
  // 202/204 and other answers without a body carry no data.
  if (!text.trim()) return undefined as T;
  return JSON.parse(text) as T;
}

/** Phones without the hub: the copy of a GET, or a shopping list change kept for later; null when neither applies. */
async function offlineAnswer(method: string, path: string, body: string | undefined): Promise<{ value: unknown } | null> {
  if (isCacheable(method, path)) {
    const cached = fromCache(path);
    if (cached !== null) return { value: JSON.parse(cached) };
  }
  if (isQueueable(method, path)) {
    // Throws when the change could not be stored on the phone: then it is not kept.
    const answer = await queue(method, path, body ?? null);
    return { value: answer ? JSON.parse(answer) : undefined };
  }
  return null;
}

// ---- phone-side link management ------------------------------------------

export function clientState(): Promise<LinkSummary> {
  return invoke<LinkSummary>("client_state");
}

export async function clientPair(payload: PairPayload, password: string, deviceName: string): Promise<LinkSummary> {
  return linked(await invoke<LinkSummary>("client_pair", { payload, password, deviceName }));
}

/**
 * Pair with a hub found on the network. The phone first checks with the
 * 6-digit code that the hub it reached is the laptop showing that code, and
 * only then sends the password (see client.rs, pair_found).
 */
export async function clientPairFound(hub: DiscoveredHub, code: string, password: string, deviceName: string): Promise<LinkSummary> {
  return linked(await invoke<LinkSummary>("client_pair_found", { host: hub.host, port: hub.port, code, password, deviceName }));
}

async function linked(link: LinkSummary): Promise<LinkSummary> {
  markOnline();
  // Back with the hub it left: shopping list changes set aside then are sent now.
  await adoptParked(link.hub_id).catch(() => 0);
  return link;
}

/**
 * Unlink this phone. It keeps nothing of the hub except shopping list changes
 * that still wait, which are set aside for that hub (see clearOffline).
 * Resolves to how many changes were set aside.
 */
export async function clientForget(): Promise<number> {
  const link = await clientState().catch(() => null);
  // A change on its way to the hub right now should arrive (or not) before
  // the rest is set aside, so it is not set aside and sent a second time.
  await settle(5000);
  const parked = await clearOffline(link?.hub_id ?? null, link?.hub_name ?? null);
  await invoke("client_forget");
  return parked;
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

/** Phones: send shopping list changes made while away from the hub. */
export async function flushOutbox(): Promise<void> {
  const mode = await getMode();
  if (mode.mode !== "client") return;
  await flush((method, path, body) => invoke<{ status: number; body: string }>("client_request", { method, path, body }));
}
