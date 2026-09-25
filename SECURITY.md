# Security Policy

Zaklon is designed to hold a household's private data. Reports of security problems are taken seriously and handled quickly.

## Reporting a vulnerability

Please do not open a public issue for security problems.

- Use GitHub's private vulnerability reporting on this repository ("Security" tab → "Report a vulnerability"), or
- e-mail the maintainer at the address listed on the GitHub profile of `stefan-cirovic`.

You will get an acknowledgement within 72 hours and a status update at least every two weeks until the issue is resolved. Fixed issues are credited in the release notes unless you ask otherwise.

## Scope

- The hub application (Windows), the Android app, the installer and the update mechanism.
- The add-on catalog signing and download verification.
- Pairing, device tokens, TLS and profile encryption.

Third-party programs that Zaklon runs as separate processes (kiwix-serve, llama.cpp, CoMaps) have their own security policies; issues in them should be reported upstream, but we welcome a heads-up so we can ship a mitigation.

## What we promise

- No telemetry, no accounts, no cloud services. See `docs/network.md` for every host the app can contact and why.
- Releases are built by GitHub Actions from tagged commits, with published checksums.
- Signing keys (release signing, catalog signing) are held by the maintainer and rotated if compromised; the public keys are published in this repository.
