<p align="center"><img src="assets/cover.jpg" alt="BiblioChad comic cover: Chad in glasses, arms crossed, in a library. Finish the book. No excuses."></p>

# BiblioChad

> An ebook reader for Windows ARM64 and Arch Linux x64, written in Rust, whose library *is* your Google Drive folder tree.
> Reads PDF and EPUB, remembers where you stopped, and has the personality of a man who finishes every book he starts.

**Finish the book.**

The design document is [`docs/project.md`](docs/project.md). This README covers downloading, building and running the app, and how the code maps to that plan.

## Download

| Your PC | Installer (recommended) | Other options |
|---|---|---|
| **ARM** (Snapdragon, Surface Pro X, Copilot+ PCs) | [**Download for Windows ARM64**](https://github.com/jamiedarville/BiblioChad/releases/latest/download/BiblioChad-Setup-arm64.exe) | [MSI](https://github.com/jamiedarville/BiblioChad/releases/latest/download/BiblioChad-arm64.msi) · [Portable zip](https://github.com/jamiedarville/BiblioChad/releases/latest/download/BiblioChad-Portable-arm64.zip) |

These links always fetch the newest [release](https://github.com/jamiedarville/BiblioChad/releases/latest). BiblioChad is only built for **Windows ARM64** and **Arch Linux x64**; there is no Windows x64 or Android build.

- **Installer** (`.exe`): installs for your user, adds a Start menu entry, and shows up in *Installed apps*.
- **MSI**: the same app as an MSI package, for managed or all-users installs.
- **Portable zip**: no install; unzip and run `bibliochad.exe` (keep the `pdfium` folder next to it).

Builds aren't code-signed yet, so Windows SmartScreen will warn on first run: click **More info → Run anyway**.

### Arch Linux (x86_64)

Works on Arch and its derivatives (CachyOS, EndeavourOS, Manjaro):

```bash
sudo pacman -U https://github.com/jamiedarville/BiblioChad/releases/latest/download/bibliochad-x86_64.pkg.tar.zst
```

Or build the package from a checkout: `cd packaging/arch && makepkg -si`.

The Google Drive sign-in is kept in your desktop keyring through the Secret Service API, so one of GNOME Keyring, KWallet or KeePassXC needs to be running. GNOME and KDE Plasma set this up for you.

**Making a release:** bump the version in `src-tauri/tauri.conf.json`, the workspace `Cargo.toml`, both `package.json` files and their lockfiles, the workspace packages in `Cargo.lock`, and `packaging/arch/PKGBUILD`. Update this README and commit. Then either push a matching tag (`git tag v0.1.3 && git push origin v0.1.3`) or open **Actions → Release → Run workflow**. Both build the Windows ARM64 installers plus the Arch Linux package, and publish them as a GitHub Release.

## New in v0.1.3

- **Build a lab**: the Learning-zone LLM key can now analyse the whole book and build a series of practical labs — setup instructions, step-by-step work, a diagram per lab, self-checks and troubleshooting. Labs are saved with the book, load again on every open, and export as Markdown.
- **Learning zone** (from the unreleased work after v0.1.2): per-page Zone of Proximal Development maps with graduated hints, self-checks and difficulty feedback, using your own Anthropic or OpenAI-compatible API key.
- **Platforms narrowed**: BiblioChad is now built only for **Windows ARM64** and **Arch Linux x64**. The Android app and the Windows x64 build have been removed.

## New in v0.1.2

- Fixed the Linux book context menu closing when you release the right mouse button. Right-click once, release, and choose **Add to Collection** without holding the button down.
- Click outside the menu with the left mouse button, or press **Escape**, to close it. Keyboard access with **Shift+F10** or the **Menu** key still works.
- Fixed release publishing when Arch Linux builds include a separate debug package.

## New in v0.1.1

- Right-click a book in grid or list view and choose **Add to Collection**. Pick an existing collection or enter a name and choose **Create and add**. Collections the book already belongs to are marked **Already added**.
- Open the same menu with the keyboard using **Shift+F10** or the **Menu** key while a book is focused. **Book details** remains available in the menu and through the book's **⋯** button.
- Releases now include an **Arch Linux x86_64 package**, alongside Windows x64 and ARM64 installers and portable builds.

Collections are separate from Drive folders, and a book can belong to several. Manage them from the **Collections** tab; use the collection checkboxes in **Book details** to remove a book from a collection.

## Learning zone (bring your own LLM key)

Open the **Learning zone** panel (the ◎ ring button in the reader's toolbar) to map your
[Zone of Proximal Development](https://en.wikipedia.org/wiki/Zone_of_proximal_development) for the page you're on:

- **You've got this**: ideas on the page you can already handle alone, as anchors.
- **Your zone**: 2–4 ideas just out of reach. Each has a *Try it* question and three hints
  that go from a light nudge to a worked step, then a full explanation. Reveal only as much help as you need.
- **Not yet**: ideas that need more groundwork first, and when to come back to them.
- **Check yourself** questions and a suggested next step.

Tell it what you already know and why you're reading the book, and it tailors each map to you. Rate a map
**Too easy**, **Just right** or **Too hard**: the book's *stretch* level (1–5) moves, and the next map adjusts.
Earlier maps feed into later ones, and your highlights on the page are included as hints about what caught your eye.

Set it up in **Settings → Learning assistant**. Anthropic (default model `claude-opus-5-5`) and any
OpenAI-compatible endpoint (OpenAI, OpenRouter, or a local Ollama / LM Studio server) are supported. The key is
stored in the system keychain. Nothing is sent until you press **Map my zone**;
then the page text, your background and goal, and highlights on that page go straight to your provider, and its
charges apply. Maps are saved locally and are not synced to Drive. The code is in `crates/bc-tutor` and
`ui/src/components/ZonePanel.svelte`.

### Build a lab

Open the **Build a lab** panel (the flask button in the reader's toolbar) and the same LLM analyses the
whole book — short books in full, long books sampled evenly from the beginning, middle and end, plus the
contents/outline — and designs a series of hands-on labs: what to set up, ordered step-by-step instructions
with the reason for each step and what you should see, a diagram for every lab (drawn in the panel, kept as
Mermaid in exports), self-checks, troubleshooting and a harder follow-up. Pick 3–7 labs and an optional focus;
your Learning-zone background and goal pitch the level. Generating can take a minute or two and your provider's
charges apply; scanned PDFs with no selectable text can't be labbed.

The guide is saved locally with the book (one per book; rebuilding replaces it), so it loads again every time
the book opens. Tick steps off as you work (ticks stay on this device), export the whole guide as Markdown
from the panel or from **Book details**, and rebuild from there or the panel if the book file changes. Like
zone maps, labs are not synced to Drive. The code is in `crates/bc-tutor` (`lab.rs`, `lab_prompt.rs`),
`crates/bc-library/src/labs.rs` and `ui/src/components/LabPanel.svelte`.

## What works today

| Area | Status |
|---|---|
| **EPUB** (foliate-js): paginated and scroll modes, TOC, themes (light/sepia/dark), font, size, spacing, margins, line length, justification, hyphenation, in-book search, highlights in 4 colors, notes, copy | ✅ |
| **PDF** (PDFium in Rust): continuous and single-page, fit width/page, zoom (Ctrl+wheel), outline, selectable text layer, search, dark-mode inversion | ✅ |
| **Position memory**: saved ~2 s after the last page turn and before the window closes; restored on open | ✅ |
| **Google Drive**: OAuth loopback + PKCE, refresh token in Windows Credential Manager (Secret Service on Linux), folder browser (My Drive + Shared Drives), resumable crawl, incremental `changes.list` sync, shortcuts, md5-verified downloads | ✅ (needs your OAuth client, see below) |
| **Library**: folder = shelf with breadcrumbs, All books / Recently read / Favorites / Collections, search, sort, filter, grid and list (virtualized), covers (EPUB/PDF, plus Drive thumbnails before first open), Continue Reading, read status, right-click **Add to Collection** with inline collection creation | ✅ |
| **Cross-device sync** of positions, bookmarks and highlights through a hidden `appDataFolder` file, with a "continue from page 212 on your other device?" prompt | ✅ |
| **Offline**: LRU cache with a size cap, "Keep offline" pinning, clear cache | ✅ |
| Local metadata edits, Markdown export of highlights, local file import, "Open with BiblioChad" for .epub/.pdf | ✅ |
| Chad Mode (jokes, ranks, streaks, achievements) with an off switch | ✅ |
| **Learning zone**: per-page Zone of Proximal Development maps with graduated hints, self-checks and difficulty feedback, using your own Anthropic or OpenAI-compatible API key | ✅ |
| **Build a lab**: whole-book lab series (setup, step-by-step instructions, per-lab diagrams, checks) generated by the same LLM key, saved with the book and exportable as Markdown | ✅ |
| **Windows ARM64 installers** (NSIS + MSI + portable zip, `windows-11-arm` runner) and **Arch Linux x64 package** | ✅ in CI |
| **Arch Linux package** (`packaging/arch/PKGBUILD`, x86_64) | ✅ in CI |
| PDF highlights, footnote pop-ups, TTS, reading stats beyond streaks | ❌ not yet (P1/P2) |

## Repository layout

```
crates/
  bc-core/      shared types: formats, locators, read status, paths, ranks
  bc-reader/    EPUB container/OPF parsing (zip-slip + bomb guards), script sanitizer, PDFium worker
  bc-library/   SQLite schema + migrations, Drive mirror, queries, progress, notes, cache, sync import/export
  bc-sync/      the sync document and its commutative merge (LWW per book, tombstoned notes)
  bc-drive/     OAuth (loopback + PKCE), keyring token store, Drive v3 client
  bc-tutor/     learning assistant: ZPD prompt + JSON schema, Anthropic and OpenAI-compatible clients
src-tauri/      app shell: commands, bibliochad:// protocol, Drive sync glue, packaging
ui/             Svelte 5 + TypeScript UI; vendored foliate-js under ui/src/vendor
packaging/arch/ PKGBUILD and desktop entry for Arch Linux
scripts/        fetch-pdfium.{sh,ps1}, make-test-corpus.py, e2e/smoke.py
```

All logic lives in Rust; the UI is a thin view. Book content reaches the webview only through the `bibliochad://` protocol, which validates archive paths, strips `<script>`/`on*` handlers/`javascript:` URLs, and sends a strict CSP.

## Building on Windows ARM64

1. Install **Rust** with `rustup` (host `aarch64-pc-windows-msvc`; check with `rustc -vV`).
2. Install **Visual Studio 2022 Build Tools** with the **MSVC ARM64 build tools** and a Windows 11 SDK.
3. Install **Node.js 22** (the ARM64 build).
4. Get PDFium: `.\scripts\fetch-pdfium.ps1 -Platform win-arm64`.
5. Install JS deps: `npm ci; npm --prefix ui ci`.
6. Run in dev mode: `npx tauri dev`. Build the installer with `npx tauri build`. Output goes to `target\release\bundle\nsis\` and `...\msi\`.

The installer bundles `pdfium.dll` and embeds the WebView2 bootstrapper, so it also works on a clean machine.

Every push also builds the Windows ARM64 version in CI. Download `bibliochad-windows-arm64` from the run's **Artifacts**. Builds are unsigned, so SmartScreen will warn on first run.

## Building on Arch Linux

```bash
sudo pacman -S --needed base-devel git rust nodejs npm webkit2gtk-4.1
cd packaging/arch && makepkg -si      # builds and installs the bibliochad package
```

The `PKGBUILD` builds the checkout it sits in (uncommitted changes included), downloads a pinned PDFium, and installs the binary to `/usr/bin/bibliochad`, PDFium to `/usr/lib/bibliochad/pdfium/`, and a desktop entry that registers BiblioChad for `.epub` and `.pdf`. For day-to-day development use `npx tauri dev` as described under [Development](#development).

## Connecting Google Drive

BiblioChad is a personal app, so you bring your own OAuth client:

1. In [Google Cloud console](https://console.cloud.google.com/), create a project and enable the **Google Drive API**.
2. Under **OAuth consent screen**, choose *External* (or *Internal* on Workspace) and add the scopes `drive.readonly` and `drive.appdata`. Add yourself as a test user.
3. **Set the publishing status to "In production".** In *Testing*, refresh tokens expire after 7 days. An unverified production app shows a warning screen, which is fine for your own use.
4. Under **Credentials**, create an OAuth client ID of type **Desktop app** and download its JSON.
5. In BiblioChad: **Settings → Google Drive → Import client JSON…**, then **Connect Google Drive**, then pick your library folder.

The JSON is stored as `%LOCALAPPDATA%\BiblioChad\google-client.json` (`~/.local/share/bibliochad/google-client.json` on Linux). You can instead bake a client in at build time with `BIBLIOCHAD_GOOGLE_CLIENT_ID` / `BIBLIOCHAD_GOOGLE_CLIENT_SECRET`. For desktop apps Google does not treat the secret as confidential.

The only thing BiblioChad ever writes to Drive is `bibliochad-sync.json` in the hidden app-data folder.

## Development

```bash
# Linux deps for the shell (Debian/Ubuntu): libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libdbus-1-dev
# On Arch: webkit2gtk-4.1 (see "Building on Arch Linux")
scripts/fetch-pdfium.sh linux-x64        # or win-arm64 / mac-arm64 ...
python3 scripts/make-test-corpus.py      # test-corpus/chad-test.{epub,pdf}

cargo test --workspace                   # Rust unit tests (Drive tests use a mock server)
PDFIUM_DIR=src-tauri/resources/pdfium BC_TEST_PDF=test-corpus/chad-test.pdf \
  cargo test -p bc-reader                # also exercise real PDFium rendering
(cd ui && npm run check && npm test)     # svelte-check + vitest

npx tauri dev                            # run the app
target/debug/bibliochad book.epub        # import + open a file directly
```

**End-to-end smoke test.** `scripts/e2e/smoke.py` drives the real app through `tauri-driver`. It opens the corpus EPUB, checks the book's `<script>` didn't run, turns pages, uses the TOC, returns to the library, opens the PDF, scrolls, searches, and saves screenshots. It also verifies that releasing right-click keeps the book menu open, adds a book to a collection, and checks keyboard access and menu dismissal. CI runs it under Xvfb on Linux WebKit and uploads the screenshots.

Useful environment variables:

| Variable | Purpose |
|---|---|
| `BIBLIOCHAD_DATA_DIR` | use a different data folder (default `%LOCALAPPDATA%\BiblioChad`, or `~/.local/share/bibliochad` on Linux) |
| `PDFIUM_DIR` | folder containing `pdfium.dll` / `libpdfium.so` |
| `BIBLIOCHAD_LOG` | log filter, e.g. `debug` (logs go to `<data>\logs\bibliochad.log`) |

## Decisions taken from the plan

The plan's recommended options were adopted: **D1** Tauri 2 + WebView2, **D2** foliate-js (pinned commit, behind `ui/src/lib/epub/foliate.ts`), **D3** PDFium via `pdfium-render`, **D4** TypeScript + Svelte 5 thin UI, **D5** SQLite (`rusqlite`, bundled), **D6** loopback + PKCE with Credential Manager, **D7** `drive.readonly` + `drive.appdata`, **D8** `reqwest` with SChannel on Windows (rustls elsewhere). Revisit any of them in `docs/project.md`.

## Licensing

No license has been chosen for BiblioChad's own code yet. Third-party components and their licenses are listed in [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).
