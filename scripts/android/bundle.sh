#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
: "${BIBLIOCHAD_ANDROID_KEYSTORE:?Set the absolute path to your upload keystore}"
: "${BIBLIOCHAD_ANDROID_STORE_PASSWORD:?Set the keystore password}"
: "${BIBLIOCHAD_ANDROID_KEY_PASSWORD:?Set the upload key password}"
case "${VITE_BIBLIOCHAD_PRIVACY_URL:-}" in
  https://*) ;;
  *) echo 'Set VITE_BIBLIOCHAD_PRIVACY_URL to your published HTTPS privacy policy.' >&2; exit 1 ;;
esac
npx tauri android build --aab --target aarch64 --ci
