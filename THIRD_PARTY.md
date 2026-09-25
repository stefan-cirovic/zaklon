# Third-party software and content

Zaklon runs several independent open-source programs as separate processes and offers freely licensed content packs. Each keeps its own license. Zaklon is not affiliated with or endorsed by any of these projects.

## Programs bundled or downloaded by the installer

| Component | Role | License | Source |
|---|---|---|---|
| Kiwix (kiwix-serve, kiwix-tools) | Serves offline knowledge packs (ZIM) | GPL-3.0-or-later | https://github.com/kiwix/kiwix-tools |
| llama.cpp (llama-server) | Runs local AI models | MIT | https://github.com/ggml-org/llama.cpp |
| CoMaps | Offline maps app (Android APK served by the hub) | Apache-2.0 | https://codeberg.org/comaps/comaps |

## Content packs (downloaded on request)

| Pack | License | Attribution |
|---|---|---|
| Wikipedia, Wiktionary, Medical Wikipedia (ZIM) | CC BY-SA 4.0 | Wikipedia contributors; each article links to its source |
| iFixit (ZIM) | CC BY-NC-SA 3.0 | iFixit |
| Stack Exchange sites (ZIM) | CC BY-SA 4.0 | Stack Exchange contributors |
| Map data | ODbL 1.0 | © OpenStreetMap contributors |
| AI models (Qwen3.5 family, Gemma 4) | Apache-2.0 | Alibaba Cloud (Qwen), Google (Gemma) |

## Libraries

The Rust and JavaScript dependency lists with their licenses are generated at release time into `THIRD_PARTY_LIBRARIES.md` and shipped with every build (`cargo about`, `pnpm licenses`).

Trademark notice: product names above are used only to identify the respective projects. Logos are not used.
