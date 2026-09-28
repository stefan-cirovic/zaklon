use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Every command the interface may call. Listing them gives each one an
/// `allow-<command>` permission; the capabilities grant them per window and
/// origin (the desktop window loads the hub's own page from 127.0.0.1, which
/// Tauri treats as a remote origin that needs explicit permissions).
const COMMANDS: &[&str] = &[
    "app_mode",
    "app_restart",
    "client_state",
    "client_pair",
    "client_pair_found",
    "client_request",
    "client_forget",
    "client_discover",
    "client_content_base",
    "client_fetch_app",
    "client_map_local",
    "outbox_read",
    "outbox_write",
    "local_ai_status",
    "local_ai_copy",
    "local_ai_start",
    "local_ai_stop",
    "local_ai_delete",
    "local_ai_ask",
];

/// The map's assets the phone app carries, so a phone away from home still
/// has the world overview: the overview, the regular font's glyphs (they
/// stand in for the other fonts) and the style's icons. Made by
/// scripts/fetch-map-assets.sh into windows/map-assets, or wherever
/// ZAKLON_MAP_ASSETS_BUNDLE points. Only the phone app carries them (the hub
/// on a laptop has its own); without them the phone shows the map only at home.
fn bundle_map_assets() {
    println!("cargo:rerun-if-env-changed=ZAKLON_MAP_ASSETS_BUNDLE");
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let dir = std::env::var_os("ZAKLON_MAP_ASSETS_BUNDLE").map(PathBuf::from).unwrap_or_else(|| manifest.join("windows").join("map-assets"));
    println!("cargo:rerun-if-changed={}", dir.join("overview.pmtiles").display());
    let phone = matches!(std::env::var("CARGO_CFG_TARGET_OS").as_deref(), Ok("android" | "ios"));
    let overview = dir.join("overview.pmtiles");
    let mut code = String::from("// Made by build.rs: the map's assets the phone app carries.\n");
    let files = |sub: &Path, ext: &[&str]| -> Vec<(String, PathBuf)> {
        let mut out: Vec<(String, PathBuf)> = std::fs::read_dir(sub)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_file() && p.extension().and_then(|x| x.to_str()).is_some_and(|x| ext.contains(&x)))
                    .map(|p| (p.file_name().unwrap().to_string_lossy().into_owned(), p))
                    .collect()
            })
            .unwrap_or_default();
        out.sort();
        out
    };
    if phone && overview.is_file() {
        let _ = writeln!(code, "pub static OVERVIEW: &[u8] = include_bytes!({:?});", overview.display().to_string());
        let glyphs = files(&dir.join("fonts").join("Noto Sans Regular"), &["pbf"]);
        let sprites = files(&dir.join("sprites"), &["json", "png"]);
        for (name, list) in [("GLYPHS", glyphs), ("SPRITES", sprites)] {
            let _ = writeln!(code, "pub static {name}: &[(&str, &[u8])] = &[");
            for (file, path) in list {
                let _ = writeln!(code, "    ({file:?}, include_bytes!({:?}) as &[u8]),", path.display().to_string());
            }
            code.push_str("];\n");
        }
    } else {
        code.push_str("pub static OVERVIEW: &[u8] = &[];\npub static GLYPHS: &[(&str, &[u8])] = &[];\npub static SPRITES: &[(&str, &[u8])] = &[];\n");
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("out dir")).join("map_assets.rs");
    std::fs::write(out, code).expect("writing map_assets.rs");
}

fn main() {
    bundle_map_assets();
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run the Tauri build script");
}
