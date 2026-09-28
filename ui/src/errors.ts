import { tLang, type Key } from "./i18n";

// Server and network errors arrive as English text. Show them in the user's
// language; anything unknown gets a translated generic message. In English the
// original text follows in brackets, so it can still be reported; in other
// languages it would only be foreign words.

const KNOWN: [RegExp, Key][] = [
  [/could not save on this phone/i, "errSaveOnPhone"],
  [/wrong household password/i, "errWrongPassword"],
  [/pairing code is invalid or expired/i, "errCodeExpired"],
  [/wrong pairing code/i, "errWrongCode"],
  [/answered in place of the hub/i, "errNotTheHub"],
  [/too many wrong attempts from this device/i, "errDeviceBlocked"],
  [/too many attempts/i, "errTooManyAttempts"],
  [/password must be at least 8/i, "passwordRule"],
  [/only the laptop can do this/i, "errLaptopOnly"],
  [/^unauthorized$/i, "errUnauthorized"],
  [/no answer|timed out|not reachable|failed to fetch|networkerror|load failed|hub has no known addresses/i, "errUnreachable"],
  [/not enough free disk space/i, "errDisk"],
  [/battery below/i, "batteryRule"],
  [/checksum mismatch/i, "errChecksum"],
  [/damaged or another version/i, "errFileMismatch"],
  [/connect it and import again/i, "errImportGone"],
  [/expiry must be a date/i, "errBadDate"],
  [/name is required/i, "errNameRequired"],
  [/text is required/i, "errTextRequired"],
  [/that folder does not exist/i, "errNoFolder"],
  [/that file does not exist/i, "errNoFile"],
  [/not a Zaklon backup|backup is incomplete|backup's (database|settings|key) (is|are) damaged/i, "errNotBackup"],
  [/does not open this backup/i, "errBackupPassword"],
  [/backup is not encrypted/i, "restoreUnencrypted"],
  [/backup is encrypted/i, "errBackupNeedsPassword"],
  [/backup key cannot be read/i, "errBackupKeyDamaged"],
  [/made by a newer Zaklon/i, "errNewerBackup"],
  [/no AI model is installed/i, "aiNeedsModel"],
  [/AI engine is not installed/i, "errAiEngineMissing"],
  [/stopped while loading the model/i, "errAiMemory"],
  [/needs more memory than this computer has/i, "errAiTooBig"],
  [/not enough free memory for the AI/i, "errAiLowMemory"],
  [/needs more memory than this phone has/i, "errAiTooBigPhone"],
  [/not enough free memory on this phone/i, "errAiLowMemoryPhone"],
  [/AI is still loading/i, "errAiLoading"],
  [/answer took too long/i, "errAiTooLong"],
  [/did not start in time|could not start the AI engine|AI engine replied|AI engine returned an empty answer|stopped answering|^AI engine:/i, "errAiEngine"],
  [/ask something first/i, "askSomething"],
  [/question is too long/i, "errQuestionLong"],
  [/conversation is too long/i, "errConversationFull"],
  [/hub stopped before the answer was finished/i, "errAnswerLost"],
  [/assistant is busy/i, "errAiBusy"],
  [/note is too long/i, "errNoteLong"],
  [/remembers too much/i, "errNotesFull"],
  [/AI engine was stopped/i, "errAiStopped"],
  [/not enough space/i, "notEnoughSpace"],
  [/formatted as FAT32/i, "fat32Warn"],
  [/a copy is already running/i, "errCopyRunning"],
  [/^cancell?ed$/i, "errCancelled"],
  [/writing to the drive/i, "errDriveWrite"],
  [/outside the library/i, "errOutsideLibrary"],
  [/pause the download first/i, "errPauseFirst"],
  [/web page, not the pack file|cannot resume|no data for 60 seconds|download failed|server replied/i, "errDownload"],
  [/could not delete/i, "errDelete"],
  [/not paired with a hub/i, "errNotPaired"],
  [/not a zaklon pairing code/i, "notAPairingCode"],
  [/reinstalled or replaced/i, "hubChanged"],
  [/fingerprint/i, "errFingerprint"],
];

/**
 * The hub's error codes: every code in ERROR_CODES in the hub's api/error.rs, plus
 * the ones error_code() falls back to by status. A hub test reads this list
 * and fails when a code is missing. "errGeneric" means there are no better
 * words than "Something went wrong." (in English the hub's text follows).
 */
const CODES: Record<string, Key> = {
  not_set_up: "hubNotSetUp",
  already_set_up: "errAlreadySetUp",
  nothing_selected: "errNothingSelected",
  not_installed: "errPacksGone",
  cannot_copy: "errPacksGone",
  bad_quantity: "errBadQuantity",
  several_batches: "errSeveralBatches",
  bad_barcode: "errBadBarcode",
  bad_location: "errBadLocation",
  not_found: "errNotFound",
  bad_category: "errGeneric",
  internal: "errGeneric",
  forbidden: "errGeneric",
  other: "errGeneric",
  wrong_password: "errWrongPassword",
  code_expired: "errCodeExpired",
  wrong_code: "errWrongCode",
  too_many_attempts: "errTooManyAttempts",
  device_blocked: "errDeviceBlocked",
  cross_site: "errCrossSite",
  drive_full: "notEnoughSpace",
  text_required: "errTextRequired",
  name_reserved: "errNameReserved",
  model_not_on_hub: "errModelNotOnHub",
  password_too_short: "passwordRule",
  laptop_only: "errLaptopOnly",
  unauthorized: "errUnauthorized",
  no_disk_space: "errDisk",
  fat32: "fat32Warn",
  battery_low: "batteryRule",
  checksum: "errChecksum",
  bad_date: "errBadDate",
  name_required: "errNameRequired",
  no_folder: "errNoFolder",
  no_file: "errNoFile",
  pause_first: "errPauseFirst",
  delete_failed: "errDelete",
  not_a_backup: "errNotBackup",
  newer_backup: "errNewerBackup",
  backup_wrong_password: "errBackupPassword",
  backup_needs_password: "errBackupNeedsPassword",
  backup_not_encrypted: "restoreUnencrypted",
  backup_key_damaged: "errBackupKeyDamaged",
  copy_running: "errCopyRunning",
  drive_write: "errDriveWrite",
  outside_library: "errOutsideLibrary",
  no_model: "aiNeedsModel",
  no_ai_engine: "errAiEngineMissing",
  ai_memory: "errAiMemory",
  ai_too_big: "errAiTooBig",
  ai_low_memory: "errAiLowMemory",
  ai_stopped: "errAiStopped",
  ai_busy: "errAiBusy",
  question_too_long: "errQuestionLong",
  question_empty: "askSomething",
  conversation_full: "errConversationFull",
  note_too_long: "errNoteLong",
  notes_full: "errNotesFull",
};

/** " (the technical text)" in English; nothing in other languages, where it would be foreign. */
export function detail(t: (k: Key) => string, raw: string | null | undefined): string {
  const text = String(raw ?? "").trim();
  return text && tLang(t) === "en" ? ` (${text})` : "";
}

export function tErr(t: (k: Key) => string, message: string | null | undefined, code?: string): string {
  const mapped = code ? CODES[code] : undefined;
  if (mapped && mapped !== "errGeneric") return t(mapped);
  const msg = String(message ?? "").trim();
  if (!msg) return t("errGeneric");
  for (const [re, key] of KNOWN) {
    if (re.test(msg)) return t(key);
  }
  return t("errGeneric") + detail(t, msg);
}

/** The hub's code for an error, if it sent one. */
export function errCode(e: unknown): string | undefined {
  return e instanceof Error ? (e as { code?: string }).code : undefined;
}

export function errText(t: (k: Key) => string, e: unknown): string {
  if (e instanceof Error) return tErr(t, e.message, errCode(e));
  return tErr(t, String(e));
}
