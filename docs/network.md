# Network activity

Zaklon makes no network connections on its own. Every connection below happens only when the user triggers it, and each is visible in the app's activity log.

| When | Host | Purpose |
|---|---|---|
| Update check (only if enabled at install or in settings) | `api.github.com`, `github.com` | Read the latest release version; download the installer or APK when the user confirms |
| Add-on download (user starts it) | `library.kiwix.org`, `download.kiwix.org` and Kiwix mirrors | Catalog and knowledge packs (ZIM) |
| Add-on download (user starts it) | CoMaps map servers (as published by CoMaps) | Map files and the CoMaps APK |
| Add-on download (user starts it) | `huggingface.co` | AI model files (GGUF) |
| Add-on download (user starts it) | `packs.zaklon.com` (Cloudflare R2) | Zaklon's own content packs and the signed catalog |
| Online research (per-conversation switch) | `duckduckgo.com`; `api.search.brave.com` if the user adds a key; then the pages the assistant opens | Web search for the assistant |

Local network traffic (hub ↔ phones) stays on your Wi-Fi: TLS on port 8484 for the app, plain HTTP on port 8480 for the "install the app" page and APK downloads only, DNS-SD and a UDP beacon on port 8485 for discovery. The desktop window talks to the hub on 127.0.0.1:8481, which is not reachable from the network; requests there are accepted only from the hub's own pages (Host and Origin are checked). The library engine (kiwix-serve) listens on a random loopback port and is reached only through the hub. On phones, the app runs a read-only loopback proxy for library articles, protected by a random secret in the URL.

There is no telemetry, no crash reporting, no analytics and no account system.
