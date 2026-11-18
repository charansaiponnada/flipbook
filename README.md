# 📖 Flipbook

Open any PDF as a real page-turning book. A native app written entirely in **Rust** with [**GPUI**](https://www.gpui.rs), the GPU-accelerated UI framework behind the Zed editor. No webview, no JavaScript, no upload: one `.exe`, and everything stays on your computer.

## What it does

- **Open a PDF** or **choose a folder**. Every PDF inside it, including sub-folders, becomes a book on a shelf.
- Drag a PDF (or a folder) onto the window, or right-click a PDF → **Open with** → **Flipbook**.
- Two-page spreads with a hard cover, page-turn animation, page slider, full screen.
- Real cover thumbnails, a "Continue reading" row, reading progress on every book. Just start typing on the shelf to search.
- Remembers your page in every book and re-opens your last folder.

New in the Rust rewrite:

- **Instant open, flat memory.** Pages are rasterised on background threads only around where you are reading, and released as you move on. A 2,000-page scan opens as fast as a pamphlet.
- **Table of contents** from the PDF's outline, and **bookmarks** (`B`), in one side panel (`T`).
- **Paper**: Day, Sepia or Night (`P`).
- **Auto-turn** for hands-free reading: every 10, 20 or 40 seconds (`A`).
- **One or two pages** (`D`); narrow windows switch to one page by themselves.
- Covers are cached on disk, so the shelf is instant the second time.
- Type a page number and press Enter to jump there.

## Keys

| Key | Does |
| --- | --- |
| `←` `→`, `Page Up` `Page Down`, `Space`, mouse wheel, click a side | Turn the page |
| `Home` / `End` | First / last page |
| digits, then `Enter` | Go to page |
| `B` | Bookmark this page |
| `T` | Contents and bookmarks |
| `P` | Paper: Day → Sepia → Night |
| `D` | One page / two pages |
| `A` | Auto-turn: 10 s → 20 s → 40 s → off |
| `F` or `F11` | Full screen |
| `Esc` | Leave full screen, close the panel, back to the shelf |

## Install

Download **`Flipbook-setup.exe`** from the [latest release](https://github.com/charansaiponnada/flipbook/releases/latest) and run it.

- Installs just for you: no administrator prompt.
- Adds Flipbook to the Start menu (and, if you tick it, the desktop).
- Adds Flipbook to **Open with** for PDFs, without taking over your default PDF app.
- Uninstall from **Settings → Apps**. Your reading progress and bookmarks are kept.

Prefer no installer? `Flipbook-portable.exe` is the same app as a single file.

Windows may show a SmartScreen warning the first time, because the exe isn't code-signed: click **More info → Run anyway**.

To publish a release, bump `version` in `Cargo.toml`, then tag it; GitHub Actions builds both files and attaches them:

```bash
git tag v0.3.0 && git push --tags
```

## Build locally

Install [Rust](https://rustup.rs) (on Windows, with the MSVC build tools), then:

```bash
cargo run --release
```

```bash
cargo test
```

To build the installer too, install [NSIS](https://nsis.sourceforge.io), then:

```bash
makensis -DVERSION=0.3.0 assets/windows/installer.nsi
```

It is written to `target/release/Flipbook-0.3.0-setup.exe`.

## How it is put together

| Part | Job |
| --- | --- |
| `src/main.rs` | The GPUI app: shelf, reader, page-turn animation, keys |
| `src/pdf.rs` | Folder scan, page rasterising, covers, table of contents |
| `src/store.rs` | Reading progress, bookmarks and settings (`%APPDATA%\Flipbook\state.json`) |
| `build.rs` | Embeds the icon and version info in the exe |
| `assets/windows/` | Icon, its generator (`icon.py`), and the NSIS installer script |

PDFs are rendered by [hayro](https://github.com/LaurenzV/hayro), a pure-Rust rasteriser, so there is no PDFium DLL to ship. It does not open password-protected PDFs.

MIT licensed.
