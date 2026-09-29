#!/usr/bin/env bash
# Build the phone app, then the Windows installer that carries it (and the
# map's assets: the world overview, fonts, icons and places).
#   scripts/build-all.sh            (debug APK: installable, for testing)
# Outputs:
#   apps/zaklon/src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk
#   target/release/bundle/nsis/Zaklon_<version>_x64-setup.exe
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
# Android and Java paths: taken from the environment when set (without the
# quotes some shells carry along), otherwise the defaults below.
unquote() { printf '%s' "$1" | tr -d "\"'"; }
DEFAULT_TOOLS='D:\DevTools'
GRADLE_USER_HOME="$(unquote "${GRADLE_USER_HOME:-$DEFAULT_TOOLS\\gradle}")"
ANDROID_HOME="$(unquote "${ANDROID_HOME:-$DEFAULT_TOOLS\\Android\\Sdk}")"
ANDROID_SDK_ROOT="$ANDROID_HOME"
JAVA_HOME="$(unquote "${JAVA_HOME:-$DEFAULT_TOOLS\\jdk\\jdk-17.0.20.1+1}")"
NDK_HOME="$(unquote "${NDK_HOME:-$ANDROID_HOME\\ndk\\27.1.12297006}")"
export GRADLE_USER_HOME ANDROID_HOME ANDROID_SDK_ROOT JAVA_HOME NDK_HOME
# The day and the commit of this build, as the system specification (Settings ›
# About) names them, the same in the phone app and the installer (read by
# crates/zaklon-hub/build.rs and ui/vite.config.ts). Taken as given when set.
ZAKLON_BUILD_DATE="${ZAKLON_BUILD_DATE:-$(date -u +%Y-%m-%d)}"
ZAKLON_BUILD_COMMIT="${ZAKLON_BUILD_COMMIT:-$(git rev-parse HEAD 2>/dev/null || true)}"
export ZAKLON_BUILD_DATE ZAKLON_BUILD_COMMIT
echo "== build $ZAKLON_BUILD_DATE, commit ${ZAKLON_BUILD_COMMIT:-unknown}"

echo "== map assets (made once, then taken from the cache)"
bash scripts/fetch-map-assets.sh

echo "== phone app"
pnpm tauri android build --debug --target aarch64
apk="apps/zaklon/src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk"
cp "$apk" apps/zaklon/src-tauri/windows/zaklon.apk

echo "== Windows installer (with the phone app inside)"
pnpm tauri build
ls -la target/release/bundle/nsis/*.exe
