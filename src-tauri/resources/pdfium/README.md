Place the PDFium shared library for the target platform here before
bundling (`pdfium.dll` for Windows ARM64, from
https://github.com/bblanchon/pdfium-binaries, asset `pdfium-win-arm64.tgz`,
file `bin/pdfium.dll`). `scripts/fetch-pdfium.ps1` / `scripts/fetch-pdfium.sh`
do this for you. The library is not committed to git.
