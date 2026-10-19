# Flipbook — Launch Tweets for X

Repo: https://github.com/charansaiponnada/flipbook
Images: `assets/social/` (01-hero, 02-features, 03-square, 04-shelf, 05-reader)

---

## Recommended thread

**Tweet 1 — hook** · attach `01-hero.png`
> I got tired of reading PDFs in a browser tab.
>
> So I built Flipbook — a tiny native Windows app that opens any PDF as a REAL page-turning book.
>
> No browser tab. No upload. No cloud. One .exe.
>
> Open source, MIT 🧵
>
> github.com/charansaiponnada/flipbook

**Tweet 2 — features** · attach `02-features.png`
> What it does:
>
> 📄 Open a PDF — or point it at a folder and every PDF inside (sub-folders too) lands on a shelf
> 📚 A real bookshelf — filenames become book spines
> 📑 Page-curl animation, hard covers, two-page spread
> ⌨️ Arrow keys, page slider, F11
> 🔖 Remembers your page in every book
> 🔒 Nothing leaves your PC

**Tweet 3 — the app** · attach `04-shelf.png` + `05-reader.png`
> The shelf and the book.
>
> Drop a PDF (or a folder) straight onto the window. Click a spine. Turn pages.
>
> Switched to dark mode because of course I did.

**Tweet 4 — the build** · attach `03-square.png`
> Under the hood: Rust + Tauri 2.
>
> → pages rendered by pdf.js
> → flips by StPageFlip
> → ~0 telemetry, 0 network calls, 0 accounts
>
> Release build is opt-level="s" + LTO + strip. It's small and it starts instantly.

**Tweet 5 — CTA**
> It's out now. MIT licensed, all of it.
>
> ⭐ Star it: github.com/charansaiponnada/flipbook
> 📦 Download the .exe: .../releases
>
> Be honest — would you use this over a browser tab? 👇

---

## Standalone tweets (alternates / day-2 content)

**The "against the grain" one** · attach `03-square.png`
> Hot take: the browser is a bad PDF reader.
>
> Tabs, chrome, RAM, ads, "download to view", uploads.
>
> Books deserve to feel like books. So I made a 1-file Windows app that opens PDFs with a real page curl.
>
> github.com/charansaiponnada/flipbook

**The "I hate upload buttons" one** · attach `01-hero.png`
> "Upload your PDF to read it."
>
> Why. It's already on my computer.
>
> Flipbook never uploads anything. It walks your folders, finds the PDFs, and reads them locally.
>
> Rust + Tauri, MIT, open source ↓
> github.com/charansaiponnada/flipbook

**The build-log one** · attach `05-reader.png`
> Spent the weekend making PDFs turn pages properly.
>
> Two-page spread → page-curl → hard covers → your saved page. All in a Rust + Tauri app that ships as one .exe.
>
> Called it Flipbook. It's open source now.
>
> github.com/charansaiponnada/flipbook

**The "quiet software" one** · attach `02-features.png`
> Quiet software #1: Flipbook.
>
> No account. No onboarding. No telemetry. No "Sign in with Google."
>
> Open it → your books are on a shelf → click → read.
>
> MIT, Windows, Rust + Tauri.
> github.com/charansaiponnada/flipbook

**The short one** · attach `01-hero.png`
> Built a Windows app that turns any PDF into a page-turning book.
>
> One .exe. Fully offline. Open source.
>
> github.com/charansaiponnada/flipbook

---

## Notes for posting

- **Post the thread Tue–Thu, ~9–11am ET** (best reach for dev tools).
- Lead image matters most: X crops link cards to ~1.91:1, so `01-hero.png` and `02-features.png` (1600×900) are ideal. Use `03-square.png` when you have no link card and want max vertical space.
- **Don't put the link in tweet 1** — X suppresses reach on link-first posts. Put it in the CTA tweet, or in a reply.
- Reply to tweet 1 with the link immediately after posting so the thread stays reachable.
- Pin the thread. Add the repo link to your bio for the week.
- Alt text suggestions:
  - 01-hero: "Flipbook — dark title card reading 'Open any PDF as a real page-turning book', with an illustrated book mid-page-curl."
  - 02-features: "Six feature tiles: open a PDF or folder, a real bookshelf, page-curl animation, keyboard and slider controls, remembers your page, nothing leaves your PC."
  - 03-square: "Dark square card reading 'No upload. No browser tab. Just pages.'"
  - 04-shelf: "The Flipbook library screen — PDF filenames shown as book spines grouped into folders."
  - 05-reader: "The Flipbook reader — a two-page spread with a page mid-curl, plus a page slider at 97 of 184."
- Hashtags to sprinkle (1–2 max, don't spam): `#RustLang` `#Tauri` `#OpenSource`
- Tag `@TauriApps` in the build tweet — they frequently repost community apps.
