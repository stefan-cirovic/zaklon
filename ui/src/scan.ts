import { getMode } from "./api";

export type ScanKind = "qr" | "product";
export type ScanResult = { ok: true; text: string } | { ok: false; reason: "denied" | "cancelled" | "unavailable" };

/** True where a camera scanner is available (the phone app). */
export async function canScan(): Promise<boolean> {
  try {
    const mode = await getMode();
    return mode.mode === "client";
  } catch {
    return false;
  }
}

/** Open the camera and scan one code. */
export async function scanCode(kind: ScanKind): Promise<ScanResult> {
  try {
    const { scan, Format, checkPermissions, requestPermissions } = await import("@tauri-apps/plugin-barcode-scanner");
    let perm = await checkPermissions();
    if (perm !== "granted") perm = await requestPermissions();
    if (perm !== "granted") return { ok: false, reason: "denied" };
    const formats =
      kind === "qr"
        ? [Format.QRCode]
        : [Format.EAN13, Format.EAN8, Format.UPC_A, Format.UPC_E, Format.Code128, Format.Code39, Format.ITF, Format.QRCode];
    const result = await scan({ windowed: false, formats });
    const text = result.content?.trim();
    return text ? { ok: true, text } : { ok: false, reason: "cancelled" };
  } catch {
    return { ok: false, reason: "cancelled" };
  }
}

/** Convenience: the scanned text, or null when nothing was scanned. */
export async function scan(kind: ScanKind): Promise<string | null> {
  const r = await scanCode(kind);
  return r.ok ? r.text : null;
}
