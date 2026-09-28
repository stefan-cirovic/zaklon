//! Helpers shared by the assistant's tests.

use std::sync::Arc;

use zaklon_core::supplies::Item;

use super::{Assistant, Passage, Source};
use crate::downloads::Downloads;
use crate::kiwix::Library;

pub(super) fn item(name: &str, qty: f64, unit: &str) -> Item {
    serde_json::from_value(serde_json::json!({
        "id": format!("id-{name}"), "name": name, "quantity": qty, "unit": unit, "category": "food",
        "place": null, "expiry": null, "barcode": null, "min_quantity": null, "notes": null,
        "updated_at": "2026-09-28T00:00:00Z", "updated_by": null, "batches": []
    }))
    .unwrap()
}

pub(super) fn passage(n: usize, title: &str, text: &str) -> Passage {
    let source = Source { n, title: title.into(), web: false, url: format!("/kiwix/content/wp/{title}"), book_title_en: "Wikipedia".into(), book_title_sr: "Vikipedija".into() };
    Passage { source, text: text.into() }
}

pub(super) fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

pub(super) fn test_assistant() -> Arc<Assistant> {
    let root = std::env::temp_dir().join(format!("zaklon-assistant-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let catalog = zaklon_core::catalog::Catalog { version: 1, generated: String::new(), packs: Vec::new() };
    let downloads = Downloads::new(catalog, root.join("library"), root.join("state.json"));
    let library = Library::new(downloads.clone());
    Assistant::new(downloads, library, None)
}
