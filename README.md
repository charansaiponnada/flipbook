# 📖 Flipbook

Open any PDF as a real page-turning book. A small native Windows app written in **Rust** (Tauri), so there is no browser tab, no upload, and the whole thing ships as one `.exe`.

- **Open a PDF** or **choose a folder**. Every PDF inside it, including sub-folders, is listed as a book on a shelf.
- Drag a PDF (or a folder) onto the window.
- Page-curl animation, hard covers, two-page spread, page slider, full screen.
- Keyboard: ← → (or Page Up/Down) to turn, Home / End, F11 for full screen, Esc to go back.
- Real cover thumbnails on the shelf, a "Continue reading" row, and search for big folders.
- Remembers your page in every book, shows your progress on the shelf, and re-opens your last folder.
- Controls fade away while you read; click the page counter to jump to a page.
- Right-click a PDF → **Open with → Flipbook** works after installing.
- Everything stays on your computer.

## Get the .exe

1. Push this repo to GitHub.
2. Tag a release: `git tag v0.1.0 && git push --tags`
3. GitHub Actions builds it and attaches `Flipbook-portable.exe` and the installer to the release.

You can also run the **Build Windows exe** workflow by hand from the Actions tab and download the artifact.

Requires Windows 10/11 (WebView2 is already installed there).

## Build locally

Install [Rust](https://rustup.rs), Node.js 22+, then:

```bash
npm install
npm run tauri -- icon app-icon.png   # generates src-tauri/icons
npm run tauri -- dev                 # run
npm run tauri -- build               # exe in src-tauri/target/release/
```

## How it is put together

| Part | Job |
| --- | --- |
| `src-tauri/src/main.rs` | Rust: walks folders for PDFs, reads PDF bytes, handles "open with" |
| `ui/index.html` | The shelf and the book |
| `ui/vendor/` | pdf.js (Apache-2.0) renders pages, StPageFlip (MIT) turns them |

MIT licensed.
