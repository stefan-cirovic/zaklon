import { invoke } from "@tauri-apps/api/core";

export type AppMode = {
  mode: "hub" | "client";
  api_base: string | null;
  platform: string;
  version: string;
};

export type Status = {
  hub_id: string; hub_name: string; version: string; port: number; fingerprint: string;
  set_up: boolean; language: "en" | "sr";
  devices?: number; uptime_secs?: number; addresses?: string[]; root?: string;
};

export type Device = { id: string; name: string; platform: string; created_at: string; last_seen: string | null };

export type PairStart = {
  code: string; expires_in_secs: number;
  payload: { v: number; hosts: string[]; port: number; fp: string; code: string; name: string; install_port: number };
};

let modePromise: Promise<AppMode> | null = null;

function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function getMode(): Promise<AppMode> {
  if (!modePromise) {
    modePromise = inTauri()
      ? invoke<AppMode>("app_mode")
      : Promise.resolve({
          mode: "hub",
          api_base: import.meta.env.VITE_API_BASE ?? "http://127.0.0.1:8481",
          platform: "browser",
          version: "dev",
        });
  }
  return modePromise;
}

export class ApiError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

/** Call the hub. On the laptop this goes to 127.0.0.1; on phones it will go through the pinned-TLS client. */
export async function api<T = unknown>(path: string, init: { method?: string; json?: unknown } = {}): Promise<T> {
  const mode = await getMode();
  if (mode.mode !== "hub" || !mode.api_base) {
    throw new ApiError(0, "not connected to a hub");
  }
  const res = await fetch(mode.api_base + path, {
    method: init.method ?? (init.json !== undefined ? "POST" : "GET"),
    headers: init.json !== undefined ? { "content-type": "application/json" } : undefined,
    body: init.json !== undefined ? JSON.stringify(init.json) : undefined,
  });
  if (!res.ok) {
    let msg = res.statusText;
    try { msg = ((await res.json()) as { error?: string }).error ?? msg; } catch { /* plain text */ }
    throw new ApiError(res.status, msg);
  }
  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
}
