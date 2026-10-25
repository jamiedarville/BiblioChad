#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
npx tauri android init --ci --skip-targets-install
python3 scripts/android/configure.py
python3 scripts/android/pdfium.py "$@"
