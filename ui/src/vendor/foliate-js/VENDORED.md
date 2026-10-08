# foliate-js (vendored)

- Upstream: https://github.com/johnfactotum/foliate-js
- Commit: 78914aef4466eb960965702401634c2cb348e9b1 (2026-05-01)
- License: MIT (see LICENSE)

Only the modules BiblioChad uses are copied. Local changes:

- `pdf.js` is replaced by a stub; PDFs are rendered with PDFium in Rust.
- `vendor/pdfjs` is not included.

To update: copy the same files from a newer commit, re-apply the stub, and
update the commit above. BiblioChad talks to foliate-js only through
`src/lib/epub/foliate.ts` so it can be swapped (risk R6).
