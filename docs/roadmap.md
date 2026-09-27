# Roadmap

Zaklon is developed in the open, without a fixed deadline, in order of technical risk. This page is updated as milestones complete.

## Spike (in progress)

Validate the riskiest pieces before building modules:

- [x] Android app (Tauri 2) connects to the Rust hub over pinned TLS on the local network with no internet (tested on a real phone, 2026-09-26). Laptop-created Wi-Fi still to test.
- [x] On-device AI on Android (Qwen3.5 2B) with the model copied from the hub (tested on a Samsung A56, about 12 tokens per second).
- [x] Barcode scanning from the Android app (zxing-cpp on the phone itself, without Google Play services or internet).
- [x] Large pack downloads with resume, verification and USB import; kiwix-serve and llama-server supervised as sidecars on Windows.

## 1.0

1. Hub core, pairing, Household screen, Windows installer. *Done: per-user installer with bundled WebView2 (works offline), tray icon, start with Windows, single instance, data kept on uninstall, and the laptop's own Wi-Fi network (Windows Mobile hotspot) for when there is no router.*
2. Library: kiwix-serve, catalog, downloads, USB export/import. *Working: supervised kiwix-serve, search across books in Latin and Cyrillic (even when typed without diacritics), reader on laptop and phone.*
3. Supplies: items, barcodes, history, backup/restore. *Done: batches with their own expiry dates, shopping list and put-away, daily backups and backups to USB with restore, a phone's offline copy with a shopping list that syncs when back home.*
4. Maps: CoMaps APK and map files served by the hub. *Done: the whole world in pieces, with English and Serbian names.*
5. Assistant: hub model, then phone model, then opt-in online research. *Done: answers grounded in the library with sources, supplies questions and confirmed changes, memory, opt-in online research; phones use the hub's model at home and their own away.*
6. Profiles with optional personal passwords, polish, release. *Next.*

## 1.1

Receipt-to-inventory by photo · medication leaflet reading with safety rails · article translation add-on · Cyrillic script · profile on USB · phone notifications.

## Later

Voice · AI skills · hub-to-hub sync · light theme · iOS, Linux and macOS.
