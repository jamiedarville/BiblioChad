#!/usr/bin/env bash
# Download a prebuilt PDFium into src-tauri/resources/pdfium/.
# Usage: scripts/fetch-pdfium.sh [platform]   (default: linux-x64)
# Platforms: win-arm64, win-x64, linux-x64, linux-arm64, mac-arm64, mac-x64
set -euo pipefail
PLATFORM="${1:-linux-x64}"
VERSION="${PDFIUM_VERSION:-latest}"
DEST="$(cd "$(dirname "$0")/.." && pwd)/src-tauri/resources/pdfium"
if [ "$VERSION" = latest ]; then
  URL="https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-$PLATFORM.tgz"
else
  URL="https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F$VERSION/pdfium-$PLATFORM.tgz"
fi
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
curl -fsSL "$URL" -o "$TMP/pdfium.tgz"
tar -xzf "$TMP/pdfium.tgz" -C "$TMP"
mkdir -p "$DEST"
case "$PLATFORM" in
  win-*) cp "$TMP/bin/pdfium.dll" "$DEST/" ;;
  mac-*) cp "$TMP/lib/libpdfium.dylib" "$DEST/" ;;
  *)     cp "$TMP/lib/libpdfium.so" "$DEST/" ;;
esac
cp "$TMP/LICENSE" "$DEST/PDFIUM_LICENSE.txt" 2>/dev/null || true
echo "PDFium ($PLATFORM, $(cat "$TMP/VERSION" 2>/dev/null | tr '\n' ' ')) -> $DEST"
