import { invoke } from "@tauri-apps/api/core";
import { getMode } from "./api";
import type { Key } from "./i18n";

export type ScanKind = "qr" | "product";
export type ScanResult = { ok: true; text: string } | { ok: false; reason: "denied" | "canceled" | "unavailable" };

/** What the phone app's scanner answers (plugins/scanner in the app). */
type Scanned = { text: string; format: string } | { canceled: true };

/** The codes printed on products (zxing-cpp's names), and QR codes. */
const PRODUCT_FORMATS = ["EAN_13", "EAN_8", "UPC_A", "UPC_E", "CODE_128", "CODE_39", "ITF", "QR_CODE"];

/** True where a camera scanner is available (the phone app). */
export async function canScan(): Promise<boolean> {
  try {
    const mode = await getMode();
    return mode.mode === "client";
  } catch {
    return false;
  }
}

/**
 * Open the camera and scan one code. The phone app asks for camera access
 * when it is not allowed yet; the scanner reads codes on the phone itself.
 */
export async function scanCode(kind: ScanKind, t: (k: Key) => string): Promise<ScanResult> {
  try {
    const r = await invoke<Scanned>("plugin:scanner|scan", {
      formats: kind === "qr" ? ["QR_CODE"] : PRODUCT_FORMATS,
      cancelLabel: t("cancel"),
      hint: t("scanHint"),
    });
    const text = "text" in r ? r.text.trim() : "";
    return text ? { ok: true, text } : { ok: false, reason: "canceled" };
  } catch (e) {
    const code = typeof e === "object" && e !== null && "code" in e ? (e as { code?: unknown }).code : undefined;
    return { ok: false, reason: code === "denied" ? "denied" : "unavailable" };
  }
}

/** Convenience: the scanned text, or null when nothing was scanned. */
export async function scan(kind: ScanKind, t: (k: Key) => string): Promise<string | null> {
  const r = await scanCode(kind, t);
  return r.ok ? r.text : null;
}
