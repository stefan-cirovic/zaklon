# Security Policy

Zaklon is designed to hold a household's private data. Reports of security problems are taken seriously and handled quickly.

## Reporting a vulnerability

Please do not open a public issue for security problems.

Please use GitHub's private vulnerability reporting (Security tab → Report a vulnerability).

<!-- TODO(owner): enable private vulnerability reporting in repo settings -->

You will get an acknowledgment within 72 hours and a status update at least every two weeks until the issue is resolved. Fixed issues are credited in the release notes unless you ask otherwise.

## Scope

- The hub application (Windows), the Android app, the installer and the update check.
- Download verification (SHA-256 / SHA-1 checks of every pack), and add-on catalog signing once it is added.
- Pairing, device tokens and TLS, backup encryption and restore (and profile encryption once profiles ship).

Third-party programs that Zaklon runs as separate processes (kiwix-serve, llama.cpp, CoMaps) have their own security policies; issues in them should be reported upstream, but we welcome a heads-up so we can ship a mitigation.

## What we promise

- No accounts, no cloud services and no telemetry of our own. The one exception today is Google ML Kit, used for barcode scanning on Android, which sends usage metrics to Google. See `docs/network.md` for every host the app can contact and why.
- Releases are built by GitHub Actions and published with SHA-256 checksums (`SHA256SUMS.txt`).
- Not in place yet (planned before 1.0): a signed Windows installer, a release-signed Android app and a signed add-on catalog. Until then, check downloads against the published checksums.
