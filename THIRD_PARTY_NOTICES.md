# Third-party notices

BiblioChad bundles or vendors the following components.

## PDFium

- Source: https://pdfium.googlesource.com/pdfium/ (prebuilt binaries from
  https://github.com/bblanchon/pdfium-binaries)
- License: BSD 3-Clause and Apache License 2.0 (PDFium), plus the licenses of
  its third-party dependencies as listed in the `LICENSE` file shipped with
  the pdfium-binaries archive. Copy that file next to `pdfium.dll` when
  packaging (`scripts/fetch-pdfium.*` does this).

## foliate-js

- Source: https://github.com/johnfactotum/foliate-js (see
  `ui/src/vendor/foliate-js/VENDORED.md` for the pinned commit)
- License: MIT. Copyright (c) 2022 John Factotum.
- foliate-js vendors zip.js (BSD 3-Clause, Gildas Lormeau) and fflate (MIT,
  Arjun Barrett).

## Rust crates and npm packages

Dependencies are listed in `Cargo.lock` and `ui/package-lock.json`. Run
`cargo about` or `npx license-checker` to produce a full report for a
release.
