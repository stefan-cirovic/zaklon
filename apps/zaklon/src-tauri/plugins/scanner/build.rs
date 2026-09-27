/// The one command the interface calls. It has no Rust code: Tauri hands it to
/// the Kotlin plugin in `android/`. Listing it gives it an `allow-scan`
/// permission, which the phone's capability grants.
const COMMANDS: &[&str] = &["scan"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).android_path("android").build();
}
