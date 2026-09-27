# Zaklon

**An offline home base for your household: knowledge, maps, supplies and a local AI assistant that keep working when the internet does not.**

*Zaklon* is the Serbian word for shelter. A laptop runs the hub; the phones in your home connect to it over Wi-Fi, with or without internet. It is built for the bad day, and it is useful on every ordinary day in between.

> Status: early development, not released yet. The hub, phone pairing, library, maps, supplies, the assistant, backups and add-ons work; profiles and the first release are next. Follow the [roadmap](docs/roadmap.md).

## What it does

- **Library.** Offline Wikipedia, medical reference, repair guides and more, searchable from every phone in the house.
- **Maps.** The whole world in pieces (countries and regions), downloaded once by the hub and served to phones running [CoMaps](https://comaps.app), without internet.
- **Supplies.** What you have, where it is, when it expires and what is running low, with batches that each keep their own expiry date, a shopping list and a "put away" step. Barcode scanning from the phone. A phone away from home keeps its last copy, and its shopping list keeps working.
- **Assistant.** A local AI model on the hub that answers from your library and names its sources, answers about your supplies and proposes changes to them (nothing changes until you confirm), remembers what you ask it to remember, and, only when you switch it on for a conversation, researches online. Phones use it at home and fall back to a smaller model of their own.
- **Household.** Pairing phones with a code, daily backups and backups to USB, copying packs to USB for another household, and (planned) profiles for each person and a laptop that can become the Wi-Fi network when there is none.

## Principles

1. **Offline by default.** Internet is optional.
2. **Local only.** No accounts, no telemetry, no cloud. Your data lives in one folder you can copy to a USB stick.
3. **Free forever.** Licensed under GPL-3.0-or-later. Donations are welcome and never unlock features.
4. **Transparent.** Public repository from the first commit. Every network call is user-initiated and listed in [docs/network.md](docs/network.md).
5. **Light.** Small installer, low idle memory, no animations.

## How it is built

A Rust hub daemon on Windows (Linux and macOS later), a React interface shared by the desktop window and the Android app (Tauri 2), SQLite for data, and two independent open-source programs run as separate processes: [Kiwix](https://kiwix.org) for the library and [llama.cpp](https://github.com/ggml-org/llama.cpp) for the assistant. Maps use [CoMaps](https://comaps.app) with data © OpenStreetMap contributors. See [docs/SPEC.md](docs/SPEC.md) for the full specification and [THIRD_PARTY.md](THIRD_PARTY.md) for licenses.

Inspired by projects such as [Internet-in-a-Box](https://internet-in-a-box.org), [Project NOMAD](https://github.com/ProjectNOMAD-Offline/NOMAD) and Kiwix Hotspot, which showed that offline knowledge hubs work. Zaklon adds the household layer, a real phone app and a Windows-first setup.

## Why the name

"Zaklon" is short, easy to say in any language, and means exactly what the project is: the place you go when everything else is down. It is understood across Serbia, Croatia, Bosnia, Montenegro, Slovenia and North Macedonia, and it was free of conflicting apps and domains when we checked.

## Installing (Windows)

Run `Zaklon_<version>_x64-setup.exe`. It installs for the current user, needs no internet (everything it needs is inside), and asks where to put Zaklon: the program and all household data (`data` folder) live in that one folder. Zaklon starts with Windows and keeps running in the tray when its window is closed, so phones stay connected; use the tray icon to quit or to turn off starting with Windows. Uninstalling removes the program but keeps the `data` folder.

On a phone on the same Wi-Fi, open `http://<laptop address>:8480/get` (the address and a QR code are shown under Household → Add a phone) to install the Android app.

## Contributing and support

- Read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request.
- Security issues: see [SECURITY.md](SECURITY.md).
- Donations: channels will be listed here once the first release is out.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
