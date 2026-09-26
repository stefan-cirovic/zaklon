# Testing

One command runs everything:

```
bash scripts/test-all.sh
```

The same checks run on GitHub Actions (Windows) for every push and pull request, see `.github/workflows/ci.yml`.

## What is covered

| Layer | Where | What it proves |
|---|---|---|
| Unit tests | `crates/*/src/**` (`#[cfg(test)]`) | Password hashing, tokens, catalog sanity, Serbian transliteration, supplies rules (quantities never below zero, expiry buckets, running low, barcodes, places, shopping list, history), download hashing, search result parsing. |
| Hub end-to-end | `crates/zaklon-hub/tests/e2e.rs` | A real hub in a fresh folder on free ports, driven like the desktop window and a paired phone: setup, TLS pinning (a wrong fingerprint is refused), public vs private status, CSRF and DNS-rebinding protection, pairing with attempt limits, device-only vs laptop-only permissions, supplies from a phone with history, verified downloads, checksum failure, resume of a partial download, USB export/import, install page and path traversal, discovery beacon, device revocation, password change, and that test port overrides are never saved. |
| Interface end-to-end | `ui/e2e/*.spec.ts` (Playwright) | Clicking through the real interface at laptop and phone size: first-run setup, pairing screen (two QR codes and a code), supplies add/adjust/running low/shopping/history/Home, expired items and delete, language switch, library empty state and add-ons catalog, no screen wider than the device, tab kept in the address. |

## Not covered automatically (yet)

- The Android app itself (camera, barcode scanner, on-device pinned TLS, content proxy) is tested by hand on a real phone.
- The library engine with real knowledge packs (large downloads) is tested by hand on the test hub.

## Useful environment variables

| Variable | Purpose |
|---|---|
| `ZAKLON_ROOT` | Data folder for the hub or desktop app. |
| `ZAKLON_TLS_PORT`, `ZAKLON_LOCAL_PORT`, `ZAKLON_INSTALL_PORT`, `ZAKLON_BEACON_PORT` | Run on other ports (never saved to the configuration). |
| `ZAKLON_IGNORE_BATTERY=1` | Tests only: allow downloads on a laptop below 50% battery. |
| `PW_CHANNEL` | Browser for interface tests: `msedge` (default on Windows) or `chrome`. |
