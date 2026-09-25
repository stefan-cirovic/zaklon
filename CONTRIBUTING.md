# Contributing to Zaklon

Thank you for considering a contribution. Zaklon is a small project with a clear direction, so please read this page before opening a pull request.

## Ground rules

- Everything in this repository is in English: code, comments, commit messages, documentation and issues.
- Offline first. A feature that only works with internet access needs a very good reason.
- Local only. Do not add telemetry, analytics, accounts or cloud dependencies. Any new network call must be user-initiated and documented in `docs/network.md`.
- Keep it light. No animations, no heavy dependencies for small gains, and the idle memory target in `docs/SPEC.md` is a hard budget.
- License: contributions are accepted under GPL-3.0-or-later.

## How to contribute

1. Open an issue first for anything larger than a typo or a small fix, so we can agree on the approach.
2. Fork, branch from `main`, keep pull requests focused on one change.
3. Run the checks (`cargo fmt`, `cargo clippy`, `pnpm lint`, tests) before pushing.
4. Describe what changed and why in the pull request; link the issue.

## Areas where help is welcome

- Translations (the UI ships in English and Serbian; more languages are welcome once the string files stabilise).
- Content packs: curated, freely licensed knowledge for the add-on catalog.
- Testing on different laptops and Android phones.
- Reviews of the pairing and encryption code.

## Code of conduct

Be kind and constructive. Harassment or personal attacks are not tolerated. The maintainer may remove comments or block contributors who do not follow this.
