//! What the hub tells about its own build in the system specification
//! (Settings › About): the day it was built and the commit, the Rust
//! compiler, and the Tauri version the desktop and phone apps are built
//! with (from the workspace's Cargo.lock). Nothing here is typed by hand.
//!
//! `ZAKLON_BUILD_DATE` (YYYY-MM-DD) and `ZAKLON_BUILD_COMMIT` set the first
//! two; scripts/build-all.sh and the release workflow set them. Without
//! them the commit comes from git (the hub is built again when it changes)
//! and the date is the day this script last ran.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let lock = manifest.join("..").join("..").join("Cargo.lock");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", lock.display());
    println!("cargo:rerun-if-env-changed=ZAKLON_BUILD_DATE");
    println!("cargo:rerun-if-env-changed=ZAKLON_BUILD_COMMIT");

    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    // "rustc 1.90.0 (1159e78c4 2025-09-14)"
    let rust = output(Command::new(rustc).arg("--version")).and_then(|v| v.split_whitespace().nth(1).map(str::to_string));
    let tauri = std::fs::read_to_string(&lock).ok().and_then(|l| locked_version(&l, "tauri"));
    let commit = given("ZAKLON_BUILD_COMMIT", |c| c.chars().all(|ch| ch.is_ascii_hexdigit()))
        .or_else(|| git_commit(&manifest))
        .map(|c| c.chars().take(12).collect::<String>().to_lowercase());
    let date = given("ZAKLON_BUILD_DATE", |d| d.len() == 10 && d.chars().all(|ch| ch.is_ascii_digit() || ch == '-')).unwrap_or_else(today);
    for (name, value) in [
        ("ZAKLON_BUILT_ON", Some(date)),
        ("ZAKLON_BUILT_FROM", commit),
        ("ZAKLON_RUST_VERSION", rust),
        ("ZAKLON_TAURI_VERSION", tauri),
    ] {
        println!("cargo:rustc-env={name}={}", value.unwrap_or_default());
    }
}

/// A variable from the environment that looks as it should (else ignored).
fn given(name: &str, ok: impl Fn(&str) -> bool) -> Option<String> {
    std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty() && ok(v))
}

/// What a program printed, trimmed; None when it could not run or failed.
fn output(cmd: &mut Command) -> Option<String> {
    let out = cmd.output().ok().filter(|o| o.status.success())?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
}

/// The version of package `name` in a Cargo.lock ("version" follows "name").
fn locked_version(lock: &str, name: &str) -> Option<String> {
    let wanted = format!("name = \"{name}\"");
    let mut lines = lock.lines();
    while let Some(line) = lines.next() {
        if line.trim() == wanted {
            let version = lines.next()?.trim();
            return version.strip_prefix("version = \"")?.strip_suffix('"').map(str::to_string);
        }
    }
    None
}

/// The commit checked out, and the files that change with it watched, so
/// the hub is built again after a new commit or a checkout.
fn git_commit(dir: &Path) -> Option<String> {
    let git = |args: &[&str]| output(Command::new("git").arg("-C").arg(dir).args(args));
    let mut watched = vec!["HEAD".to_string(), "packed-refs".to_string()];
    watched.extend(git(&["symbolic-ref", "-q", "HEAD"]));
    for name in watched {
        if let Some(path) = git(&["rev-parse", "--path-format=absolute", "--git-path", name.as_str()]).filter(|p| Path::new(p).is_file()) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    git(&["rev-parse", "HEAD"])
}

/// Today (UTC) as YYYY-MM-DD, from the days since 1970 (H. Hinnant's
/// civil_from_days).
fn today() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}
