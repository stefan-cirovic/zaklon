#!/usr/bin/env bash
# Download the official llama.cpp Android (arm64) build and place the server
# and its libraries where the Android app packages them. The files are not
# kept in git; run this before building the APK.
#   scripts/fetch-android-llama.sh [release-tag]
set -euo pipefail
TAG="${1:-b11202}"
SHA256="${LLAMA_ANDROID_SHA256:-}"
here="$(cd "$(dirname "$0")" && pwd)"
dest="$here/../apps/zaklon/src-tauri/gen/android/app/src/main/llama-libs/arm64-v8a"
ndk="${NDK_HOME:-/d/DevTools/Android/Sdk/ndk/27.1.12297006}"
strip="$(ls "$ndk"/toolchains/llvm/prebuilt/*/bin/llvm-strip* | head -1)"
tmp="$(mktemp -d)"
url="https://github.com/ggml-org/llama.cpp/releases/download/$TAG/llama-$TAG-bin-android-arm64.tar.gz"
echo "downloading $url"
curl -sSL -o "$tmp/l.tgz" "$url"
if [ -n "$SHA256" ]; then echo "$SHA256  $tmp/l.tgz" | sha256sum -c -; fi
tar -xzf "$tmp/l.tgz" -C "$tmp"
src="$(find "$tmp" -maxdepth 1 -type d -name "llama-*" | head -1)"
rm -rf "$dest" && mkdir -p "$dest"
# Android extracts only files named lib*.so; the server executable is renamed so
# it lands next to its libraries in the app's native library folder.
cp "$src/llama-server" "$dest/libllama_server_exec.so"
for f in libllama-server-impl.so libllama-common.so libmtmd.so libllama.so libggml.so libggml-base.so "$src"/libggml-cpu-android_*.so; do
  cp "$src/$(basename "$f")" "$dest/"
done
for f in "$dest"/*.so; do "$strip" --strip-unneeded "$f"; done
echo "$TAG" > "$dest/../LLAMA_VERSION"
rm -rf "$tmp"
du -ch "$dest"/*.so | tail -1
ls "$dest"
