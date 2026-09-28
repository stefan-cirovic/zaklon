# Zaklon 1.0 — Product and Technical Specification

Status: working draft · Last updated: 2026-09-27

> This spec describes the 1.0 target. Where the current build differs, [the roadmap](roadmap.md) and the code are current. Notes marked *Current build* describe what exists today.

Zaklon (Serbian for "shelter") is a free, open-source, offline-first home base. A laptop runs the hub; the household's Android phones connect to it over local Wi-Fi, with or without internet. It keeps the family's knowledge library, maps, supplies and a local AI assistant working when everything else is down, and it is just as useful on an ordinary day.

## 1. Principles

1. **Works offline by default.** Internet is an optional convenience, never a requirement.
2. **Local only.** No accounts, no telemetry, no cloud. The household owns its data as plain files in one folder.
3. **Free forever.** GPL-3.0-or-later. Donations are optional and never unlock features.
4. **Transparent.** Public repository from the first commit; every outbound network call is documented in `docs/network.md`. All of them are started by the user, except a daily update check that the household is asked about at setup (the box starts ticked) and can switch off at any time.
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
│ zaklon-hub (Rust, runs inside the app)          │◄TLS─►│ React UI      │
│  ├ HTTP/JSON API + static UI                    │      │ offline copy  │
│  ├ SQLite (household data)                      │      │ llama.cpp     │
│  ├ mDNS/DNS-SD advert + UDP beacon              │      │ (small model) │
│  ├ Hotspot control (Windows Mobile Hotspot)     │      │ camera/barcode│
│  ├ sidecar: kiwix-serve (library)               │      └───────────────┘
│  └ sidecar: llama-server (assistant)            │
└─────────────────────────────────────────────────┘
```

- **Hub** (`zaklon-hub`, Rust): axum HTTP server, SQLite via rusqlite, embedded React build, DNS-SD advertisement, download manager, backup, sidecar supervision. It runs inside the desktop app's process (Tauri 2), which starts at login and keeps running in the tray when its window is closed; the window shows the same UI the phone uses. For testing, the hub also runs on its own (`zaklon-hub --root <folder>`).
- **Sidecars** are separate processes and separate programs (GPL "mere aggregation"): `kiwix-serve` (GPLv3+) serves ZIM files and its search API; `llama-server` from llama.cpp (MIT) exposes an OpenAI-compatible API bound to localhost only. The hub proxies both to authenticated clients.
- **Phone app** (Tauri 2 Android): same React UI and a small Rust layer (pinned-TLS client, read-only proxy for library articles, on-device AI). Barcode scanning uses a small plugin of its own (`apps/zaklon/src-tauri/plugins/scanner`): a CameraX camera view whose frames zxing-cpp reads on the phone, with no Google Play services and no network. On-device AI runs llama.cpp's `llama-server`, bundled in the app, as a separate process on 127.0.0.1; models are copied from the hub over Wi-Fi while the screen is kept on.
- **Single install folder** chosen at install time. The installer is per-user (its default folder is under `%LOCALAPPDATA%`); the program files sit in the chosen folder and the household's data in its `data` subfolder:

```
Zaklon/            program files (managed by the installer)
  data/
    household/    household.db, hub.json, tls/
    library/      packs: zim/, maps/, models/, apk/; bin/ (library and AI engines); installer/
    profiles/     <profile-id>/profile.db (+ encrypted if a personal password is set; profiles are planned)
    catalog/      download state; an optional newer catalog.json
    backups/      daily backups and backups made by hand; the data the last 3 restores replaced
    backup-temp/  unencrypted pieces while a backup is made (removed right after, and on every start)
    logs/
```

Everything under `data/` is user data, and uninstalling keeps it. Moving the folder to another disk and pointing the app at it must work.

A restore takes the household's data from the backup and, on a hub that was set up, keeps who may connect as it is: the paired phones, the household password, the Wi-Fi password, the backup key and the hub's identity. Only a backup of another hub restored onto a new install (never set up, from the setup screen) takes those from the backup too. The backup's database is refused if it holds triggers or views, and only its rows are copied into the hub's own schema.

### 4.1 Networking and pairing
- The hub listens on TCP 8484 with TLS (self-signed certificate generated on first run) for phones, on 127.0.0.1:8481 without TLS for the desktop window, and on TCP 8480 without TLS for the "install the app" page and APK files only.
- Discovery: DNS-SD `_zaklon._tcp` plus a UDP beacon on 8485 for networks that block mDNS; the pairing QR (version 2) carries the hub's addresses, port, certificate fingerprint, a 128-bit pairing secret, the hub name and the install-page port, so discovery is never required.
- **Pairing flow**: Household → Devices → Add a phone → QR appears on the laptop. On the phone: install the app from `http://<hub>:8480/get` (the address is shown next to the QR), scan the QR, enter the household password. The phone pins the certificate fingerprint, sends the QR's secret with the password (`POST /api/pair/complete`, which never takes the 6-digit code) and receives a long-lived device token. All later traffic is TLS with the pinned certificate; the household password is never stored on the phone.
- **Pairing from "Find hubs"** (no QR): a discovery answer is not authenticated, so the phone does not take the certificate from it. The person types the 6-digit code from the laptop and the password. The phone connects accepting any certificate, notes the one it got, and runs SPAKE2 with the hub on the code (`POST /api/pair/pake/start`, with the name the phone pairs under; crate `zaklon-pake`). With the shared key the hub proves (HMAC) the fingerprint of its own certificate; the phone checks that proof against the certificate it actually saw, so a device that answers in its place and passes the messages on is caught. Only then does the phone prove the key and send the password, over a connection pinned to that certificate (`POST /api/pair/pake/finish`). One run is one guess at the code: it takes one of the code's three attempts and counts as a failure of its address until it pairs.
- **Pairing limits**: a code is open for 5 minutes with three attempts. Every failed pairing request (QR or "Find hubs") takes one, and one address may take at most two, so a single other device on the Wi-Fi cannot use them all up. A wrong QR secret gets the same answer as no open code. An address with 10 failures waits 10 minutes (checked before it takes an attempt). The pairing requests exist only on the TLS listener, not on the laptop's loopback port.
- **Laptop as access point**: Household → Network → "Wi-Fi network from this laptop" switches on the Mobile hotspot built into Windows (SSID `Zaklon`, password and a QR code to join shown on screen). Phones join it like any Wi-Fi network.
- Devices are listed with name, platform and last seen. The laptop can rename or remove any phone; a phone can rename or remove only itself.

### 4.2 Security model
- One household password (minimum 8 characters, no other rules) set at install; changeable by anyone at the laptop; reset from the laptop requires no proof (physical access is trust).
- Personal profiles may set a personal password; the profile database is encrypted with a key derived from it (Argon2id + XChaCha20-Poly1305). No recovery: losing the password loses the private space, stated clearly when it is set.
- Device tokens are revocable per device. Rate limiting on pairing attempts.
- Sidecars bind to localhost only; the hub is the single exposed service.

### 4.3 Sync
- Hub SQLite is the source of truth. Every row carries a hybrid logical clock and the device id of the last writer.
- Phones keep a local SQLite copy of the household data and an outbox of operations; sync exchanges operations since the last cursor. Conflicts on supplies resolve per field, last writer wins; history is append-only, so nothing is silently lost.
- Phones can work offline against the cache and reconcile when the hub is reachable. Private profile data syncs the same way but only to devices unlocked for that profile.
- *Current build:* there is no per-row clock yet. A phone keeps its last copy of the supplies for reading, and an outbox of shopping-list changes that the hub applies when the phone is back; the hub recognizes a repeated change by its id. Other changes need the hub.

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
Explains and launches CoMaps. The hub serves the CoMaps APK and map files in the layout CoMaps expects as a custom map server, so phones download maps without internet. The whole world is offered in pieces (countries, and regions of large countries) exactly as CoMaps publishes them; the Serbia map is part of the Serbian starter set.

### Supplies
List and grid views, filters by category/place/expiry, search. Add item manually or by scanning a barcode with the phone camera; unknown barcodes are named once and remembered locally. Consume/add quantity with one tap; running-low and shopping lists; history view; CSV export.

### Assistant
Chat per profile. Capabilities in 1.0:
- Answers in the language the user writes (English or Serbian).
- Searches the library and cites sources as links to the article.
- Answers questions about supplies from the household database.
- Edits supplies on request ("add 2 kg flour", "we used the oil") with an explicit confirmation card before writing.
- Simple memory: proposes facts to remember; saved only on confirmation; visible and editable in profile settings. *Current build:* until profiles exist, the notes are shared by the household and listed on the Assistant screen ("What the assistant remembers").
- Saved conversations, laid out like a chat app: the list on the left (on the phone, a panel opened from the top) grouped by recency with a search, the conversation in the rest of the width and the question box at the bottom. *Current build:* until profiles exist, conversations belong to a device: the hub keeps them, and each device sees, renames and deletes only its own (the laptop its own, each phone its own). A copy can be sent to another device of the household, where it appears as a new conversation marked with the sender's name. The title comes from the first question and can be changed. Removing a phone deletes its conversations; backups include them. Away from the hub a phone shows its list and the conversations it opened last, read-only. Limits: 500 conversations per device (the one used longest ago makes room) and 200 questions per conversation.
- Online research: off by default; a per-conversation switch and a visible indicator while on. The pages it read are listed as sources. Search: DuckDuckGo (no key). *Not in the current build:* an optional Brave Search key and saving findings to the library.
- Declines medical dosing advice and does not invent sources; says when it does not know.
- Model lifecycle: loaded when the Assistant screen opens or on the first question, stopped after 20 minutes without questions, and can be stopped at once to free the memory. There is no low-battery warning yet.
- Time limits: the library search takes at most 20 seconds (a few requests: books of one language are searched together, recent results are remembered) and uses what it found by then; a question has 6 minutes once the model is loaded. A question nobody has asked about for a minute (a closed window) gives way to the next question in line.

### Add-ons
Catalog screen with a starter set by UI language, downloaded with one button together with the AI model that fits the computer ("Basic pack for Serbia": Serbian Wikipedia with pictures, Serbian Wiktionary, Medical Wikipedia, iFixit, the safe water and first-aid guides and the Serbia map; "English essentials": English Wikipedia summaries, Medical Wikipedia, iFixit and the same guides), maps, and AI models with a "recommended for this computer" badge. Downloads require at least 50% battery (or the charger) and enough free space; they resume after interruption and verify hashes (and signatures, once the catalog is signed). "Export to USB" and "Import from USB" move packs between hubs without internet.

The screen looks and works like Windows File Explorer's "This PC": "Devices and drives" shows the drive the library is on with a usage bar (red when nearly full) and how much the add-ons take (opening it lists what is on it, largest first), the laptop's other drives (a USB drive opens with "Copy to USB" and "Import" set to it) and the battery; below, the add-ons sit in folders with their count and size: Wikipedia and books, Health and first aid, Garden and food, Repair and skills (knowledge packs, by their catalog topic), AI models, Maps (every country) and Programs. A folder opens as tiles or as a details table (name, size, status, license, actions), with a breadcrumb ("Add-ons › Maps") and Back; the place is part of the address (`#addons/maps`), so the browser's and the phone's Back button work too. The search box finds packs and countries across all folders. Tiles or details is remembered per device. Removing packs and maps is laptop only.

### Household
Laid out like Windows Settings: the hub at the top (its name, whether it runs, paired phones, address, version and the last backup), a "Find a setting" search, and a tile for each category, each with an icon and a line on what it holds. A category opens as its own page ("Household › Backups", with a way back); on a laptop the categories are listed beside it. Every category and setting has an address (`#household/backups`, `#household/network/hotspot`), so the browser's back button, links and the search all lead to the right place. The search finds settings by name and by other words, in English and Serbian whatever the app's language (without Serbian accents too).

| Category | What is in it | Phone |
|---|---|---|
| Devices | Add a phone (pairing code and QR codes), paired phones | the hub this phone uses ("Forget this hub"), paired phones |
| Network | Wi-Fi network from this laptop, Windows Firewall, the laptop's addresses | not shown |
| Backups | encryption, backups on this computer, backup to USB, restore | not shown |
| Privacy & security | household password, privacy statement | privacy statement |
| Appearance | accent color, pure black (this device) | same |
| Language | app language (this device), Latin script for Serbian articles | same |
| AI assistant | the AI model the assistant uses, what it remembers | same (models are downloaded on the laptop) |
| Updates | check now, automatic daily check on/off (on by default) | check now |
| About | version and license, this hub's computer and data folder, licenses and attribution | same, without the data folder |

Profiles are planned. First run shows the setup (with restoring a previous hub's backup) instead.

## 7. AI

**Models offered in 1.0** (all Apache-2.0, GGUF files converted by ggml-org and Unsloth, run by llama.cpp):

| Model | Download | Runs on | Notes |
|---|---|---|---|
| Qwen3.5-0.8B (Q8_0) | 0.83 GB | phones with 4 GB | fastest, basic answers; fallback when the hub is unreachable |
| Qwen3.5-2B (Q4_K_M) | 1.28 GB | phones with 6 GB or more, older laptops | good balance, understands Serbian |
| Qwen3.5-4B (Q4_K_M) | 2.74 GB | laptops with 12 GB (recommended) | noticeably better answers |
| Qwen3.5-9B (Q4_K_M) | 5.68 GB | computers with 16 GB or more | best answers, slower |

The list ships in the catalog and can change without an app release.

**Selection**: on first run and on demand the app reads RAM, CPU, GPU/VRAM, free disk and battery, and marks one model "recommended" with size, expected speed (slow / fine / fast) and what it is good for. The user may install several and switch the active one at any time. The phone has its own list and recommendation.

**Default model** is chosen by a fixed test: ten identical questions in Serbian (Latin script) and English per model, rated by a native speaker. The test set lives in `docs/ai-eval.md`.

**Retrieval**: no vector database in 1.0. Library search uses the full-text index inside ZIM files through kiwix-serve; supplies are queried from SQLite; notes by text search. The model calls these as tools.

**Learning** means memory + library + (later) skills, never model retraining.

## 8. Add-on catalog and updates

- `catalog.json` is built into the app. Planned: a catalog signed with minisign, with the public key embedded in the app and published in the repository. *Not in the current build:* signing and fetching a newer catalog.
- Each entry: id, title and description (i18n), category (knowledge | maps | model | app), for knowledge packs a topic (reference | health | garden | skills: the Add-ons folder it shows in), version, size, files (URL list with mirrors, SHA-256, or the SHA-1 CoMaps publishes for map files), license, attribution text, source link, languages and the UI languages it is recommended for.
- Large third-party files are not mirrored: knowledge packs point to Kiwix, maps to CoMaps, models to Hugging Face. Zaklon's own packs will live on Cloudflare R2 (planned).
- Downloads use HTTP range requests with per-file verification; the hub can serve any installed pack to phones.
- App updates: releases on GitHub (signed releases are planned; see `SECURITY.md`). The hub checks once a day (on by default; switch under Household → Updates) and on demand, and only tells: "Open the download page" opens the release page in the browser. The app never downloads or applies an update itself.

## 9. Localization

English is the default UI language; Serbian (Latin script) is selectable per profile. All user-facing strings are in translation files; Serbian typing works everywhere; the assistant answers in the language of the question. Cyrillic display and input come in 1.1 via deterministic transliteration.

## 10. Design: "instrument panel"

- Dark theme only in 1.0, in the colors of VS Code's "Dark Modern" (background #1F1F1F, bars #181818, panels #252526, borders #2B2B2B, text #CCCCCC; pure black optional for OLED), with one user-selected accent color (amber, the logo's light, by default; green, white, purple, blue) used sparingly for the active item, primary button and status. Red is reserved for warnings (expiry, running low, low battery). All text meets WCAG AA contrast.
- Help: a built-in user guide in English and Serbian (following the app's language), reached from the Tools screen and from a "How it works" link at the top of every screen, which opens that screen's page (`#help/<topic>`, and a Household category its own section). A page per topic (getting started, pairing a phone, each screen and tool, Household settings, working without internet, troubleshooting) with numbered steps and links to the screens it names; a search on the Help home finds topics and sections by any word, with or without accents. The text lives in `ui/src/help/en.ts` and `sr.ts` and is loaded only when Help opens.
- The system UI typeface as VS Code uses it (Segoe UI on Windows, the system font elsewhere), and Sora Light for the "ZAKLON" wordmark only; no monospace.
- Thin dividers, no shadows, no gradients. The only animation: the logo in the bar brings in the wordmark when pointed at (none when the system asks for reduced motion).
- Screens use the whole width of the window (grids and columns on a laptop), not a narrow centered column.
- One bar along the bottom, on the phone and the laptop: Home, Assistant, Tools, Household, and at most one tool the household pinned (five items at most). The pinned tool is chosen on the laptop and kept on the hub, the same on every device; nothing is pinned by default. The Tools screen lists every tool (Supplies, Library, Maps, Add-ons) with a one-line description. The logo sits at the left end of the bar and opens zaklon.com.
- Large tap targets and readable default text size; no separate "large text" mode.
- Section names in Serbian are plain: Početna, Asistent, Alati, Domaćinstvo, Biblioteka, Mape, Zalihe, Dodaci.
- Logo: done (see `logo/README.md`); it works as a 16 px icon.

## 11. Trust, licensing and distribution

- License: GPL-3.0-or-later for the hub and apps. Third-party components stay separate programs with their own licenses; `THIRD_PARTY.md` lists every component and content pack with license and attribution (Wikipedia CC BY-SA 4.0, OpenStreetMap ODbL, Kiwix GPLv3+, llama.cpp MIT, CoMaps Apache-2.0, model licenses, and zxing-cpp Apache-2.0 for barcode scanning on phones); the Licenses screen in the app lists the main ones.
- Repository public from the first commit with `LICENSE`, `README.md`, `SECURITY.md`, `CONTRIBUTING.md`, `THIRD_PARTY.md`, a public roadmap and this spec. Everything public is in English.
- Releases are built by GitHub Actions only, with SHA-256 checksums. Planned: an SBOM and a VirusTotal link; SignPath Foundation code signing after the first release; Microsoft Store and winget next; Android via GitHub Releases, IzzyOnDroid and F-Droid. Google Play developer verification will be completed through a registered association before the 2027 global rollout.
- Privacy statement (plain language): no accounts, no telemetry, no crash upload. The app talks to the network only for (1) the daily update check, asked about at setup and switchable off, (2) downloads the user starts, (3) online research the user turns on. Barcode scanning on phones runs on the phone itself and never goes online. The complete list of hosts is published in `docs/network.md`.
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

Each step ends with a build that can be installed and tested.

## 14. Defaults for 1.0

Single install folder chosen at install · Android 9 minimum · 64-bit Windows 10/11 only · household password ≥ 8 characters · Home content as in §6 · unknown barcodes named once and remembered · expiry reminders as an in-app list only · backup/restore included · "Basic pack for Serbia" / "English essentials" recommended by language · logo done · no deadline; work proceeds in risk order.
