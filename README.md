<p align="center"><img src="assets/cover.jpg" alt="BiblioChad comic cover: Chad in glasses, arms crossed, in a library. Finish the book. No excuses."></p>

# BiblioChad

> An ebook reader for Windows on ARM, written in Rust, whose library *is* your Google Drive folder tree.
> Reads PDF and EPUB, remembers where you stopped, and has the personality of a man who finishes every book he starts.

**Finish the book.**

The design document is [`docs/project.md`](docs/project.md). This README covers building and running the app, and how the code maps to that plan.

## What works today

| Area | Status |
|---|---|
| **EPUB** (foliate-js): paginated and scroll modes, TOC, themes (light/sepia/dark), font, size, spacing, margins, line length, justification, hyphenation, in-book search, highlights in 4 colors, notes, copy | ✅ |
| **PDF** (PDFium in Rust): continuous and single-page, fit width/page, zoom (Ctrl+wheel), outline, selectable text layer, search, dark-mode inversion | ✅ |
| **Position memory**: saved ~2 s after the last page turn and before the window closes; restored on open | ✅ |
| **Google Drive**: OAuth loopback + PKCE, refresh token in Windows Credential Manager, folder browser (My Drive + Shared Drives), resumable crawl, incremental `changes.list` sync, shortcuts, md5-verified downloads | ✅ (needs your OAuth client, see below) |
| **Library**: folder = shelf with breadcrumbs, All books / Recently read / Favorites / Collections, search, sort, filter, grid and list (virtualized), covers (EPUB/PDF, plus Drive thumbnails before first open), Continue Reading, read status | ✅ |
| **Cross-device sync** of positions, bookmarks and highlights through a hidden `appDataFolder` file, with a "continue from page 212 on your other device?" prompt | ✅ |
| **Offline**: LRU cache with a size cap, "Keep offline" pinning, clear cache | ✅ |
| Local metadata edits, Markdown export of highlights, local file import, "Open with BiblioChad" for .epub/.pdf | ✅ |
| Chad Mode (jokes, ranks, streaks, achievements) with an off switch | ✅ |
| **Windows installers** (NSIS + MSI + portable zip) for **ARM64** (`windows-11-arm` runner) and **x64** (`windows-latest`) | ✅ in CI |
| PDF highlights, footnote pop-ups, TTS, reading stats beyond streaks | ❌ not yet (P1/P2) |

## Repository layout

```
crates/
  bc-core/      shared types: formats, locators, read status, paths, ranks
  bc-reader/    EPUB container/OPF parsing (zip-slip + bomb guards), script sanitizer, PDFium worker
  bc-library/   SQLite schema + migrations, Drive mirror, queries, progress, notes, cache, sync import/export
  bc-sync/      the sync document and its commutative merge (LWW per book, tombstoned notes)
  bc-drive/     OAuth (loopback + PKCE), keyring token store, Drive v3 client
src-tauri/      app shell: commands, bibliochad:// protocol, Drive sync glue, packaging
ui/             Svelte 5 + TypeScript UI; vendored foliate-js under ui/src/vendor
scripts/        fetch-pdfium.{sh,ps1}, make-test-corpus.py, e2e/smoke.py
```

All logic lives in Rust; the UI is a thin view. Book content reaches the webview only through the `bibliochad://` protocol, which validates archive paths, strips `<script>`/`on*` handlers/`javascript:` URLs, and sends a strict CSP.

## Building on Windows (ARM64 or x64)

1. Install **Rust** with `rustup` (host `aarch64-pc-windows-msvc`; check with `rustc -vV`).
2. Install **Visual Studio 2022 Build Tools** with the MSVC build tools for your CPU (*ARM64* or *x64/x86*) and a Windows 11 SDK.
3. Install **Node.js 22** (the build matching your CPU).
4. Get PDFium: `.\scripts\fetch-pdfium.ps1 -Platform win-arm64` (or `-Platform win-x64` on an Intel/AMD PC).
5. Install JS deps: `npm ci; npm --prefix ui ci`.
6. Run in dev mode: `npx tauri dev`. Build the installer with `npx tauri build`. Output goes to `target\release\bundle\nsis\` and `...\msi\`.

The installer bundles `pdfium.dll` and embeds the WebView2 bootstrapper, so it also works on a clean machine.

Every push also builds both Windows versions in CI. Download `bibliochad-windows-arm64` (Snapdragon and other ARM PCs) or `bibliochad-windows-x64` (Intel/AMD PCs) from the run's **Artifacts**. Builds are unsigned, so SmartScreen will warn on first run.

## Connecting Google Drive

BiblioChad is a personal app, so you bring your own OAuth client:

1. In [Google Cloud console](https://console.cloud.google.com/), create a project and enable the **Google Drive API**.
2. Under **OAuth consent screen**, choose *External* (or *Internal* on Workspace) and add the scopes `drive.readonly` and `drive.appdata`. Add yourself as a test user.
3. **Set the publishing status to "In production".** In *Testing*, refresh tokens expire after 7 days. An unverified production app shows a warning screen, which is fine for your own use.
4. Under **Credentials**, create an OAuth client ID of type **Desktop app** and download its JSON.
5. In BiblioChad: **Settings → Google Drive → Import client JSON…**, then **Connect Google Drive**, then pick your library folder.

The JSON is stored as `%LOCALAPPDATA%\BiblioChad\google-client.json`. You can instead bake a client in at build time with `BIBLIOCHAD_GOOGLE_CLIENT_ID` / `BIBLIOCHAD_GOOGLE_CLIENT_SECRET`. For desktop apps Google does not treat the secret as confidential.

The only thing BiblioChad ever writes to Drive is `bibliochad-sync.json` in the hidden app-data folder.

## Development

```bash
# Linux deps for the shell: libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev
scripts/fetch-pdfium.sh linux-x64        # or win-arm64 / mac-arm64 ...
python3 scripts/make-test-corpus.py      # test-corpus/chad-test.{epub,pdf}

cargo test --workspace                   # Rust unit tests (Drive tests use a mock server)
PDFIUM_DIR=src-tauri/resources/pdfium BC_TEST_PDF=test-corpus/chad-test.pdf \
  cargo test -p bc-reader                # also exercise real PDFium rendering
(cd ui && npm run check && npm test)     # svelte-check + vitest

npx tauri dev                            # run the app
target/debug/bibliochad book.epub        # import + open a file directly
```

**End-to-end smoke test.** `scripts/e2e/smoke.py` drives the real app through `tauri-driver`. It opens the corpus EPUB, checks the book's `<script>` didn't run, turns pages, uses the TOC, returns to the library, opens the PDF, scrolls, searches, and saves screenshots. CI runs it under Xvfb and uploads the screenshots.

Useful environment variables:

| Variable | Purpose |
|---|---|
| `BIBLIOCHAD_DATA_DIR` | use a different data folder (default `%LOCALAPPDATA%\BiblioChad`) |
| `PDFIUM_DIR` | folder containing `pdfium.dll` / `libpdfium.so` |
| `BIBLIOCHAD_LOG` | log filter, e.g. `debug` (logs go to `<data>\logs\bibliochad.log`) |

## Decisions taken from the plan

The plan's recommended options were adopted: **D1** Tauri 2 + WebView2, **D2** foliate-js (pinned commit, behind `ui/src/lib/epub/foliate.ts`), **D3** PDFium via `pdfium-render`, **D4** TypeScript + Svelte 5 thin UI, **D5** SQLite (`rusqlite`, bundled), **D6** loopback + PKCE with Credential Manager, **D7** `drive.readonly` + `drive.appdata`, **D8** `reqwest` with SChannel on Windows (rustls elsewhere). Revisit any of them in `docs/project.md`.

## Licensing

No license has been chosen for BiblioChad's own code yet. Third-party components and their licenses are listed in [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).
