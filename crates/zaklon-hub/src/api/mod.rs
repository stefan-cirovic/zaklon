//! JSON API shared by the desktop window (over 127.0.0.1) and paired phones
//! (over pinned TLS). A caller is either *Local* (the laptop itself) or a
//! *Device* presenting its bearer token. Physical access to the laptop is
//! trust, so Local can do everything, including first-run setup.
//!
//! The router is here, and the handlers in a file for each area. Errors and
//! their codes are in `error`, who is asking in `auth`.

mod admin;
mod assistant;
mod auth;
mod basemap;
mod error;
mod home;
mod household;
mod maps;
mod memory;
mod packs;
mod pairing;
mod power;
mod spec;
mod supplies;
mod water;

use std::sync::Arc;

use axum::{
    routing::{get, post},
    Router,
};
use tower_http::trace::TraceLayer;

use crate::HubState;

pub use auth::{Caller, LibraryReader, Local};
pub use error::{error_code, ApiError};
pub use pairing::Paired;

use admin::{
    backups_create, backups_encryption, backups_list, backups_restore, export_cancel, export_start, export_status, firewall_allow,
    firewall_private, firewall_status, hotspot_start, hotspot_status, hotspot_stop, updates_check, updates_settings, updates_state,
};
use assistant::{
    assistant_answer, assistant_ask, assistant_cancel, assistant_overview, assistant_select, assistant_stop, assistant_warm,
    conversations_create, conversations_delete, conversations_get, conversations_list, conversations_outcome, conversations_rename,
    conversations_send,
};
use household::{change_password, delete_device, list_devices, me, pin_tool, pinned_tool, rename_device, setup, status};
use maps::{maps_app_file, maps_country_download, maps_country_remove, maps_overview};
use memory::{memory_add, memory_delete, memory_list};
use packs::{
    catalog, drives, hardware, kiwix_proxy, kiwix_proxy_latin, library_books, library_search, model_file, models_list, pack_download,
    pack_pause, pack_remove, packs_import, system, world_check, world_update,
};
use pairing::{network_only, pair_complete, pair_start, pake_finish, pake_start};
use power::{power_plan, power_plan_save};
use supplies::{
    barcode_lookup, batch_add, batch_delete, batch_update, history, items_adjust, items_create, items_delete, items_get, items_list,
    items_update, places_add, places_delete, places_list, put_away, put_away_list, shopping_add, shopping_bought, shopping_dismiss,
    shopping_list, supplies_summary,
};
use water::{water_plan, water_plan_save};

/// Which listener a request came in on. Laptop trust exists only on the
/// loopback listener used by the desktop window; the network (TLS) listener
/// always requires a device token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Listener {
    Local,
    Network,
}

pub fn router(state: Arc<HubState>, listener: Listener) -> Router {
    // A phone pairs over the network (TLS) listener. The laptop never pairs
    // with itself, so on its loopback listener these do not exist: a web page
    // open in the laptop's browser cannot reach them there either (DNS
    // rebinding), with or without a code on the screen.
    let pairing = match listener {
        Listener::Network => Router::new()
            .route("/api/pair/complete", post(pair_complete))
            .route(zaklon_pake::START_PATH, post(pake_start))
            .route(zaklon_pake::FINISH_PATH, post(pake_finish)),
        Listener::Local => Router::new()
            .route("/api/pair/complete", post(network_only))
            .route(zaklon_pake::START_PATH, post(network_only))
            .route(zaklon_pake::FINISH_PATH, post(network_only)),
    };
    Router::new()
        .merge(pairing)
        .route("/api/status", get(status))
        .route("/api/setup", post(setup))
        .route("/api/password", post(change_password))
        .route("/api/pair/start", post(pair_start))
        .route("/api/devices", get(list_devices))
        .route("/api/devices/{id}", axum::routing::patch(rename_device).delete(delete_device))
        .route("/api/me", get(me))
        .route("/api/catalog", get(catalog))
        .route("/api/system", get(system))
        .route("/api/system/spec", get(spec::system_spec))
        .route("/api/packs/import", post(packs_import))
        .route("/api/packs/{id}", axum::routing::delete(pack_remove))
        .route("/api/packs/{id}/download", post(pack_download))
        .route("/api/packs/{id}/pause", post(pack_pause))
        .route("/api/world-map/check", post(world_check))
        .route("/api/world-map/update", post(world_update))
        .route("/api/export", get(export_status).post(export_start))
        .route("/api/export/cancel", post(export_cancel))
        .route("/api/drives", get(drives))
        .route("/api/firewall", get(firewall_status))
        .route("/api/firewall/allow", post(firewall_allow))
        .route("/api/firewall/private", post(firewall_private))
        .route("/api/hotspot", get(hotspot_status))
        .route("/api/hotspot/start", post(hotspot_start))
        .route("/api/hotspot/stop", post(hotspot_stop))
        .route("/api/updates", get(updates_state))
        .route("/api/updates/check", post(updates_check))
        .route("/api/updates/settings", post(updates_settings))
        .route("/api/pinned-tool", get(pinned_tool).post(pin_tool))
        .route("/api/power", get(power_plan).put(power_plan_save))
        .route("/api/backups", get(backups_list).post(backups_create))
        .route("/api/backups/restore", post(backups_restore))
        .route("/api/backups/encryption", post(backups_encryption))
        .route("/api/hardware", get(hardware))
        .route("/api/supplies/summary", get(supplies_summary))
        .route("/api/items", get(items_list).post(items_create))
        .route("/api/items/{id}", get(items_get).patch(items_update).delete(items_delete))
        .route("/api/items/{id}/adjust", post(items_adjust))
        .route("/api/barcodes/{code}", get(barcode_lookup))
        .route("/api/places", get(places_list).post(places_add))
        .route("/api/places/{id}", axum::routing::delete(places_delete))
        .route("/api/items/{id}/batches", post(batch_add))
        .route("/api/batches/{id}", axum::routing::patch(batch_update).delete(batch_delete))
        .route("/api/shopping", get(shopping_list).post(shopping_add))
        .route("/api/shopping/{id}/bought", post(shopping_bought))
        .route("/api/shopping/{id}/dismiss", post(shopping_dismiss))
        .route("/api/put-away", get(put_away_list))
        .route("/api/put-away/{id}", post(put_away))
        .route("/api/history", get(history))
        .route("/api/water", get(water_plan).put(water_plan_save))
        .route("/api/maps", get(maps_overview))
        .route("/api/maps-app", get(maps_app_file))
        .route("/api/maps/{country}/download", post(maps_country_download))
        .route("/api/maps/{country}", axum::routing::delete(maps_country_remove))
        .route("/api/map", get(basemap::map_info))
        .route("/api/map/places", get(home::places_search))
        .route("/api/home-location", get(home::home_get).put(home::home_set).delete(home::home_clear))
        .route("/tiles/{z}/{x}/{file}", get(basemap::tile))
        .route("/map/fonts/{fontstack}/{range}", get(basemap::glyphs))
        .route("/map/sprites/{file}", get(basemap::sprite))
        .route("/api/assistant", get(assistant_overview))
        .route("/api/assistant/model", post(assistant_select))
        .route("/api/assistant/ask", post(assistant_ask))
        .route("/api/assistant/answers/{id}", get(assistant_answer))
        .route("/api/assistant/answers/{id}/cancel", post(assistant_cancel))
        .route("/api/assistant/stop", post(assistant_stop))
        .route("/api/assistant/warm", post(assistant_warm))
        .route("/api/conversations", get(conversations_list).post(conversations_create))
        .route("/api/conversations/{id}", get(conversations_get).patch(conversations_rename).delete(conversations_delete))
        .route("/api/conversations/{id}/send", post(conversations_send))
        .route("/api/conversations/{id}/turns/{turn}", axum::routing::patch(conversations_outcome))
        .route("/api/memory", get(memory_list).post(memory_add))
        .route("/api/memory/{id}", axum::routing::delete(memory_delete))
        .route("/api/models", get(models_list))
        .route("/api/models/{id}/file", get(model_file))
        .route("/api/library", get(library_books))
        .route("/api/library/search", get(library_search))
        .route("/kiwix/{*rest}", get(kiwix_proxy))
        .route("/kiwix-lat/{*rest}", get(kiwix_proxy_latin))
        .fallback(crate::ui::serve)
        .layer(axum::Extension(listener))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Runs slow work that blocks (disk, Argon2, PowerShell, copying the
/// database) on a thread made for it, so the async workers keep answering
/// phones and the laptop window meanwhile.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f).await.map_err(|e| ApiError::from(anyhow::anyhow!("background task failed: {e}")))
}
