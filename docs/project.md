# BiblioChad

> An ebook reader for Windows ARM64, written in Rust, whose library *is* your Google Drive folder tree. Reads PDF and EPUB, remembers where you stopped, and has the personality of a man who finishes every book he starts.

**Status:** Planning  |  **Owner:** Jamie  |  **Target platform:** Windows 11 on ARM64 (`aarch64-pc-windows-msvc`)  |  **Last updated:** 2026-10-08

---

## 1. Goals and non-goals

### Goals (v1.0)
1. Read **PDF** and **EPUB** files natively on Windows ARM64, with no x64 emulation.
2. Sign in to Google Drive, pick **one folder**, and show a **Library whose structure mirrors that folder tree** (sub-folders become shelves).
3. **Remember reading position** per book (page / CFI), resume instantly, and sync it across devices.
4. Cover the reader-side features people actually use in Calibre: covers, metadata, search/sort/filter, collections, bookmarks, highlights, notes, themes, and typography controls.
5. Be fast on large libraries (thousands of books) and usable offline for books already opened or pinned.
6. Be funny. The tone is Chad. See section 13.

### Non-goals (v1.0)
- Format conversion, DRM removal, or editing book files (Calibre's library-management and conversion side).
- Writing to Drive. The Drive scope for the library is **read-only**; the app never modifies or deletes your books.
- MOBI/AZW3/CBZ (candidates for v1.x, see open question 5 in section 15).
- Content server, send-to-device, store integrations, plugins.
- Multi-user accounts, or a hosted backend. Everything runs on-device plus your own Drive.

---

## 2. Key decisions

| # | Decision | Recommendation | Why | Status |
|---|----------|----------------|-----|--------|
| D1 | App/UI approach | **Tauri 2 (Rust core + WebView2 UI)** | EPUB is HTML/CSS. A webview gives correct typography, selection, and highlighting for free. A pure-native UI (egui/iced/Slint) would need an HTML/CSS layout engine, which is months of work for worse results. WebView2 ships with Windows 11 and has a native ARM64 build. | **Needs your sign-off** |
| D2 | EPUB rendering | **foliate-js** (MIT) inside the webview, fed by a Rust custom protocol | Handles pagination, CFI, fixed-layout, RTL, and highlights; used by the Foliate reader. Verify maintenance/license in M0. | Proposed |
| D3 | PDF rendering | **PDFium via `pdfium-render`**, page bitmaps + text layer from Rust | Fidelity, speed, and Rust-side control (range loading from Drive). PDFium publishes `win-arm64` builds. Fallback: pdf.js in the webview. | Proposed |
| D4 | Frontend language | **TypeScript (Svelte or Solid), thin UI**; all logic in Rust | foliate-js needs JS glue regardless. Keeping the UI thin keeps the app's real logic in Rust. Alternative: Leptos (Rust/WASM) for an all-Rust codebase, at a velocity cost. | **Needs your sign-off** |
| D5 | Local database | **SQLite** via `rusqlite` (bundled) | Single file, fast, easy backup, no server. | Proposed |
| D6 | Drive auth | OAuth 2.0 installed-app flow, **loopback redirect + PKCE**; refresh token in **Windows Credential Manager** (`keyring`) | Standard for desktop apps; tokens never touch disk in plaintext. | Proposed |
| D7 | Drive scopes | `drive.readonly` (library) + `drive.appdata` (progress sync file) | `drive.file` can't see files the app didn't create, so it can't read an existing library. `appdata` gives a hidden app-owned folder so we never write into your book folder. | Proposed |
| D8 | HTTP/TLS stack | `reqwest` with **`native-tls` (SChannel)** | Avoids C/asm toolchain friction (clang/NASM) that some TLS crates hit on Windows ARM64. Verify in M0. | Proposed |

---

## 3. Architecture

```
+--------------------------------------------------------------+
|  UI (WebView2)                                                |
|  Library views | Reader (foliate-js / PDF canvas) | Settings  |
+------------------------------^-------------------------------+
                               | Tauri commands/events + custom protocol (bibliochad://)
+------------------------------v-------------------------------+
|  Rust core                                                    |
|  +-----------+  +-----------+  +-----------+  +-----------+   |
|  | bc-drive  |  | bc-library|  | bc-reader |  | bc-sync   |   |
|  | OAuth,API |  | SQLite,   |  | EPUB/PDF  |  | progress, |   |
|  | change    |  | metadata, |  | engines,  |  | conflicts |   |
|  | feed      |  | covers    |  | pdfium    |  | appdata   |   |
|  +-----+-----+  +-----+-----+  +-----+-----+  +-----+-----+   |
|        |              |              |              |         |
|  +-----v--------------v--------------v--------------v-----+   |
|  | bc-core: types, errors, config, logging (tracing)       |   |
|  +---------------------------------------------------------+   |
+------------------------------^-------------------------------+
                               |
          Google Drive API v3  |  %LOCALAPPDATA%\BiblioChad (db, cache, covers)
```

### Proposed repo layout
```
bibliochad/
  Cargo.toml                 # workspace
  crates/
    bc-core/                 # shared types, errors, config
    bc-drive/                # OAuth, Drive client, crawl + changes feed
    bc-library/              # SQLite schema, migrations, metadata + cover extraction
    bc-reader/               # EPUB package parsing, PDFium wrapper, text extraction
    bc-sync/                 # reading-position sync, conflict resolution
  src-tauri/                 # app shell, commands, custom protocol, packaging
  ui/                        # TypeScript frontend (Svelte/Solid) + foliate-js
  docs/
  .github/workflows/         # CI incl. ARM64 build
```

### Candidate crates (verify versions and ARM64 build in M0)
`tauri` 2.x, `tokio`, `reqwest` (native-tls), `oauth2`, `serde`/`serde_json`, `rusqlite` (bundled), `keyring`, `pdfium-render`, `zip`, `quick-xml`, `image`, `md-5`/`sha2`, `tracing`, `thiserror`/`anyhow`, `directories`.

---

## 4. Google Drive integration

### Auth flow
1. User clicks **Connect Google Drive**. App starts a loopback listener on `127.0.0.1:<random port>`, opens the system browser to Google's consent page (PKCE, `access_type=offline`).
2. App exchanges the code for tokens and stores the **refresh token in Windows Credential Manager**.
3. Access tokens are refreshed silently; the UI never sees tokens.

> **Gotcha (verified):** for an *External* OAuth consent screen with publishing status **Testing**, refresh tokens expire after **7 days**. For a personal app, set the publishing status to **In production** (it stays unverified, which shows a warning screen, acceptable for one user) or use an *Internal* app type if you're on Google Workspace. Also, tokens unused for six months expire.

### Folder selection
- In-app **folder browser** built on `files.list` with `mimeType='application/vnd.google-apps.folder'` (Google's Picker is a web widget and a poor fit for a desktop shell).
- Store the chosen folder ID as the library root. Support **My Drive and Shared Drives** (`supportsAllDrives=true`, `includeItemsFromAllDrives=true`).

### Crawl and incremental sync
- **Initial crawl:** breadth-first `files.list` with `q="'<folderId>' in parents and trashed=false"` and a minimal `fields` mask (`id,name,mimeType,parents,size,md5Checksum,modifiedTime,shortcutDetails,thumbnailLink`). Persist to the `nodes` table.
- **Incremental:** `changes.list` with a stored `startPageToken`; filter the account-wide feed to nodes whose ancestry includes the library root.
- **Edge cases to handle:** Drive shortcuts (`application/vnd.google-apps.shortcut` → resolve `targetId`), duplicate names in one folder, files with multiple parents, items moved out of scope, trashed items, Google-native docs (ignore), and rate limits (exponential backoff on 403 `rateLimitExceeded`, 429, 5xx).
- Supported file types in v1: `application/epub+zip`, `application/pdf` (also match by extension when Drive reports `application/octet-stream`).

### Download and cache
- Download on demand with `alt=media`; **HTTP Range** requests so large PDFs can open before fully downloaded (PDFium can read through a custom reader).
- Cache in `%LOCALAPPDATA%\BiblioChad\cache\` keyed by `drive_file_id + md5Checksum`; verify the checksum after download; LRU eviction with a user-set size cap; **Pin for offline** exempts a book from eviction.

---

## 5. Library model

- **Folder = shelf.** The Library view shows the Drive tree: breadcrumb navigation, folder cards, then books. A flat "All books" view and a "Recently read" view sit beside it.
- **Metadata** is extracted locally after download (and for EPUB, ideally from the first bytes via range request): EPUB OPF (title, author, series, publisher, language, ISBN, description, cover); PDF Info/XMP (title, author, page count) with first-page render as cover fallback. Before first open, a book shows its filename and a placeholder cover.
- **Local metadata overrides** (title, author, series, tags) live in SQLite only. The app never writes back to Drive.
- **Collections / tags** are app-level and independent of the folder structure, so a book can sit in one folder and several collections.
- **Identity:** a book is identified by Drive `fileId` + `md5Checksum` (survives rename/move; changes if the file content changes). Local imports use a SHA-256 of content.

---

## 6. Reading position and sync

### What is stored per book
| Format | Locator |
|--------|---------|
| EPUB | CFI (canonical fragment identifier) + chapter href + overall percent |
| PDF  | zero-based page index + vertical offset fraction + zoom/fit mode |

Also: `updated_at` (UTC), `device_id`, `percent` (for sorting and "x% read").

### Local
- Write position on a debounce (about 2 seconds after the last page change) and on close/suspend, so a crash costs at most a couple of seconds.

### Cross-device
- A small JSON file in the Drive **`appDataFolder`** holds `{ book_key -> { locator, percent, updated_at, device_id } }` plus bookmarks and annotations. It is hidden from the user's Drive UI and never touches the book folder.
- **Conflict rule:** last-writer-wins per book by `updated_at`, with a prompt when another device is meaningfully ahead: *"Continue from page 212 on your other device?"* Annotations merge by ID; deletions are tombstoned.
- v1 sequencing: **local-only position in M1, cloud sync in M5.**

---

## 7. Feature list

Priority: **P0** = v1 must have, **P1** = v1 should have, **P2** = after v1.

### Library
- P0 Drive-folder library with breadcrumb navigation; grid and list views with covers
- P0 Continue Reading shelf; read status (Unread / Reading / Finished); progress bar on covers
- P0 Search, sort (title, author, recently read, added, progress), filter (format, status)
- P1 Collections/tags; favorites; local metadata editing
- P1 Offline pinning; cache manager
- P2 Series grouping; duplicate detection; reading stats and streaks

### Reader (both formats)
- P0 Open, navigate (keyboard, mouse wheel, touch), table of contents, go to page/percent
- P0 Remember and restore position
- P0 Light/dark themes; fullscreen
- P1 Bookmarks; in-book text search; jump-back history

### EPUB specifics
- P0 Paginated and continuous-scroll modes; font family/size, line spacing, margins, justification
- P1 Highlights and notes (colors); text selection and copy; footnote popups
- P2 Dictionary/lookup; text-to-speech via Windows speech APIs

### PDF specifics
- P0 Fit width / fit page / zoom; single-page and continuous modes
- P1 Text layer for selection and search; highlights and notes; outline/bookmarks panel
- P2 Reflow/"reading mode" for scanned or dense layouts; dual-page spread; crop margins

### Platform
- P0 Native ARM64 build; installer
- P1 Touch and pen friendly (Snapdragon X laptops and tablets); high-DPI correct
- P1 Export annotations to Markdown
- P2 Auto-update

---

## 8. Data model (SQLite sketch)

```sql
accounts(id, email, created_at)
sources(id, account_id, drive_root_id, name, changes_page_token, last_synced_at)

nodes(                      -- mirror of Drive items under the root
  id INTEGER PRIMARY KEY,
  source_id, drive_id UNIQUE, parent_drive_id,
  name, mime_type, size, md5, modified_time,
  is_folder, is_trashed, shortcut_target_id
)

books(
  id INTEGER PRIMARY KEY, node_id UNIQUE,
  format,                   -- 'epub' | 'pdf'
  title, author, series, series_index, publisher, language, isbn, description,
  page_count, cover_path, added_at, metadata_extracted_at,
  title_override, author_override
)

progress(book_id, device_id, locator_json, percent, status, updated_at, PRIMARY KEY(book_id, device_id))
bookmarks(id, book_id, locator_json, label, created_at)
annotations(id, book_id, kind, locator_json, color, note, created_at, updated_at, deleted_at)
collections(id, name) ; collection_books(collection_id, book_id)
cache_entries(drive_id, md5, path, bytes, last_access_at, pinned)
settings(key, value)
```

---

## 9. Windows ARM64 notes

- **Toolchain:** `rustup` with host/target `aarch64-pc-windows-msvc`; Visual Studio 2022 Build Tools with the **MSVC ARM64 build tools** and a Windows SDK. LLVM/clang only if a dependency demands it (a reason to prefer SChannel over C-heavy TLS crates, D8).
- **Build natively on the ARM64 machine** where possible; cross-compiling from x64 is the fallback.
- **PDFium:** bundle the `pdfium.dll` from the `win-arm64` build next to the executable (BSD-3 / Apache-2.0 licensed; include notices). Verify the DLL loads under the ARM64 process in M0.
- **WebView2:** present on Windows 11; the app is built and run as ARM64 so the webview process is native too. Confirm in M0 that the installer handles the runtime on a clean machine.
- **Native dependencies:** `rusqlite` (bundled) compiles SQLite with the C compiler; `image` and `zip` are pure Rust. Any new dependency with a C build step gets an explicit ARM64 CI check.
- **CI:** GitHub Actions `windows-11-arm` runners are free for **public** repos only (verified); for a private repo, build locally or cross-compile on `windows-latest`. Tauri's own pipeline docs do not cover Windows ARM64, so treat the installer step (NSIS) as an M0 spike item.
- **Distribution:** unsigned builds trigger SmartScreen warnings; fine for personal use. Code signing or MSIX can come later.

---

## 10. Milestones

Estimates are rough **focused-work weeks for one developer**; scale to your actual hours per week. Each milestone ends with a demoable build.

| M | Name | Est. | Exit criteria |
|---|------|------|---------------|
| **M0** | **Spike and decisions** | 1 wk | ARM64 Tauri "hello world" builds and runs. PDFium (arm64) renders a PDF page. foliate-js renders an EPUB from a Rust custom protocol. OAuth loopback flow returns a token and lists a Drive folder. Installer builds. **Go/no-go on D1-D8.** |
| **M1** | **Local reader MVP** | 2-3 wks | Open local EPUB and PDF; paginate, TOC, themes, font controls; position saved in SQLite and restored on reopen. |
| **M2** | **Drive connection** | 2 wks | Connect/disconnect Google; token in Credential Manager; folder browser; initial crawl into `nodes`; download + checksum + cache; open a Drive book end to end. |
| **M3** | **Library UI** | 2 wks | Folder-tree library with covers and metadata; grid/list; search, sort, filter; Continue Reading; read status; virtualized lists. |
| **M4** | **Reading features** | 3 wks | Bookmarks, highlights, notes; in-book search; PDF text layer and search; annotation export. |
| **M5** | **Sync and offline** | 2 wks | `changes.list` incremental sync; progress/annotation sync via `appDataFolder`; conflict prompt; offline pinning; cache management UI. |
| **M6** | **Polish and v1.0** | 2 wks | Performance targets met; accessibility pass; error handling and logging; installer; chad-voice copy and achievements; v1.0 tag. |

**Rough total: 14-16 focused weeks.**

### Performance and quality targets
- Cold start to library visible: **< 2 s** (cached library).
- Open a cached book to first page: **< 500 ms** (EPUB), **< 300 ms** (PDF first page).
- PDF page turn: **< 100 ms** after render cache warms.
- Library of **5,000 books** scrolls smoothly (virtualized grid); initial crawl of 5,000 files completes in minutes, not tens of minutes, and is resumable.
- Zero data loss on crash: reading position never more than ~2 s stale.

---

## 11. Testing and quality

- **Unit tests:** locator parsing/serialization, metadata extraction (a corpus of tricky EPUBs: missing covers, weird OPF, EPUB2 vs EPUB3, fixed-layout, RTL), sync conflict resolution, Drive path reconstruction.
- **Drive client tests:** recorded HTTP fixtures plus a mock server; explicit tests for pagination, shortcuts, rate-limit retries, and token refresh failure.
- **Integration tests:** end-to-end "connect → crawl → open → close → reopen at same position" on a throwaway test Drive folder.
- **ARM64 CI:** build, test, and package on every push (public repo) or on tags (otherwise).
- **Test corpus:** public-domain EPUBs and PDFs (Project Gutenberg, Standard Ebooks), plus a few pathological files (huge scanned PDF, 1 GB-class file, malformed ZIP).
- **Fuzzing (P2):** the EPUB/ZIP parsing path, since it handles untrusted files.

---

## 12. Security and privacy

- Drive access is **read-only**; the only write is the hidden `appDataFolder` sync file.
- Tokens stored in **Windows Credential Manager**; no secrets in the database, logs, or config.
- Treat every EPUB as untrusted: **disable scripts**, render in a sandboxed iframe with a strict CSP, block remote resource loads, resolve resources only through the custom protocol, and guard against zip-slip and decompression bombs.
- No telemetry by default. Logs are local; redact file names and IDs on request.
- Provide **Disconnect and wipe**: revoke the token, delete the cache and database.

---

## 13. Voice and personality (the fun part)

BiblioChad is narrated by a confident, slightly absurd Chad who treats reading as a competitive sport. Keep jokes in copy and empty states; **never** in error messages that block the user from fixing something.

- **App tagline ideas:** "Finish the book." / "Your Drive, but jacked." / "Reads more than you. Remembers where you stopped."
- **Empty library:** "No books yet. Connect your Drive and let's get lifting."
- **Resume prompt:** "You were on page 212. Chad remembers. Chad always remembers."
- **Progress ranks (optional):** Bookworm → Page Turner → Tome Raider → Lord Footnote → Gigachad of Letters.
- **Achievements:** "Finished a 1,000-page book", "Read the footnotes", "Seven-day streak", "Cleared the unread pile".
- **Mascot:** a jawline with glasses. Needs an original design (no existing meme art or characters).
- **Settings toggle:** *Chad Mode* on/off, so the jokes can be silenced.

---

## 14. Risks and mitigations

| ID | Risk | Likelihood | Impact | Mitigation |
|----|------|-----------|--------|-----------|
| R1 | Tauri/WebView2 packaging quirks on Windows ARM64 | Med | Med | Prove build + installer in M0; fallback to portable `.zip` distribution. |
| R2 | A dependency fails to build on ARM64 | Med | Med | M0 builds every planned crate; avoid C-heavy deps (D8); CI from day one. |
| R3 | Google token expiry (Testing mode) or restricted-scope friction | High if unaddressed | High | Publish consent screen "In production" (unverified, single user) or Workspace Internal; handle `invalid_grant` with a clean re-auth flow. |
| R4 | Huge PDFs over Drive are slow or memory-hungry | Med | Med | Range requests, PDFium streaming reader, page-bitmap cache with a memory cap. |
| R5 | EPUB rendering edge cases (fixed-layout, RTL, vertical text, broken CSS) | Med | Low-Med | Rely on foliate-js; build a regression corpus; document known limits. |
| R6 | foliate-js is not a stable, packaged library API | Med | Med | Pin a commit; wrap behind a small adapter so it can be swapped. |
| R7 | Scope creep toward "all of Calibre" | High | High | Keep non-goals (section 1) visible; defer P2 items; each new feature must displace another. |
| R8 | Sync conflicts lose annotations | Low | High | Merge by ID, tombstones, never auto-delete; keep local history for 30 days. |
| R9 | Drive API quota or rate limits during large crawls | Low-Med | Low | Minimal `fields`, batching, backoff, resumable crawl. |

---

## 15. Open questions (need your input)

1. **D1/D4:** Are you comfortable with a Rust core plus a thin TypeScript UI in a webview, or do you want an all-Rust codebase (Leptos, or a native UI with weaker EPUB rendering)?
2. **Google account:** personal Gmail or Workspace? This decides the OAuth consent-screen setup (Internal vs In production).
3. **Drive layout:** My Drive only, or also Shared Drives? One library root, or several?
4. **Repo visibility:** public (free ARM64 CI runners) or private (local builds)?
5. **Formats:** is anything beyond PDF and EPUB worth planning for now (MOBI/AZW3, CBZ comic archives, plain text)?
6. **Files:** are your books DRM-free? (DRM-protected files are out of scope.)
7. **Device:** which ARM64 machine(s) will you test on (touch, pen, screen size)?

---

## 16. Next actions (this week)

- [ ] Answer the open questions above, especially D1/D4 and the Google account type
- [ ] Create the Google Cloud project, enable the Drive API, create a **Desktop app** OAuth client, and set the consent screen publishing status
- [ ] Install the ARM64 Rust toolchain and MSVC ARM64 build tools; confirm `rustc -vV` shows `aarch64-pc-windows-msvc`
- [ ] Create the `bibliochad` repo with the workspace layout from section 3
- [ ] Run **M0**: the five spikes (Tauri ARM64 build, PDFium arm64 render, foliate-js EPUB render, OAuth loopback + folder listing, installer)
- [ ] Gather a 20-file test corpus (EPUB and PDF, including a few awkward ones)

---

## Appendix A: What is verified vs assumed

| Claim | Status |
|-------|--------|
| PDFium publishes `win-arm64` builds (`pdfium-win-arm64.tgz`) | Verified (bblanchon/pdfium-binaries README) |
| Refresh tokens expire after 7 days for External apps in Testing status; unused tokens expire after 6 months | Verified (Google OAuth docs) |
| GitHub-hosted Windows ARM64 runners are available free for public repos | Verified (GitHub announcement coverage); private repos not confirmed free |
| Tauri documents Windows ARM64 packaging | **Not found in Tauri's GitHub-pipeline docs**; test in M0 |
| `drive.file` cannot read pre-existing files not created by the app | Established Drive API behavior; confirm in M0 spike |
| foliate-js is MIT-licensed and suitable for embedding | From prior knowledge; confirm license and API stability in M0 |
| `reqwest` + `native-tls` avoids ARM64 TLS toolchain issues | Reasoned, not tested; confirm in M0 |
| Timeline estimates | Rough judgement, not measured |

## Appendix B: References
- Google OAuth 2.0 overview and token expiration: https://developers.google.com/identity/protocols/oauth2
- PDFium prebuilt binaries: https://github.com/bblanchon/pdfium-binaries
- GitHub Actions Windows on Arm runners for public repos: https://elevenforum.com/t/github-actions-now-supports-windows-on-arm-runners-for-all-public-repos.35206
- Tauri v2 GitHub pipelines: https://v2.tauri.app/distribute/pipelines/github
- foliate-js: https://github.com/johnfactotum/foliate-js
