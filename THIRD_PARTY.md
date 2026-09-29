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
| MapLibre GL JS | Draws the Zaklon map | BSD-3-Clause | Bundled with the interface (loaded when a map is first shown) | https://github.com/maplibre/maplibre-gl-js |
| Protomaps basemap style (`@protomaps/basemaps`) | The map's layers, in Zaklon's own colors | BSD-3-Clause (code), CC0 (design) | Bundled with the interface | https://github.com/protomaps/basemaps |
| pmtiles (Rust crate) | Reads the map's tile archives on the hub | MIT or Apache-2.0 | Compiled into the hub | https://github.com/stadiamaps/pmtiles-rs |
| BLAKE3 (`blake3` Rust crate, with `arrayvec`, `constant_time_eq` and `cpufeatures`) | Checks world map downloads against the BLAKE3 hash Protomaps publishes | CC0-1.0 or Apache-2.0 (or Apache-2.0 with LLVM exception); its helpers MIT or Apache-2.0, and CC0-1.0, MIT-0 or Apache-2.0 | Compiled into the hub | https://github.com/BLAKE3-team/BLAKE3 |
| go-pmtiles (`pmtiles` tool) | Cuts the world overview from the world map when the app is built | BSD-3-Clause | Used only by `scripts/fetch-map-assets.sh`, not shipped | https://github.com/protomaps/go-pmtiles |

## Fonts

| Font | Role | License | Source |
|---|---|---|---|
| Sora (Light 300, Latin subset, bundled with the interface via `@fontsource/sora`) | The "ZAKLON" wordmark | SIL Open Font License 1.1 | https://github.com/sora-xor/sora-font |
| Noto Sans (Regular, Medium, Italic, Devanagari), as map glyphs from `protomaps/basemaps-assets`, in the installer's map assets | The Zaklon map's labels | SIL Open Font License 1.1 | https://github.com/protomaps/basemaps-assets |

## Map assets (in the Windows installer, made by `scripts/fetch-map-assets.sh`)

| Asset | License | Attribution |
|---|---|---|
| World overview (zoom 0 to 5), cut from the Protomaps basemap build | ODbL 1.0 (map data); the low zoom levels come from Natural Earth (public domain) | © OpenStreetMap contributors; shown on the map as "Protomaps © OpenStreetMap" with a link to openstreetmap.org/copyright. The style is Zaklon's own colors on the Protomaps design. |
| Map icons (sprites) from `protomaps/basemaps-assets` | MIT | Derived from tangrams/icons, Copyright (c) 2017 Mapzen |
| List of places for "find a place" (GeoNames cities5000, trimmed, with Serbian names from GeoNames' alternate names) | CC BY 4.0 | GeoNames (geonames.org) |

## Content packs (downloaded on request)

The knowledge packs are ZIM files published by Kiwix (openZIM).

| Pack | License | Attribution |
|---|---|---|
| Wikipedia in Serbian and English (including "Best of Serbian Wikipedia"), Medical Wikipedia (ZIM) | CC BY-SA 4.0 | Wikipedia contributors; each article links to its source. Images in the "with pictures" packs keep their own licenses. |
| Wiktionary in Serbian (ZIM) | CC BY-SA 4.0 | Wiktionary contributors |
| Wikibooks in Serbian and English (ZIM) | CC BY-SA 4.0 | Wikibooks contributors |
| Wikisource in Serbian (ZIM) | CC BY-SA 4.0 (most texts are in the public domain) | Wikisource contributors |
| Project Gutenberg books in Serbian (ZIM) | Public domain | Project Gutenberg |
| Stack Exchange sites: Gardening & Landscaping, Sustainable Living, Seasoned Advice (cooking), Homebrewing, Home Improvement, Woodworking, Motor Vehicle Maintenance & Repair, Bicycles (ZIM) | CC BY-SA (2.5, 3.0 or 4.0, by post date) | Stack Exchange contributors |
| NHS Medicines A to Z (ZIM) | Open Government Licence v3.0 | Contains public sector information licensed under the Open Government Licence v3.0 (NHS website, nhs.uk) |
| USDA Complete Guide to Home Canning (ZIM) | Public domain (U.S. government work) | U.S. Department of Agriculture |
| First aid and field medicine manuals (irp.fas.org military medicine, ZIM) | Public domain (U.S. government works) | U.S. Department of Defense and U.S. Coast Guard, collected by the Federation of American Scientists |
| Ready.gov emergency preparedness (ZIM) | Public domain (U.S. government work) | FEMA / Ready.gov; third-party pictures keep their own terms; no endorsement by FEMA is implied |
| WikEM emergency medicine (ZIM) | CC BY-SA 4.0 | WikEM contributors |
| Appropedia (ZIM) | CC BY-SA 4.0 (older pages CC BY-SA 3.0) | Appropedia contributors |
| Energypedia (ZIM) | CC BY-SA 3.0 and 4.0 | energypedia contributors; documents it links to keep their own terms |
| Restarters repair wiki (ZIM) | CC BY-SA 4.0 | Restarters Wiki contributors (The Restart Project) |
| Public Domain Recipes (ZIM) | Public domain (Unlicense) | publicdomainrecipes.com contributors |
| Gardenology plant encyclopedia (ZIM) | CC BY-SA 3.0 | Gardenology.org contributors |
| Kiwix maps of Serbia, the Balkans, Montenegro, and Bosnia and Herzegovina (ZIM) | ODbL 1.0 (map data), CC BY 4.0 (place search) | © OpenStreetMap contributors; tiles by OpenFreeMap, © OpenMapTiles; place search © GeoNames |
| Map data | ODbL 1.0 | © OpenStreetMap contributors |
| World map for the Zaklon map (a Protomaps basemap build, PMTiles, downloaded from build.protomaps.com: the newest build Protomaps keeps that is at least a week old, or the build pinned in the app) | ODbL 1.0 | © OpenStreetMap contributors; tiles built by Protomaps (https://protomaps.com) |
| Map region list and region names (bundled in `crates/zaklon-core/catalog/comaps-*.json`, Serbian names converted to Latin script) | Apache-2.0 | CoMaps contributors |
| AI models (Qwen3.5 0.8B, 2B, 4B, 9B) | Apache-2.0 | Qwen team, Alibaba Cloud; GGUF conversions by ggml-org (0.8B) and Unsloth (2B, 4B, 9B) |

Packs people download themselves (listed apart and downloaded only after the person confirms the license; never in a starter set):

| Pack | License | Attribution |
|---|---|---|
| iFixit repair guides (ZIM) | CC BY-NC-SA 3.0 | iFixit and its contributors. Non-commercial: copies, including packs copied to a USB stick, must not be sold. |
| GrimGrains plant-based recipes (ZIM) | CC BY-NC-SA 4.0 | GrimGrains by Hundred Rabbits. Non-commercial. |
| Hundred Rabbits off-grid notes (ZIM) | CC BY-NC-SA 4.0 | Hundred Rabbits. Non-commercial. |
| Kiwix guide collection: safe water (ZIM) | Various; see each document | Various authors, collected by Kiwix |

No longer offered (a household that has them keeps them): the Kiwix guide collections on first aid and medicine and on food preparation, which contain commercially published books and titles that need their publisher's permission for digital use.

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
