# Testing

One command runs everything:

```
bash scripts/test-all.sh
```

GitHub Actions (Windows) runs most of these checks on every push to `main` and every pull request (see `.github/workflows/ci.yml`). The phone offline-logic check and clippy/tests of the app crate (`zaklon-app`) run only in `scripts/test-all.sh`; CI only checks that the app crate compiles.

## What is covered

| Layer | Where | What it proves |
|---|---|---|
| Unit tests | `crates/*/src/**` (`#[cfg(test)]`) | Password hashing, tokens, catalog sanity (every knowledge pack has known topics, maps are about maps, AI models and programs about none; an older catalog's single topic is read as its topics), Serbian transliteration, supplies rules (quantities never below zero, expiry buckets, running low, barcodes, places, shopping list, history), download hashing, search result parsing. |
| Hub end-to-end | `crates/zaklon-hub/tests/e2e.rs` | A real hub in a fresh folder on free ports, driven like the desktop window and a paired phone: setup, TLS pinning (a wrong fingerprint is refused), public vs private status, CSRF and DNS-rebinding protection, pairing by QR code with its secret (never the 6-digit code) and attempt limits, device-only vs laptop-only permissions, supplies from a phone with history, verified downloads, checksum failure, resume of a partial download, USB export/import, install page and path traversal, discovery beacon, device revocation, password change, and that test port overrides are never saved. |
| Pairing from "Find hubs" | `crates/zaklon-pake` (unit), `crates/zaklon-hub/tests/pake.rs`, `apps/zaklon/src-tauri/src/client.rs` (unit) | The code check with SPAKE2 against a real hub: the right code pairs under the name the check came with (and a lost reply is recovered), a check without a name takes no attempt, a wrong code is caught on the phone, one address may use only two of a code's three tries and the third burns it, the password is looked at only after the phone's proof, each check is answered once, a device in between with its own certificate is caught before the password is sent, the pairing requests do not exist on the laptop's loopback port, and too many failed checks block the address. The phone app's own pairing code (by QR code and from "Find hubs") is run against a real hub too. |
| Saved conversations | `crates/zaklon-core/src/conversations.rs` (unit), `crates/zaklon-hub/tests/conversations.rs` | Questions are saved as asked and completed when the answer finishes; titles come from the first question; each device (the laptop, each phone) sees, renames, deletes and asks in only its own, and a phone cannot reach the laptop's; a copy sent to another device is marked with the sender's name and changes nothing of the original; search finds titles and questions with or without diacritics; the limits hold (oldest conversations make room, a full conversation is refused); removing a phone deletes its conversations; a restore keeps only those of devices that may connect. |
| Backups and restore | `crates/zaklon-hub/src/backup.rs`, `crates/zaklon-core/src/db.rs` (unit) | Plain and encrypted backups round trip; wrong passwords and damaged or swapped files are refused; a restore keeps this hub's phones, password and identity (also with no phone paired, and after an interruption between the folder moves) and takes them only for another hub's backup on a new install; a backup database with triggers or views is refused and its triggers never run; what a backup really holds decides, not its manifest; unpacking is limited; unencrypted pieces stay in the data folder and are cleaned up. |
| Phone offline logic | `ui/e2e/unit/offline.check.mts` (plain Node, 23.6 or newer) | The phone's cached copy of the supplies and the shopping-list outbox that waits for the hub; the copy of the saved conversations (the list and the last 30 opened). |
| User guide | `ui/e2e/unit/help.check.mts` (plain Node, 23.6 or newer) | The English and Serbian help have the same topics, sections, kinds of blocks, steps and links; every link leads to a screen, a Household category or setting, an Add-ons folder or a help section that exists; every Household category has its section. |
| Interface end-to-end | `ui/e2e/*.spec.ts` (Playwright) | Clicking through the real interface at laptop and phone size: first-run setup, pairing screen (two QR codes and a code), supplies add/adjust/running low/shopping/history/Home, expired items and delete, language switch, library empty state and add-ons catalog (the folders are the topics; a pack about several topics is in the folder of each and counted in each; the addresses of the folders from before the topics lead to the new ones), no screen wider than the device, tab kept in the address, the Tools screen sorted by topic (each topic with its tools, a tool about several topics under each, and its guides opening Add-ons at that topic's folder, in English and Serbian) and pinning a tool to the bar (and unpinning it), the logo opening the website; Household's tiles and category pages (back arrow, browser back, the list beside the page on a laptop, 44 px touch targets on a phone), its search (English and Serbian words, Enter and arrow keys) and addresses that open a category or one setting; the assistant's saved conversations (a question starts one, it is in the list under Today, opens again after a reload, is renamed, found by search and deleted; a copy is sent to another device, with the device list simulated; a copy from another device names its sender); Help (opened from Tools, a topic with its links, Next and Back; "How it works" on every screen opening its topic, and a Household page its own section; the guide in Serbian; the search, with or without accents, and Enter opening the section found). |

## Not covered automatically (yet)

- The Android app itself (camera, barcode scanner, on-device pinned TLS, content proxy) is tested by hand on a real phone.
- The library engine with real knowledge packs (large downloads) is tested by hand on a test hub.

## Useful environment variables

| Variable | Purpose |
|---|---|
| `ZAKLON_ROOT` | Data folder for the hub or desktop app. |
| `ZAKLON_TLS_PORT`, `ZAKLON_LOCAL_PORT`, `ZAKLON_INSTALL_PORT`, `ZAKLON_BEACON_PORT` | Run on other ports (never saved to the configuration). |
| `ZAKLON_LOOPBACK_ONLY=1` | Tests and development: listen on 127.0.0.1 only and skip DNS-SD, so nothing listens on the network and Windows does not ask about its firewall (never saved). The hub end-to-end test and the interface tests set it. |
| `ZAKLON_IGNORE_BATTERY=1` | Tests only: allow downloads on a laptop below 50% battery. |
| `PW_CHANNEL` | Browser for interface tests: `msedge` (default on Windows) or `chrome`. |
| `ZAKLON_E2E_PORT_BASE` | Interface tests: the hub's test ports (default 28480: install 28480, local 28481, TLS 28484, beacon 28485), e.g. 38480 for a second run on the same machine. |
