# Third-party software and content

Zaklon runs several independent programs as separate processes, includes some third-party components in its installer and Android app, and offers content packs from other projects. Each keeps its own license. Zaklon is not affiliated with or endorsed by any of these projects.

## Programs Zaklon runs, bundles or downloads

| Component | Role | License | Arrives | Source |
|---|---|---|---|---|
| Kiwix tools (kiwix-serve) | Library engine | GPL-3.0-or-later | Downloaded by the hub with the first knowledge pack | https://github.com/kiwix/kiwix-tools |
| llama.cpp (llama-server) | AI engine | MIT | Downloaded by the hub with the first AI model; bundled in the Android app | https://github.com/ggml-org/llama.cpp |
| CoMaps | Offline maps app for phones | Apache-2.0 | Downloaded by the hub with the first map, then offered to phones | https://codeberg.org/comaps/comaps |
| Microsoft Edge WebView2 Runtime | Draws the desktop window | Microsoft Software License Terms (proprietary) | Bundled in the Windows installer, which installs it only if Windows does not have it yet | https://developer.microsoft.com/microsoft-edge/webview2/ |
| zxing-cpp (its Android library, `io.github.zxing-cpp:android`) | Reads barcodes and QR codes on phones, on the phone itself | Apache-2.0 | Bundled in the Android app | https://github.com/zxing-cpp/zxing-cpp |
| AndroidX (among them CameraX, the camera view of the barcode scanner), Material Components for Android, and the libraries these use (among them Kotlin, Guava and Dagger) | Android app libraries | Apache-2.0 | Bundled in the Android app | https://developer.android.com/jetpack/androidx |
| NSIS | Windows installer | zlib/libpng | The Windows installer is built with it | https://nsis.sourceforge.io |

## Fonts

| Font | Role | License | Source |
|---|---|---|---|
| Sora (Light 300, Latin subset, bundled with the interface via `@fontsource/sora`) | The "ZAKLON" wordmark | SIL Open Font License 1.1 | https://github.com/sora-xor/sora-font |

## Content packs (downloaded on request)

The knowledge packs are ZIM files published by Kiwix (openZIM).

| Pack | License | Attribution |
|---|---|---|
| Wikipedia in Serbian and English, Medical Wikipedia (ZIM) | CC BY-SA 4.0 | Wikipedia contributors; each article links to its source. Images in the "with pictures" packs keep their own licenses. |
| Wiktionary in Serbian (ZIM) | CC BY-SA 4.0 | Wiktionary contributors |
| Wikibooks in Serbian (ZIM) | CC BY-SA 4.0 | Wikibooks contributors |
| iFixit repair guides (ZIM) | CC BY-NC-SA 3.0 | iFixit and its contributors. Non-commercial: copies, including packs copied to a USB stick, must not be sold. |
| Stack Exchange sites: Gardening & Landscaping, Sustainable Living (ZIM) | CC BY-SA (2.5, 3.0 or 4.0, by post date) | Stack Exchange contributors |
| NHS Medicines A to Z (ZIM) | Open Government Licence v3.0 | Contains public sector information licensed under the Open Government Licence v3.0 (NHS website, nhs.uk) |
| USDA Complete Guide to Home Canning (ZIM) | Public domain (U.S. government work) | U.S. Department of Agriculture |
| Gardenology plant encyclopedia (ZIM) | CC BY-SA 3.0 | Gardenology.org contributors |
| Kiwix guide collections: safe water, first aid and medicine, food preparation (ZIM) | Various; see each document | Various authors, collected by Kiwix |
| Map data | ODbL 1.0 | © OpenStreetMap contributors |
| Map region list and region names (bundled in `crates/zaklon-core/catalog/comaps-*.json`, Serbian names converted to Latin script) | Apache-2.0 | CoMaps contributors |
| AI models (Qwen3.5 0.8B, 2B, 4B, 9B) | Apache-2.0 | Qwen team, Alibaba Cloud; GGUF conversions by ggml-org (0.8B) and Unsloth (2B, 4B, 9B) |

## Figures the tools calculate with

Only numbers and methods are used; no text is copied. The sources do not endorse Zaklon.

| Tool | Figures | Source |
|---|---|---|
| Water calculator | Drinking water per person (one gallon a day) | Ready.gov, https://www.ready.gov/water (U.S. government work) |
| Water calculator | Water for basic hygiene (15 L per person a day) | The Sphere Handbook (2018), water supply standard 2.1 |
| Water calculator | Boiling times and bleach amounts | CDC, https://www.cdc.gov/water-emergency/about/index.html, and EPA, https://www.epa.gov/ground-water-and-drinking-water/emergency-disinfection-drinking-water (U.S. government works) |
| Water calculator | Crop coefficients | USDA NRCS, National Engineering Handbook Part 623, Chapter 2 (1993; public domain) |
| Water calculator | Reference evapotranspiration | Hargreaves and Samani (1985); extraterrestrial radiation by the formulas of FAO Irrigation and Drainage Paper 56 (Allen et al. 1998) |
| Water calculator | Gravity drip kits (a bucket or drum about 1 m up) | Palada M. et al. 2011. More Crop Per Drop. AVRDC – The World Vegetable Center (CC BY-SA 3.0) |

## Libraries

Zaklon is built with Tauri, React, Rust crates (among them axum, tokio, rustls and reqwest) and SQLite (public domain, compiled in through rusqlite). Backups are encrypted in the standard age format with the `age` crate (MIT or Apache-2.0, https://github.com/str4d/rage), so the `age` tool can open them too. A generated list of all Rust and JavaScript libraries and their licenses is not produced yet; it is planned to ship with every build. Until then, the complete dependency lists are `Cargo.lock` and `pnpm-lock.yaml` in this repository.

Trademark notice: product names above are used only to identify the respective projects. Logos are not used.
