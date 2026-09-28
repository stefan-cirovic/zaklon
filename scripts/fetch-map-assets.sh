#!/usr/bin/env bash
# Make the Zaklon map's assets that ship with the app (the installer puts them
# next to the program; the hub serves them), and keep them in a cache so later
# builds reuse the very same files:
#   - overview.pmtiles: the whole world at zoom 0-5 (Protomaps build 20260928,
#     about 15 MB), cut with the pmtiles tool from a local copy of the world
#     map when there is one, else from the build's address (reads only ~15 MB);
#   - fonts/ and sprites/: the map style's glyphs and icons, from
#     protomaps/basemaps-assets at a pinned commit (git checks every file
#     against the commit);
#   - cities.tsv: the places for "find a place" (GeoNames cities5000, trimmed).
# None of this is kept in git.
#   scripts/fetch-map-assets.sh
# Environment (all optional):
#   ZAKLON_CACHE           cache folder (E:/ZaklonDev/cache/maps on the build machine)
#   ZAKLON_TOOLS           where the pmtiles tool is kept (E:/ZaklonDev/tools)
#   ZAKLON_WORLD_PMTILES   a local copy of the world map, only read
#   MAP_ASSETS_OUT         output folder (apps/zaklon/src-tauri/windows/map-assets)
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/.." && pwd)"

if [ -d /e/ZaklonDev ]; then dev=/e/ZaklonDev; else dev="$HOME/.cache/zaklon"; fi
CACHE="${ZAKLON_CACHE:-$dev/cache/maps}"
TOOLS="${ZAKLON_TOOLS:-$dev/tools}"
OUT="${MAP_ASSETS_OUT:-$repo/apps/zaklon/src-tauri/windows/map-assets}"
mkdir -p "$CACHE" "$TOOLS"

# ---- the map build and the overview -------------------------------------------
BUILD=20260928
BUILD_URL="https://build.protomaps.com/$BUILD.pmtiles"
BUILD_SIZE=138415942566
OVERVIEW_MAXZOOM=5
WORLD="${ZAKLON_WORLD_PMTILES:-$dev/maps/protomaps-world-$BUILD.pmtiles}"

# ---- the pmtiles tool (go-pmtiles, BSD-3-Clause) -------------------------------
PMTILES_VERSION=1.31.2
case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*)
    pm_asset="go-pmtiles_${PMTILES_VERSION}_Windows_x86_64.zip"
    pm_sha256=a658baa4d7e55020aef6ca17bd9ff9faa1582671266b36f58c52db0ac8e785a1
    pm_exe=pmtiles.exe ;;
  Linux)
    pm_asset="go-pmtiles_${PMTILES_VERSION}_Linux_x86_64.tar.gz"
    pm_sha256=3ed7dbf4ec2e6dfe5e25b6f70d1ffc932729f93c86db353bf514dd71010a312f
    pm_exe=pmtiles ;;
  *) echo "no pmtiles tool known for $(uname -s)" >&2; exit 1 ;;
esac

# ---- the style's fonts and icons ------------------------------------------------
ASSETS_REPO=https://github.com/protomaps/basemaps-assets.git
ASSETS_COMMIT=028c18f713baecad011301ff7a69acc39bcc2ae7
FONTS=("Noto Sans Regular" "Noto Sans Medium" "Noto Sans Italic" "Noto Sans Devanagari Regular v1")
SPRITES=(dark.json dark.png dark@2x.json dark@2x.png)

# ---- the places (GeoNames, CC BY 4.0) ------------------------------------------
GEONAMES=https://download.geonames.org/export/dump

fetch() { # url file
  curl --fail --proto '=https' --proto-redir '=https' --tlsv1.2 -sSL --retry 3 -o "$2" "$1"
}

# Temporary files next to the cache (the build machine keeps big files off C:).
tmp="$(mktemp -d "$CACHE/tmp.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT

echo "== pmtiles tool $PMTILES_VERSION"
pm_dir="$TOOLS/pmtiles/$PMTILES_VERSION"
pmtiles="$pm_dir/$pm_exe"
if [ ! -x "$pmtiles" ] || [ "$(cat "$pm_dir/.sha256" 2>/dev/null)" != "$pm_sha256" ]; then
  fetch "https://github.com/protomaps/go-pmtiles/releases/download/v$PMTILES_VERSION/$pm_asset" "$tmp/$pm_asset"
  echo "$pm_sha256  $tmp/$pm_asset" | sha256sum -c -
  rm -rf "$pm_dir" && mkdir -p "$pm_dir"
  case "$pm_asset" in
    *.zip) unzip -q -o "$tmp/$pm_asset" -d "$pm_dir" ;;
    *) tar -xzf "$tmp/$pm_asset" -C "$pm_dir" ;;
  esac
  echo "$pm_sha256" > "$pm_dir/.sha256"
fi
"$pmtiles" version 2>/dev/null || true

echo "== world overview (zoom 0-$OVERVIEW_MAXZOOM of build $BUILD)"
overview="$CACHE/protomaps-overview-$BUILD-z$OVERVIEW_MAXZOOM.pmtiles"
if [ -f "$overview" ] && [ -f "$overview.sha256" ]; then
  # Made before: it must still be exactly what was recorded then.
  (cd "$CACHE" && sha256sum -c "$(basename "$overview").sha256")
else
  if [ -f "$WORLD" ] && [ "$(stat -c %s "$WORLD")" = "$BUILD_SIZE" ]; then
    # Read from the local copy (never written); the tool wants a plain file name.
    echo "cutting it from $WORLD"
    (cd "$(dirname "$WORLD")" && "$pmtiles" extract "$(basename "$WORLD")" "$tmp/overview.pmtiles" --maxzoom=$OVERVIEW_MAXZOOM)
  else
    echo "cutting it from $BUILD_URL"
    "$pmtiles" extract "$BUILD_URL" "$tmp/overview.pmtiles" --maxzoom=$OVERVIEW_MAXZOOM
  fi
  "$pmtiles" verify "$tmp/overview.pmtiles"
  mv "$tmp/overview.pmtiles" "$overview"
  (cd "$CACHE" && sha256sum "$(basename "$overview")" > "$(basename "$overview").sha256")
fi

echo "== style fonts and icons (basemaps-assets $ASSETS_COMMIT)"
assets="$CACHE/basemaps-assets-$ASSETS_COMMIT"
if [ "$(git -C "$assets" rev-parse HEAD 2>/dev/null)" != "$ASSETS_COMMIT" ] || [ -n "$(git -C "$assets" status --porcelain 2>/dev/null)" ]; then
  rm -rf "$assets"
  git init -q "$assets"
  git -C "$assets" -c fetch.fsckObjects=true fetch -q --depth 1 "$ASSETS_REPO" "$ASSETS_COMMIT"
  git -C "$assets" checkout -q FETCH_HEAD
fi
# The commit's hash covers every file in it.
test "$(git -C "$assets" rev-parse HEAD)" = "$ASSETS_COMMIT"
test -z "$(git -C "$assets" status --porcelain)"

echo "== places (GeoNames cities5000, with Serbian names from its alternate names)"
day="$(ls "$CACHE"/geonames-*.sha256 2>/dev/null | sed 's/.*geonames-\([0-9]*\)\.sha256/\1/' | sort | tail -1 || true)"
if [ -z "$day" ] || [ -n "${ZAKLON_REFRESH_CITIES:-}" ]; then
  # GeoNames updates its lists every day, so there is no fixed checksum: the
  # copy fetched first is kept, with its checksums, and used from then on.
  day="$(date +%Y%m%d)"
  fetch "$GEONAMES/cities5000.zip" "$tmp/cities5000.zip"
  fetch "$GEONAMES/admin1CodesASCII.txt" "$tmp/admin1.txt"
  # About 200 MB, read once to find the Serbian names.
  fetch "$GEONAMES/alternateNamesV2.zip" "$tmp/alternates.zip"
  mv "$tmp/cities5000.zip" "$CACHE/geonames-cities5000-$day.zip"
  mv "$tmp/admin1.txt" "$CACHE/geonames-admin1-$day.txt"
  mv "$tmp/alternates.zip" "$CACHE/geonames-alternates-$day.zip"
  (cd "$CACHE" && sha256sum "geonames-cities5000-$day.zip" "geonames-admin1-$day.txt" "geonames-alternates-$day.zip" > "geonames-$day.sha256")
fi
(cd "$CACHE" && sha256sum -c "geonames-$day.sha256")
unzip -q -o "$CACHE/geonames-cities5000-$day.zip" -d "$tmp"
unzip -p "$CACHE/geonames-alternates-$day.zip" alternateNamesV2.txt |
  node "$here/map-cities.mjs" "$tmp/cities5000.txt" "$CACHE/geonames-admin1-$day.txt" "$tmp/cities.tsv" -

echo "== putting the assets together in $OUT"
rm -rf "$OUT"
mkdir -p "$OUT/fonts" "$OUT/sprites"
cp "$overview" "$OUT/overview.pmtiles"
for f in "${FONTS[@]}"; do
  mkdir -p "$OUT/fonts/$f"
  # Only a font's own files: where it has none of its own (the Devanagari
  # font outside Devanagari), the repository links to Noto Sans Regular,
  # which the hub serves in its place.
  git -C "$assets" ls-files -s -- "fonts/$f" | awk -F'\t' '$1 ~ /^100644 / { print $2 }' | while IFS= read -r rel; do
    cp "$assets/$rel" "$OUT/fonts/$f/"
  done
done
cp "$assets/fonts/OFL.txt" "$OUT/fonts/OFL.txt"
for s in "${SPRITES[@]}"; do
  cp "$assets/sprites/v4/$s" "$OUT/sprites/"
done
cp "$tmp/cities.tsv" "$OUT/cities.tsv"
cat > "$OUT/ATTRIBUTION.txt" <<EOF
The Zaklon map's assets

overview.pmtiles
  The world at zoom 0-$OVERVIEW_MAXZOOM, cut from the Protomaps basemap build $BUILD.
  Map data (c) OpenStreetMap contributors, Open Database License 1.0
  (https://www.openstreetmap.org/copyright). Low zoom levels use Natural Earth
  (public domain). Tiles built with the Protomaps basemap profile (BSD-3-Clause).

fonts/
  Noto Sans glyphs from protomaps/basemaps-assets ($ASSETS_COMMIT),
  SIL Open Font License 1.1 (fonts/OFL.txt).

sprites/
  Map icons from protomaps/basemaps-assets ($ASSETS_COMMIT), derived from
  tangrams/icons: MIT License, Copyright (c) 2017 Mapzen.

cities.tsv
  Places from GeoNames (https://www.geonames.org), Creative Commons
  Attribution 4.0 License. Trimmed to names (with the Serbian name from
  GeoNames' alternate names), region, country, position and population.
EOF
(cd "$OUT" && sha256sum overview.pmtiles cities.tsv > manifest.sha256)
for f in overview.pmtiles fonts sprites cities.tsv; do du -s --apparent-size --block-size=1K "$OUT/$f"; done
