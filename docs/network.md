# Network activity

Zaklon goes online only for the connections below. All of them are started by the user, except the update check, which runs once a day while it is switched on. The household is asked about it in the first-run setup (the box starts ticked); the switch is also under Household → About → New versions on the laptop. There is no activity log in the app yet; this page is the complete list.

| When | Host | Purpose |
|---|---|---|
| Update check: about a minute after Zaklon starts and then every 24 hours while the switch is on (never before the first-run setup is finished), and whenever someone presses "Check now" | `api.github.com` | Reads the version number of the latest published release. The request carries only the app version (`User-Agent: Zaklon/<version>`), nothing about the household. "Open the download page" opens `github.com` in the browser; Zaklon itself downloads and installs nothing. |
| Knowledge pack download (user starts it) | `lb.download.kiwix.org` (which redirects to a nearby Kiwix mirror), then `mirror.accum.se` if that fails | Knowledge packs (ZIM), verified with SHA-256 |
| First knowledge pack download | `download.kiwix.org` | The library engine (Kiwix tools for Windows), verified with SHA-256 |
| First AI model download | `github.com` (release files are served from `release-assets.githubusercontent.com`) | The AI engine (llama.cpp for Windows), verified with SHA-256 |
| AI model download (user starts it) | `huggingface.co` and the Hugging Face file servers it redirects to (for example `cdn-lfs.hf.co` or `cas-bridge.xethub.hf.co`) | AI model files (GGUF), verified with SHA-256 |
| Map download (user starts it on the Maps screen) | `mapgen-fi-1.comaps.app` | Map files, verified with the SHA-1 CoMaps publishes |
| First map download | `codeberg.org` | The CoMaps app for phones, verified with SHA-256 |
| Online research (switched on per conversation in the Assistant) | `html.duckduckgo.com`, then the first public result pages (never addresses inside the home network) | Web search for the assistant; the pages are listed as sources |
| Phone: barcode scanning, the first time | Google Play services (Google servers) | Barcode scanning on Android uses Google ML Kit through Google Play services. The scanner model is downloaded by Google Play services the first time the scanner is used, and ML Kit sends performance and usage metrics to Google. See [THIRD_PARTY.md](../THIRD_PARTY.md). |
| Planned, not used yet | `packs.zaklon.com` | Zaklon's own content packs and a signed catalog |

The add-on catalog itself is built into the app; Zaklon does not download it. The `library.kiwix.org` links shown with some packs are only links to where the pack is described.

## Local network

Local network traffic (hub ↔ phones) stays on your Wi-Fi:

- TLS on port 8484 for the app.
- Plain HTTP on port 8480 for the "install the app" page, APK downloads and map files for CoMaps only (public map data, nothing private).
- DNS-SD and a UDP beacon on port 8485 for discovery.
- The desktop window talks to the hub on 127.0.0.1:8481, which is not reachable from the network; requests there are accepted only from the hub's own pages (Host and Origin are checked).
- The library engine (kiwix-serve) listens on a random loopback port and is reached only through the hub.
- The AI engine (llama-server) listens on a loopback port only, both on the hub and on phones, and is reached only through Zaklon.
- On phones, the app runs a read-only loopback proxy for library articles, protected by a random secret in the URL.

For tests and development, `ZAKLON_LOOPBACK_ONLY=1` makes the hub listen on 127.0.0.1 only and skip DNS-SD (see [TESTING.md](TESTING.md)).

## Telemetry

Zaklon itself has no telemetry, no crash reporting, no analytics and no account system. The one exception today is Google ML Kit, used for barcode scanning on Android phones (see the table above).

The system web view that draws Zaklon's window (Microsoft Edge WebView2 on Windows, Android System WebView on phones) is part of the operating system. It may contact Microsoft or Google on its own, for example to update itself or for their safe-browsing checks; Zaklon does not control that.
