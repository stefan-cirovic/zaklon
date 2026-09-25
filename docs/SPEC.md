# Zaklon 1.0 — Product and Technical Specification

Status: draft for approval · Last updated: 2026-09-25

Zaklon (Serbian for "shelter") is a free, open-source, offline-first home base. A laptop runs the hub; the household's Android phones connect to it over local Wi-Fi, with or without internet. It keeps the family's knowledge library, maps, supplies and a local AI assistant working when everything else is down, and it is just as useful on an ordinary day.

## 1. Principles

1. **Works offline by default.** Internet is an optional convenience, never a requirement.
2. **Local only.** No accounts, no telemetry, no cloud. The household owns its data as plain files in one folder.
3. **Free forever.** GPL-3.0-or-later. Donations are optional and never unlock features.
4. **Transparent.** Public repository from the first commit; every outbound network call is documented and user-initiated.
5. **Light.** Small installer, low idle memory, no animations, runs on a mid-range laptop on battery.
6. **Trust inside the household.** No admin role; anyone with the household password has equal rights.

## 2. Scope

### In 1.0
- Hub application for Windows 10/11 (64-bit) running on a laptop.
- Android client (Android 9+) installed from the hub over local Wi-Fi.
- Pairing over the home router or a Wi-Fi network created by the laptop.
- Library: offline knowledge packs (Kiwix ZIM) with full-text search.
- Maps: offline maps via CoMaps, with map files and the CoMaps APK served by the hub.
- Supplies: household inventory with barcode scanning, expiry dates, places, running-low and shopping lists, change history.
- Profiles: shared household space plus a private space per person (optional personal password).
- Assistant: local AI on the hub and a smaller model on the phone; library search with sources; inventory questions and edits (with confirmation); simple memory; opt-in online research.
- Add-ons: catalog of packs (knowledge, maps, models), resumable downloads, USB export/import.
- Household: devices, profiles, backup/restore, settings, hub status.
- Languages: English (default) and Serbian (Latin script).

### Later (not in 1.0)
Receipt-to-inventory by photo (1.1) · medication leaflet reading (1.1+, with safety rails) · article translation add-on (1.1) · Cyrillic script (1.1) · profile on USB (1.1) · phone push notifications (1.1) · voice · AI skills · hub-to-hub sync · light theme · iOS, Linux, macOS · Termux and Meshtastic integrations.

## 3. Platforms and requirements

| | Hub (laptop) | Phone |
|---|---|---|
| OS | Windows 10 (19045+) or 11, 64-bit | Android 9+ (API 28) |
| Minimum | 8 GB RAM, 20 GB free disk | 4 GB RAM |
| Recommended | 12–16 GB RAM, 100+ GB free disk | 6 GB RAM |
| GPU | Optional (NVIDIA/AMD/Intel via Vulkan or CUDA) | Not required |
| Network | Wi-Fi adapter | Wi-Fi, camera |

Reference hardware: HP ENVY x360 (i7-1165G7, 12 GB, integrated graphics) as the primary hub; a desktop with GTX 1070 as the "strong hub" test machine.

Performance targets: installer under 100 MB without content; hub idle under 200 MB RAM with the AI unloaded; phone app under 30 MB; cold start under 3 s on the reference laptop.

## 4. Architecture

```
┌──────────────── Laptop (hub) ─────────────────┐      ┌──── Phone ────┐
│ Tauri 2 desktop shell (React UI, system WebView)│      │ Tauri 2 app   │
│ zaklon-hub (Rust daemon, tray service)          │◄TLS─►│ React UI      │
│  ├ HTTP/JSON API + static UI                    │      │ SQLite cache  │
│  ├ SQLite (household, profiles, catalog)        │      │ llama.cpp     │
│  ├ mDNS/DNS-SD advert + UDP beacon              │      │ (small model) │
│  ├ Hotspot control (Windows Mobile Hotspot)     │      │ camera/barcode│
│  ├ sidecar: kiwix-serve (library)               │      └───────────────┘
│  └ sidecar: llama-server (assistant)            │
└─────────────────────────────────────────────────┘
```

- **Hub daemon** (`zaklon-hub`, Rust): axum HTTP server, SQLite via sqlx/rusqlite, embedded React build, DNS-SD advertisement, download manager, backup, sidecar supervision. Runs as a tray application started at login; the desktop window is a Tauri 2 shell over the same UI the phone uses.
- **Sidecars** are separate processes and separate programs (GPL "mere aggregation"): `kiwix-serve` (GPLv3+) serves ZIM files and its search API; `llama-server` from llama.cpp (MIT) exposes an OpenAI-compatible API bound to localhost only. The hub proxies both to authenticated clients.
- **Phone app** (Tauri 2 Android): same React UI, Rust core shared with the hub for models and sync, Kotlin plugins for camera/barcode, on-device inference (llama.cpp via JNI) and foreground download service.
- **Single install folder** chosen at install time (default `C:\Zaklon`):

```
Zaklon/
  app/          program files (managed by the installer)
  library/      packs: zim/, maps/, models/, apk/
  household/    household.db, photos/, history
  profiles/     <profile-id>/profile.db (+ encrypted if a personal password is set)
  catalog/      catalog.json, signatures, download state
  backups/      default target for backup archives
  logs/
```

Everything under `Zaklon/` except `app/` is user data. Moving the folder to another disk and pointing the app at it must work.

### 4.1 Networking and pairing
- The hub listens on TCP 8484 with TLS (self-signed certificate generated on first run) for phones, on 127.0.0.1:8481 without TLS for the desktop window, and on TCP 8480 without TLS for the "install the app" page and APK files only.
- Discovery: DNS-SD `_zaklon._tcp` plus a UDP beacon on 8485 for networks that block mDNS; the pairing QR embeds `{host, port, certSha256, token}` so discovery is never required.
- **Pairing flow**: Household → Add device → QR appears on the laptop. On the phone: install the app from `http://<hub>:8480/get` (the address is shown next to the QR), scan the QR, enter the household password. The phone pins the certificate fingerprint and receives a long-lived device token. All later traffic is TLS with the pinned certificate; the household password is never stored on the phone.
- **Laptop as access point**: Household → "Create Wi-Fi network" toggles Windows Mobile Hotspot (SSID `Zaklon`, password shown on screen). Phones join it like any Wi-Fi network.
- Devices are listed with name, platform, last seen; any paired member can rename or remove a device.

### 4.2 Security model
- One household password (minimum 8 characters, no other rules) set at install; changeable by anyone at the laptop; reset from the laptop requires no proof (physical access is trust).
- Personal profiles may set a personal password; the profile database is encrypted with a key derived from it (Argon2id + XChaCha20-Poly1305). No recovery: losing the password loses the private space, stated clearly when it is set.
- Device tokens are revocable per device. Rate limiting on pairing attempts.
- Sidecars bind to localhost only; the hub is the single exposed service.

### 4.3 Sync
- Hub SQLite is the source of truth. Every row carries a hybrid logical clock and the device id of the last writer.
- Phones keep a local SQLite copy of the household data and an outbox of operations; sync exchanges operations since the last cursor. Conflicts on supplies resolve per field, last writer wins; history is append-only, so nothing is silently lost.
- Phones can work offline against the cache and reconcile when the hub is reachable. Private profile data syncs the same way but only to devices unlocked for that profile.

## 5. Data model

**Shared (household)**
- `item`: name, quantity, unit (preset list: pcs, kg, g, l, ml, pack, plus custom), category (food, drink, medicine, hygiene, equipment, fuel, other), place (preset list: pantry, fridge, freezer, medicine cabinet, garage, basement, plus custom), expiry date, barcode, minimum quantity, notes, photo (optional).
- `shopping_list_entry`: item or free text, quantity, done, source (manual | running-low).
- `history`: who, what, when, before/after values; append-only.
- `device`, `pack` (installed add-ons), `household_settings`.

**Private (per profile)**
- `conversation`, `message` (assistant chats, with source references and an "online" flag per message).
- `note`: plain text notes.
- `memory`: facts the assistant remembers, each confirmed by the user, editable and deletable.
- `profile_settings`: name, avatar, language, accent color.

Medicines in 1.0 are ordinary items with an expiry date; no leaflet text, no dosage information.

## 6. Modules

### Home
Hub status (reachable, battery level and charging state, disk free), device count, "Expiring soon" (next 30 days), "Running low" (below minimum), two quick actions: Add item, Ask the assistant.

### Library
Installed packs, unified full-text search across packs (via kiwix-serve), reader view, "Open in assistant" from any article. Packs are served from the hub; a phone can optionally download a pack for use away from the hub.

### Maps
Explains and launches CoMaps. The hub serves the CoMaps APK and map files in the layout CoMaps expects as a custom map server, so phones download maps without internet. A "Balkans" and a "World base" pack are offered by default; other regions via the catalog.

### Supplies
List and grid views, filters by category/place/expiry, search. Add item manually or by scanning a barcode with the phone camera; unknown barcodes are named once and remembered locally. Consume/add quantity with one tap; running-low and shopping lists; history view; CSV export.

### Assistant
Chat per profile. Capabilities in 1.0:
- Answers in the language the user writes (English or Serbian).
- Searches the library and cites sources as links to the article.
- Answers questions about supplies from the household database.
- Edits supplies on request ("add 2 kg flour", "we used the oil") with an explicit confirmation card before writing.
- Simple memory: proposes facts to remember; saved only on confirmation; visible and editable in profile settings.
- Online research: off by default; a per-conversation "Online" switch, a visible indicator while on, and a log of every request. Default search: DuckDuckGo (no key); optional user-provided Brave Search key in settings. Findings can be saved to the library with one click.
- Declines medical dosing advice and does not invent sources; says when it does not know.
- Model lifecycle: loaded on first question, unloaded after 5 minutes idle; warns when battery is below 20%.

### Add-ons
Catalog screen with recommended packs by UI language ("English essentials" ≈ 17 GB: Medical Wikipedia + English Wikipedia mini; "Serbia core" ≈ 21 GB: Serbian Wikipedia, Medical Wikipedia, Serbian Wiktionary, iFixit, selected Stack Exchange sites), maps, and AI models with a "recommended for this computer" badge. Downloads require at least 50% battery (or charger) and enough free space; they resume after interruption and verify hashes and signatures. "Export to USB" and "Import from USB" move packs between hubs without internet.

### Household
Devices, profiles, backup/restore, hub status and hardware summary, "Create Wi-Fi network", updates (check now; automatic check on/off, default on as chosen at install), language, accent color, install folder, licenses and attribution, privacy statement.

## 7. AI

**Models offered in 1.0** (all Apache-2.0, GGUF via llama.cpp):

| Model | Download | Runs on | Notes |
|---|---|---|---|
| Qwen3.5-0.8B | ~0.6 GB | phones with 4 GB | fallback when the hub is unreachable |
| Qwen3.5-2B | ~2 GB | 8 GB laptops, phones with 6 GB+ | fast, reads images |
| Qwen3.5-4B | ~3.5 GB | 12–16 GB laptops (recommended) | best balance on CPU |
| Gemma 4 E4B | ~6 GB | 16 GB laptops | adds audio; slower on CPU |
| Qwen3.5-9B | ~6 GB | GPU with 8 GB VRAM or 32 GB RAM | best answers |

The list ships in the catalog and can change without an app release.

**Selection**: on first run and on demand the app reads RAM, CPU, GPU/VRAM, free disk and battery, and marks one model "recommended" with size, expected speed (slow / fine / fast) and abilities (text, images, audio). The user may install several and switch the active one at any time. The phone has its own list and recommendation.

**Default model** is chosen by a fixed test: ten identical questions in Serbian (Latin script) and English per model, rated by the founder. The test set lives in `docs/ai-eval.md`.

**Retrieval**: no vector database in 1.0. Library search uses the full-text index inside ZIM files through kiwix-serve; supplies are queried from SQLite; notes by text search. The model calls these as tools.

**Learning** means memory + library + (later) skills, never model retraining.

## 8. Add-on catalog and updates

- `catalog.json` is signed with minisign; the public key is embedded in the app and published in the repository.
- Each entry: id, title (i18n), category (knowledge | maps | model | app), version, size, files (URL list with mirrors, SHA-256, optional BLAKE3 chunk hashes), license, attribution text, minimum app version.
- Large third-party files are not mirrored: knowledge packs point to Kiwix, maps to CoMaps, models to Hugging Face. Zaklon's own packs live on Cloudflare R2.
- Downloads use HTTP range requests with per-file verification; the hub can serve any installed pack to phones.
- App updates: signed releases on GitHub; the app checks on demand or automatically (user's choice at install). Updates are never applied silently.

## 9. Localization

English is the default UI language; Serbian (Latin script) is selectable per profile. All user-facing strings are in translation files; Serbian typing works everywhere; the assistant answers in the language of the question. Cyrillic display and input come in 1.1 via deterministic transliteration.

## 10. Design: "instrument panel"

- Dark theme only in 1.0: near-black background (pure black optional for OLED), white text, one user-selected accent color (green, white, purple, blue, amber) used sparingly for the active tab, primary button and status. Red is reserved for warnings (expiry, running low, low battery).
- One typeface everywhere (Inter or system sans-serif); no monospace.
- Thin dividers, no shadows, no gradients, no animations; state changes are instant.
- Phone: bottom tabs (Home, Library, Maps, Supplies, Assistant, More). Laptop: left sidebar with all seven sections.
- Large tap targets and readable default text size; no separate "large text" mode.
- Section names in Serbian are plain: Početna, Biblioteka, Mape, Zalihe, Asistent, Dodaci, Domaćinstvo.
- Logo: three proposals during the spike; must work as a 16 px icon.

## 11. Trust, licensing and distribution

- License: GPL-3.0-or-later for the hub and apps. Third-party components stay separate programs with their own licenses; a Licenses screen and `THIRD_PARTY.md` list every component and content pack with license and attribution (Wikipedia CC BY-SA 4.0, OpenStreetMap ODbL, Kiwix GPLv3+, llama.cpp MIT, CoMaps Apache-2.0, model licenses).
- Repository public from the first commit with `LICENSE`, `README.md`, `SECURITY.md`, `CONTRIBUTING.md`, `THIRD_PARTY.md`, a public roadmap and this spec. Everything public is in English.
- Releases are built by GitHub Actions only, with SHA-256 checksums, SBOM and a VirusTotal link; SignPath Foundation code signing after the first release; Microsoft Store and winget next; Android via GitHub Releases, IzzyOnDroid and F-Droid. Google Play developer verification will be completed through a registered association before the 2027 global rollout.
- Privacy statement (plain language): no accounts, no telemetry, no crash upload. The app talks to the network only for (1) update checks if enabled, (2) downloads the user starts, (3) online research the user turns on. The complete list of hosts is published in `docs/network.md`.
- Third-party names appear as plain text ("uses Kiwix", "map data © OpenStreetMap contributors") with no logos.
- Donations: GitHub Sponsors, Open Collective (public ledger) and published BTC/ETH addresses; a public finances page. No feature is ever paywalled.

## 12. Risks and the spike

The riskiest parts are validated in a time-boxed spike (up to two weeks) before module work starts:

1. Tauri 2 Android app connecting over pinned TLS to the Rust hub across a laptop-created Wi-Fi network with no internet.
2. On-device inference on Android (Qwen3.5-0.8B/2B) inside the Tauri app, including model download from the hub.
3. Camera barcode scanning on Android from the Tauri app.
4. A 20 GB pack download with interruption, resume, hash verification and USB import; kiwix-serve and llama-server as supervised sidecars on Windows.

Exit criteria: all four work on the reference laptop and at least two family phones. Fallbacks: Capacitor or native Kotlin for the phone client (hub unchanged); Go for the hub if Rust productivity is the blocker (API unchanged).

## 13. Delivery order after the spike

1. Hub core + pairing + Household screen + installer.
2. Library (kiwix-serve, catalog, downloads, USB).
3. Supplies (with barcode, history, backup/restore).
4. Maps (CoMaps serving).
5. Assistant on the hub, then on the phone; online research last.
6. Profiles and personal passwords; polish; release 1.0.

Each step ends with a build the founder can install and test.

## 14. Defaults confirmed for 1.0

Single install folder chosen at install · Android 9 minimum · 64-bit Windows 10/11 only · household password ≥ 8 characters · Home content as in §6 · unknown barcodes named once and remembered · expiry reminders as an in-app list only · backup/restore included · "English essentials" / "Serbia core" recommended by language · three logo proposals during the spike · no deadline; work proceeds in risk order.
