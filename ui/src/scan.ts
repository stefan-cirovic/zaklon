import { getMode } from "./api";

export type ScanKind = "qr" | "product";

/** True where a camera scanner is available (the phone app). */
export async function canScan(): Promise<boolean> {
  const mode = await getMode();
  return mode.mode === "client";
}

/**
 * Open the camera and return the scanned text, or null when the person
 * cancels or does not allow the camera.
 */
export async function scan(kind: ScanKind): Promise<string | null> {
  const { scan, Format, checkPermissions, requestPermissions } = await import("@tauri-apps/plugin-barcode-scanner");
  let perm = await checkPermissions();
  if (perm !== "granted") perm = await requestPermissions();
  if (perm !== "granted") return null;
  const formats =
    kind === "qr"
      ? [Format.QRCode]
      : [Format.EAN13, Format.EAN8, Format.UPC_A, Format.UPC_E, Format.Code128, Format.Code39, Format.ITF, Format.QRCode];
  try {
    const result = await scan({ windowed: false, formats });
    return result.content?.trim() || null;
  } catch {
    return null;
  }
}
