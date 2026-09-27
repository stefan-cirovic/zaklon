#!/usr/bin/env bash
# Build the phone app, then the Windows installer that carries it.
#   scripts/build-all.sh            (debug APK: installable, for testing)
# Outputs:
#   apps/zaklon/src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk
#   target/release/bundle/nsis/Zaklon_<version>_x64-setup.exe
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
# Paths on this project's Windows build machine (the shell may carry quoted ones).
GRADLE_USER_HOME='D:\DevTools\gradle'
ANDROID_HOME='D:\DevTools\Android\Sdk'
ANDROID_SDK_ROOT="$ANDROID_HOME"
JAVA_HOME='D:\DevTools\jdk\jdk-17.0.20.1+1'
NDK_HOME='D:\DevTools\Android\Sdk\ndk\27.1.12297006'
export GRADLE_USER_HOME ANDROID_HOME ANDROID_SDK_ROOT JAVA_HOME NDK_HOME

echo "== phone app"
pnpm tauri android build --debug --target aarch64
apk="apps/zaklon/src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk"
cp "$apk" apps/zaklon/src-tauri/windows/zaklon.apk

echo "== Windows installer (with the phone app inside)"
pnpm tauri build
ls -la target/release/bundle/nsis/*.exe
