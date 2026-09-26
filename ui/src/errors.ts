import type { Key } from "./i18n";

// Server and network errors arrive as English text. Show them in the user's
// language; anything unknown gets a translated generic message plus the
// original text in brackets, so it can still be reported.

const KNOWN: [RegExp, Key][] = [
  [/wrong household password/i, "errWrongPassword"],
  [/pairing code is invalid or expired/i, "errCodeExpired"],
  [/too many attempts/i, "errTooManyAttempts"],
  [/password must be at least 8/i, "passwordRule"],
  [/only the laptop can do this/i, "errLaptopOnly"],
  [/^unauthorized$/i, "errUnauthorized"],
  [/no answer|timed out|not reachable|failed to fetch|networkerror|load failed|hub has no known addresses/i, "errUnreachable"],
  [/not enough free disk space/i, "errDisk"],
  [/battery below/i, "batteryRule"],
  [/checksum mismatch/i, "errChecksum"],
  [/expiry must be a date/i, "errBadDate"],
  [/name is required|text is required/i, "errNameRequired"],
  [/that folder does not exist/i, "errNoFolder"],
  [/pause the download first/i, "errPauseFirst"],
  [/web page, not the pack file|cannot resume|no data for 60 seconds|download failed|server replied/i, "errDownload"],
  [/could not delete/i, "errDelete"],
  [/not paired with a hub/i, "errNotPaired"],
  [/not a zaklon pairing code/i, "notAPairingCode"],
  [/fingerprint/i, "errFingerprint"],
];

export function tErr(t: (k: Key) => string, message: string | null | undefined): string {
  const msg = String(message ?? "").trim();
  if (!msg) return t("errGeneric");
  for (const [re, key] of KNOWN) {
    if (re.test(msg)) return t(key);
  }
  return `${t("errGeneric")} (${msg})`;
}

export function errText(t: (k: Key) => string, e: unknown): string {
  if (e instanceof Error) return tErr(t, e.message);
  return tErr(t, String(e));
}
