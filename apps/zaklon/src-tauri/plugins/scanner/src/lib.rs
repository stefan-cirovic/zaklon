//! Barcode and QR code scanner for the Zaklon phone app.
//!
//! The Android side (`android/`) opens a full-screen camera view with CameraX
//! and reads codes with zxing-cpp, entirely on the phone: no Google Play
//! services and no network. The interface calls `plugin:scanner|scan`, which
//! Tauri hands straight to the Kotlin plugin, so there are no Rust commands.
//!
//! `scan` takes `{ formats?, cancelLabel?, hint? }` (zxing-cpp format names
//! such as "EAN_13" or "QR_CODE"; the two texts in the interface's language)
//! and resolves to `{ text, format }`, or to `{ canceled: true }` when the
//! person closes the camera. It is rejected with the code "denied" when camera
//! access is not allowed and "unavailable" when there is no usable camera.

use tauri::{
    plugin::{Builder, TauriPlugin},
    Runtime,
};

/// Package of the Kotlin plugin class.
#[cfg(target_os = "android")]
const ANDROID_PACKAGE: &str = "com.zaklon.scanner";

/// The scanner plugin. Only Android has an implementation; elsewhere `scan`
/// fails, and the interface shows its scan buttons on phones only.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("scanner")
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            _api.register_android_plugin(ANDROID_PACKAGE, "ScannerPlugin")?;
            Ok(())
        })
        .build()
}
