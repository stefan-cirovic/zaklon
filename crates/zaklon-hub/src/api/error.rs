//! Errors as the API sends them: a status, a sentence for people reading
//! logs, and a stable code the app translates.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

pub struct ApiError(pub(super) StatusCode, pub(super) String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        // The sentence is for people reading logs; the code is what the app
        // translates, so rewording a message can never break a translation.
        let code = error_code(self.0, &self.1);
        (self.0, Json(serde_json::json!({ "error": self.1, "code": code }))).into_response()
    }
}

/// Stable codes for the app to translate, found from the message; the first
/// entry the message contains wins. One table, next to where the messages
/// come from. The tests below keep it honest: every message gets its code,
/// every entry still matches a message, and the app translates every code.
const ERROR_CODES: &[(&str, &str)] = &[
    ("wrong household password", "wrong_password"),
    ("does not open this backup", "backup_wrong_password"),
    ("backup is not encrypted", "backup_not_encrypted"),
    ("backup is encrypted", "backup_needs_password"),
    ("backup key cannot be read", "backup_key_damaged"),
    ("pairing code is invalid or expired", "code_expired"),
    ("wrong pairing code", "wrong_code"),
    ("too many wrong attempts from this device", "device_blocked"),
    ("too many attempts", "too_many_attempts"),
    ("password must be at least", "password_too_short"),
    ("set a household password first", "not_set_up"),
    ("already set up", "already_set_up"),
    ("only the laptop can do this", "laptop_only"),
    ("request from another website", "cross_site"),
    ("for both world maps", "world_no_room"),
    ("not enough free disk space", "no_disk_space"),
    ("not enough space", "drive_full"),
    ("formatted as FAT32", "fat32"),
    ("checksum mismatch", "checksum"),
    ("expiry must be a date", "bad_date"),
    ("name is required", "name_required"),
    ("text is required", "text_required"),
    ("that name is reserved", "name_reserved"),
    ("that folder does not exist", "no_folder"),
    ("that file does not exist", "no_file"),
    ("pause the download first", "pause_first"),
    ("confirm the license", "license_not_confirmed"),
    ("no longer offered", "no_longer_offered"),
    ("no newer world map", "no_world_update"),
    ("no longer at its download address", "download_gone"),
    ("could not delete", "delete_failed"),
    ("not a Zaklon backup", "not_a_backup"),
    ("backup is incomplete", "not_a_backup"),
    ("database is damaged", "not_a_backup"),
    ("settings are damaged", "not_a_backup"),
    ("key is damaged", "not_a_backup"),
    ("backup is too large", "not_a_backup"),
    ("made by a newer Zaklon", "newer_backup"),
    ("a copy is already running", "copy_running"),
    ("writing to the drive", "drive_write"),
    ("outside the library", "outside_library"),
    ("nothing selected", "nothing_selected"),
    ("no AI model is installed", "no_model"),
    ("AI engine is not installed", "no_ai_engine"),
    ("stopped while loading the model", "ai_memory"),
    ("needs more memory than this computer has", "ai_too_big"),
    ("not enough free memory for the AI", "ai_low_memory"),
    ("AI engine was stopped", "ai_stopped"),
    ("assistant is busy", "ai_busy"),
    ("question is too long", "question_too_long"),
    ("conversation is too long", "conversation_full"),
    ("ask something first", "question_empty"),
    ("note is too long", "note_too_long"),
    ("remembers too much", "notes_full"),
    ("model is not installed on the hub", "model_not_on_hub"),
    ("is not installed", "not_installed"),
    ("cannot be copied", "cannot_copy"),
    ("delta must be", "bad_quantity"),
    ("quantity must be", "bad_quantity"),
    ("several batches", "several_batches"),
    ("unknown category", "bad_category"),
    ("bad barcode", "bad_barcode"),
    ("home location needs", "bad_location"),
    ("no such", "not_found"),
    ("no file", "not_found"),
    ("unauthorized", "unauthorized"),
    ("internal error", "internal"),
];

pub fn error_code(status: StatusCode, msg: &str) -> &'static str {
    if let Some((_, code)) = ERROR_CODES.iter().find(|(needle, _)| msg.contains(needle)) {
        return code;
    }
    match status {
        StatusCode::NOT_FOUND => "not_found",
        StatusCode::UNAUTHORIZED => "unauthorized",
        StatusCode::FORBIDDEN => "forbidden",
        StatusCode::TOO_MANY_REQUESTS => "too_many_attempts",
        s if s.is_server_error() => "internal",
        _ => "other",
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        tracing::error!("internal error: {e:#}");
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, "internal error".into())
    }
}

pub(super) fn bad(msg: &str) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}
pub(super) fn forbidden(msg: &str) -> ApiError {
    ApiError(StatusCode::FORBIDDEN, msg.into())
}
pub(super) fn unauthorized() -> ApiError {
    ApiError(StatusCode::UNAUTHORIZED, "unauthorized".into())
}
pub(super) fn not_found(msg: &str) -> ApiError {
    ApiError(StatusCode::NOT_FOUND, msg.into())
}

/// Validation problems from the storage layer are the caller's fault.
/// A failure from the household data: a problem with what was asked (a
/// missing name, a bad date) is the asker's to fix; a failure of the database
/// or the disk is ours. Decided by the kind of error, not by its wording.
pub(super) fn invalid(e: anyhow::Error) -> ApiError {
    let ours = e.chain().any(|c| c.downcast_ref::<zaklon_core::rusqlite::Error>().is_some() || c.downcast_ref::<std::io::Error>().is_some());
    if ours {
        ApiError::from(e)
    } else {
        bad(&e.to_string())
    }
}

#[cfg(test)]
mod error_code_tests {
    use super::*;

    const BAD: StatusCode = StatusCode::BAD_REQUEST;
    const FORBIDDEN: StatusCode = StatusCode::FORBIDDEN;
    const NOT_FOUND: StatusCode = StatusCode::NOT_FOUND;

    /// One source file split for `hub_sources`: its code before its first test
    /// module (`#[cfg(test)] mod name { .. }`; test modules sit at the end of
    /// every file here), and the names of the test modules it keeps in files
    /// of their own (`#[cfg(test)] mod name;`).
    fn split_off_tests(text: &str) -> (&str, Vec<&str>) {
        const TEST_ONLY: &str = "#[cfg(test)]";
        let mut test_files = Vec::new();
        for (at, _) in text.match_indices(TEST_ONLY) {
            // A test-only function or import is not a module; look further.
            let Some(module) = text[at + TEST_ONLY.len()..].trim_start().strip_prefix("mod ") else { continue };
            let (name, rest) = module.split_at(module.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(module.len()));
            match rest.trim_start().chars().next() {
                Some('{') => return (&text[..at], test_files),
                Some(';') => test_files.push(name),
                _ => {}
            }
        }
        (text, test_files)
    }

    /// The source of the hub and its core without test code and without the
    /// code table, so a message counts as sent only if the hub really sends
    /// it, not if only a test mentions it. Folders inside (such as src/api)
    /// are read too.
    fn hub_sources() -> String {
        use std::path::{Path, PathBuf};
        let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut files: Vec<(PathBuf, String)> = Vec::new();
        let mut dirs: Vec<PathBuf> = ["zaklon-core/src", "zaklon-hub/src"].iter().map(|d| crates.join(d)).collect();
        while let Some(dir) = dirs.pop() {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                if e.path().is_dir() {
                    dirs.push(e.path());
                } else {
                    let text = std::fs::read_to_string(e.path()).unwrap();
                    files.push((e.path(), text));
                }
            }
        }
        // Files that are test modules as a whole, such as assistant/test_util.rs.
        let mut test_files: Vec<PathBuf> = Vec::new();
        for (path, text) in &files {
            let module_dir = match path.file_name().and_then(|n| n.to_str()) {
                Some("mod.rs" | "lib.rs" | "main.rs") => path.parent().unwrap().to_path_buf(),
                _ => path.with_extension(""),
            };
            test_files.extend(split_off_tests(text).1.iter().map(|name| module_dir.join(format!("{name}.rs"))));
        }
        let mut all = String::new();
        for (path, text) in &files {
            if test_files.contains(path) {
                continue;
            }
            let code = split_off_tests(text).0;
            match code.find("const ERROR_CODES") {
                Some(table) => {
                    all.push_str(&code[..table]);
                    all.push_str(&code[table + code[table..].find("];").unwrap()..]);
                }
                None => all.push_str(code),
            }
        }
        all
    }

    /// Test code is left out, so a message only a test mentions does not
    /// count as sent: test modules at the end of a file, and whole files that
    /// are test modules.
    #[test]
    fn hub_sources_leave_out_test_code() {
        let file = "fn sends() { bad(\"a real message\") }\n#[cfg(test)]\nfn helper() {}\n#[cfg(test)]\nmod helpers;\nfn more() {}\n\
                    #[cfg(test)]\nmod tests {\n    fn t() { bad(\"only a test says this\") }\n}\n";
        let (code, test_files) = split_off_tests(file);
        assert!(code.contains("a real message") && code.contains("fn helper") && code.contains("fn more"), "{code}");
        assert!(!code.contains("only a test says this"), "{code}");
        assert_eq!(test_files, ["helpers"]);

        let sources = hub_sources();
        assert!(sources.contains("fn error_code("), "the hub's code is read");
        assert!(!sources.contains("fn every_message_gets_its_code"), "a test module was read");
        assert!(!sources.contains("Helpers shared by the assistant's tests"), "assistant/test_util.rs was read");
        assert!(!sources.contains("(\"wrong household password\", \"wrong_password\")"), "the code table was read");
    }

    /// Every code the hub can send: the table's, and those that come from
    /// the status alone. "other" is not one: the app then shows the message.
    fn hub_codes() -> Vec<&'static str> {
        let by_status = [NOT_FOUND, StatusCode::UNAUTHORIZED, FORBIDDEN, StatusCode::TOO_MANY_REQUESTS, StatusCode::INTERNAL_SERVER_ERROR]
            .map(|s| error_code(s, ""));
        let mut codes: Vec<&str> = ERROR_CODES.iter().map(|(_, c)| *c).chain(by_status).collect();
        codes.sort_unstable();
        codes.dedup();
        codes
    }

    #[test]
    fn every_message_gets_its_code() {
        // Messages the hub sends, as written where they are made (the fixed
        // part of a message built with format!), and the code each must get.
        let messages = [
            (BAD, "password must be at least 8 characters", "password_too_short"),
            (BAD, "already set up; use /api/password to change the password", "already_set_up"),
            (BAD, "set a household password first", "not_set_up"),
            (FORBIDDEN, "pairing code is invalid or expired", "code_expired"),
            (FORBIDDEN, "too many attempts; start pairing again on the laptop", "too_many_attempts"),
            (FORBIDDEN, "wrong household password", "wrong_password"),
            (FORBIDDEN, "wrong pairing code", "wrong_code"),
            (BAD, "bad pairing message", "other"),
            (StatusCode::TOO_MANY_REQUESTS, "too many wrong attempts from this device; try again in a few minutes", "device_blocked"),
            (FORBIDDEN, "only the laptop can do this", "laptop_only"),
            (FORBIDDEN, "request from another website", "cross_site"),
            (StatusCode::UNAUTHORIZED, "unauthorized", "unauthorized"),
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error", "internal"),
            (BAD, "name is required", "name_required"),
            (BAD, "that name is reserved", "name_reserved"),
            (BAD, "that folder does not exist", "no_folder"),
            (BAD, "that file does not exist", "no_file"),
            (BAD, "bad barcode", "bad_barcode"),
            (BAD, "delta must be a non-zero number", "bad_quantity"),
            (BAD, "delta must be a number", "bad_quantity"),
            (BAD, "quantity must be more than zero", "bad_quantity"),
            (BAD, "unknown category", "bad_category"),
            (BAD, "expiry must be a date like 2027-03-31", "bad_date"),
            (BAD, "this item has several batches; change the date of a batch instead", "several_batches"),
            (BAD, "text is required", "text_required"),
            (BAD, "the note is too long", "note_too_long"),
            (BAD, "the assistant remembers too much already; delete some notes first", "notes_full"),
            (NOT_FOUND, "no such item", "not_found"),
            (BAD, "no such tool", "not_found"),
            (NOT_FOUND, "no such model", "not_found"),
            (NOT_FOUND, "no file", "not_found"),
            (NOT_FOUND, "model is not installed on the hub", "model_not_on_hub"),
            (BAD, "this is not a Zaklon backup", "not_a_backup"),
            (BAD, "the backup is incomplete", "not_a_backup"),
            (BAD, "the backup's database is damaged", "not_a_backup"),
            (BAD, "the backup's settings are damaged", "not_a_backup"),
            (BAD, "the backup's key is damaged", "not_a_backup"),
            (BAD, "this backup was made by a newer Zaklon; update first", "newer_backup"),
            (BAD, "the password does not open this backup", "backup_wrong_password"),
            (BAD, "this backup is encrypted; enter the household password", "backup_needs_password"),
            (BAD, "this backup is not encrypted, so nothing shows whether it was changed; confirm to restore it anyway", "backup_not_encrypted"),
            (BAD, "this backup is too large for a household's data", "not_a_backup"),
            (NOT_FOUND, "no such endpoint here; phones pair over the network", "not_found"),
            (BAD, "this hub's backup key cannot be read; turn backup encryption on again", "backup_key_damaged"),
            (BAD, "wrong household password", "wrong_password"),
            (BAD, "a copy is already running", "copy_running"),
            (BAD, "nothing selected", "nothing_selected"),
            (BAD, "choose a folder outside the library", "outside_library"),
            (BAD, "} is not installed", "not_installed"),
            (BAD, "} cannot be copied", "cannot_copy"),
            (BAD, "not enough space: ", "drive_full"),
            (BAD, "this drive is formatted as FAT32, which cannot hold files of 4 GB or more; format it as exFAT or NTFS", "fat32"),
            (BAD, "writing to the drive: ", "drive_write"),
            (BAD, "pause the download first", "pause_first"),
            (BAD, "confirm the license of this pack first", "license_not_confirmed"),
            (BAD, "this pack is no longer offered", "no_longer_offered"),
            (BAD, "could not delete ", "delete_failed"),
            (BAD, "not enough free disk space", "no_disk_space"),
            (BAD, "not enough free disk space for both world maps; remove the old one first", "world_no_room"),
            (BAD, "no newer world map is offered", "no_world_update"),
            (BAD, "the file is no longer at its download address", "download_gone"),
            (BAD, "checksum mismatch", "checksum"),
            (BAD, "no AI model is installed", "no_model"),
            (BAD, "the AI engine is not installed", "no_ai_engine"),
            (BAD, "the AI engine stopped while loading the model", "ai_memory"),
            (BAD, "this AI model needs more memory than this computer has; choose a smaller model", "ai_too_big"),
            (BAD, "not enough free memory for the AI right now; close some programs and try again", "ai_low_memory"),
            (BAD, "the AI engine was stopped", "ai_stopped"),
            (BAD, "the assistant is busy with other questions; try again in a moment", "ai_busy"),
            (BAD, "the question is too long", "question_too_long"),
            (BAD, "the conversation is too long; start a new one", "conversation_full"),
            (NOT_FOUND, "no such conversation", "not_found"),
            (BAD, "the outcome must be done or canceled", "other"),
            (BAD, "ask something first", "question_empty"),
            (BAD, "a home location needs a latitude between -90 and 90", "bad_location"),
            (BAD, "a home location needs a longitude between -180 and 180", "bad_location"),
        ];
        let sources = hub_sources();
        for (status, msg, code) in messages {
            assert!(sources.contains(msg), "the hub no longer sends {msg:?}; update this list");
            assert_eq!(error_code(status, msg), code, "{msg}");
        }
        assert_eq!(error_code(NOT_FOUND, "no such note"), "not_found");
        assert_eq!(error_code(BAD, "something new"), "other");
    }

    /// An entry whose message the hub no longer sends is dead weight, and a
    /// trap: it can catch some future message by accident.
    #[test]
    fn every_table_entry_matches_a_message_the_hub_sends() {
        let sources = hub_sources();
        let dead: Vec<&str> = ERROR_CODES.iter().map(|(needle, _)| *needle).filter(|n| !sources.contains(n)).collect();
        assert!(dead.is_empty(), "no message contains {dead:?}");
    }

    /// The app translates every code the hub can send (`CODES` in
    /// ui/src/errors.ts); a missing one would show the English message.
    #[test]
    fn the_app_translates_every_code() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/src/errors.ts");
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        let table = &text[text.find("const CODES").expect("errors.ts has a CODES table")..];
        let table = &table[table.find('{').unwrap() + 1..table.find("};").unwrap()];
        let ui: Vec<&str> = table
            .lines()
            .map(|l| l.split("//").next().unwrap_or_default())
            .flat_map(|l| l.split(','))
            .filter_map(|entry| entry.split_once(':'))
            .map(|(key, _)| key.trim().trim_matches(|c| c == '"' || c == '\''))
            .collect();
        let missing: Vec<&str> = hub_codes().into_iter().filter(|c| !ui.contains(c)).collect();
        assert!(missing.is_empty(), "add these codes to CODES in ui/src/errors.ts: {missing:?}");
    }

    #[test]
    fn user_mistakes_are_400_and_our_failures_500() {
        let user = invalid(anyhow::anyhow!("any wording at all"));
        assert_eq!(user.0, StatusCode::BAD_REQUEST);
        let disk = invalid(anyhow::Error::new(std::io::Error::other("disk gone")));
        assert_eq!(disk.0, StatusCode::INTERNAL_SERVER_ERROR);
        let db = invalid(anyhow::Error::new(zaklon_core::rusqlite::Error::InvalidQuery));
        assert_eq!(db.0, StatusCode::INTERNAL_SERVER_ERROR);
    }
}
