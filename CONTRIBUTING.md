# Contributing to Zaklon

Thank you for considering a contribution. Zaklon is a small project with a clear direction, so please read this page before opening a pull request.

## Ground rules

- Everything in this repository is in English (American spelling): code, comments, commit messages, documentation and issues. The exceptions are the Serbian translation (`ui/src/i18n.ts`), the assistant's Serbian prompts and Serbian test data.
- Offline first. A feature that only works with internet access needs a very good reason.
- Local only. Do not add telemetry, analytics, accounts or cloud dependencies. Any new network call must be listed in `docs/network.md` and, like the existing ones, be started by the user or be possible to switch off.
- Keep it light. No animations, no heavy dependencies for small gains, and the idle memory target in `docs/SPEC.md` is a hard budget.
- License: contributions are accepted under GPL-3.0-or-later.

## How to contribute

1. Open an issue first for anything larger than a typo or a small fix, so we can agree on the approach.
2. Fork, branch from `main`, keep pull requests focused on one change.
3. Run `bash scripts/test-all.sh` (interface build, clippy, unit and end-to-end tests) and `pnpm --filter ui lint` before pushing. Match the formatting of the file you edit: the Rust code is not run through `cargo fmt`, so please do not reformat unrelated code.
4. Describe what changed and why in the pull request; link the issue.

## Building from source

Zaklon is built on Windows 10 or 11 (64-bit). Run the scripts from Git Bash or another Bash shell.

### Tools

| Tool | Version | Needed for |
|---|---|---|
| Rust (with the MSVC toolchain and the Microsoft C++ Build Tools) | stable | Everything |
| Node.js | 22 or newer (23.6 or newer for the phone offline-logic check) | The interface |
| pnpm | 10 | The interface |
| Microsoft Edge or Google Chrome | current | Interface end-to-end tests |
| JDK | 17 | The Android app |
| Android SDK | platform 36, build-tools 36.0.0 | The Android app |
| Android NDK | 27.1.12297006 | The Android app |
| Rust target `aarch64-linux-android` | | The Android app (`rustup target add aarch64-linux-android`) |

The Android build reads these environment variables. Check the top of `scripts/build-all.sh` and make sure they point to your own installation:

| Variable | Points to |
|---|---|
| `ANDROID_HOME` (and `ANDROID_SDK_ROOT`) | The Android SDK folder |
| `NDK_HOME` | The NDK folder, for example `$ANDROID_HOME/ndk/27.1.12297006` |
| `JAVA_HOME` | The JDK 17 folder |
| `GRADLE_USER_HOME` | Optional: where Gradle keeps its cache |

### Commands

```
pnpm install                                      # JavaScript dependencies
pnpm --filter ui build                            # the interface into ui/dist (the hub serves it, so build it first)
cargo run -p zaklon-hub -- --root <data folder>   # the hub alone; open http://127.0.0.1:8481 in a browser
pnpm tauri dev                                    # the desktop app (debug build)
```

A debug build of the hub reads `ui/dist` from disk, so after changing the interface, run `pnpm --filter ui build` again and reload the page. A release build embeds `ui/dist` in the program.

Full build of the phone app and the Windows installer that carries it:

```
bash scripts/fetch-android-llama.sh   # llama.cpp for Android, placed where the app packages it (needs NDK_HOME)
bash scripts/build-all.sh             # debug APK, then the Windows installer
```

`scripts/fetch-android-llama.sh` takes an optional llama.cpp release tag and checks the download when `LLAMA_ANDROID_SHA256` is set. The installer ends up in `target/release/bundle/nsis/`. Release builds are made by `.github/workflows/release.yml`.

Tests are described in [docs/TESTING.md](docs/TESTING.md).

## Areas where help is welcome

- Translations (the UI ships in English and Serbian; more languages are welcome once the string files stabilize).
- Content packs: curated, freely licensed knowledge for the add-on catalog.
- Testing on different laptops and Android phones.
- Reviews of the pairing and TLS code.

## Code of conduct

Be kind and constructive. Harassment or personal attacks are not tolerated. The maintainer may remove comments or block contributors who do not follow this.
