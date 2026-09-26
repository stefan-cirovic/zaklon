# Roadmap

Zaklon is developed in the open, without a fixed deadline, in order of technical risk. This page is updated as milestones complete.

## Spike (in progress)

Validate the riskiest pieces before building modules:

- [x] Android app (Tauri 2) connects to the Rust hub over pinned TLS on the local network with no internet (tested on a real phone, 2026-09-26). Laptop-created Wi-Fi still to test.
- [ ] On-device AI on Android (small Qwen3.5 model) with the model downloaded from the hub.
- [ ] Barcode scanning from the Android app.
- [ ] 20 GB pack download with resume, verification and USB import; kiwix-serve and llama-server supervised as sidecars on Windows.

## 1.0

1. Hub core, pairing, Household screen, Windows installer.
2. Library: kiwix-serve, catalog, downloads, USB export/import. *Working: supervised kiwix-serve, search across books in Latin and Cyrillic (also without diacritics), reader on laptop and phone.*
3. Supplies: items, barcodes, history, backup/restore.
4. Maps: CoMaps APK and map files served by the hub.
5. Assistant: hub model, then phone model, then opt-in online research.
6. Profiles with optional personal passwords, polish, release.

## 1.1

Receipt-to-inventory by photo · medication leaflet reading with safety rails · article translation add-on · Cyrillic script · profile on USB · phone notifications.

## Later

Voice · AI skills · hub-to-hub sync · light theme · iOS, Linux and macOS.
