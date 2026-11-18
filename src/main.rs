// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod icons;
mod pdf;
mod store;
mod update;

use gpui::{
    div, img, linear_color_stop, svg, linear_gradient, prelude::*, px, rgb, size, App, Application, Bounds, Context, Div,
    ExternalPaths, FocusHandle, FontWeight, Hsla, KeyDownEvent, MouseButton, ObjectFit, PathPromptOptions, Pixels,
    Point, RenderImage, Rgba, ScrollWheelEvent, SharedString, Stateful, Svg, Task, TitlebarOptions, Window,
    WindowAppearance, WindowBounds, WindowOptions,
};
use hayro::hayro_syntax::Pdf;
use pdf::{Entry, TocItem};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use store::Store;

/// Page-turn styles, picked with S or the toolbar.
const TURNS: [&str; 5] = ["Curl", "Fold", "Slide", "Fade", "None"];
/// Seconds each style takes to turn a page.
const TURN_SECS: [f32; 5] = [0.7, 0.55, 0.38, 0.32, 0.];
const BAR: f32 = 52.0; // top bar height
const FOOT: f32 = 60.0; // bottom bar height
const SERIF: &str = "Georgia";
const PAPERS: [&str; 3] = ["Day", "Sepia", "Night"];

struct Theme {
    bg: Rgba,
    surface: Rgba,
    ink: Rgba,
    mute: Rgba,
    line: Rgba,
    accent: Rgba,
    on_accent: Rgba,
    stage: Rgba,
}

fn theme(dark: bool) -> Theme {
    let c = |v: [u32; 8]| Theme {
        bg: rgb(v[0]),
        surface: rgb(v[1]),
        ink: rgb(v[2]),
        mute: rgb(v[3]),
        line: rgb(v[4]),
        accent: rgb(v[5]),
        on_accent: rgb(v[6]),
        stage: rgb(v[7]),
    };
    if dark {
        c([0x16130f, 0x221d17, 0xece3d3, 0xa39583, 0x342c23, 0xe0996a, 0x1b140d, 0x0f0d0a])
    } else {
        c([0xf3eee5, 0xfbf8f2, 0x231d16, 0x6f6354, 0xe0d6c6, 0x9a4f2c, 0xffffff, 0xddd5c6])
    }
}

fn shade(alpha: f32) -> Hsla {
    gpui::black().opacity(alpha)
}

fn fmt_size(b: u64) -> String {
    if b < 1 << 20 {
        format!("{} KB", (b / 1024).max(1))
    } else {
        format!("{:.1} MB", b as f64 / (1 << 20) as f64)
    }
}

// ---------- spreads: the cover sits alone on the right, then pages pair up (1,2) (3,4) ... ----------

fn spread_of(page: usize) -> usize {
    (page + 1) / 2
}
fn first_of(spread: usize) -> usize {
    (spread * 2).saturating_sub(1)
}
fn left_of(spread: usize) -> Option<usize> {
    (spread > 0).then(|| spread * 2 - 1)
}
fn right_of(spread: usize, n: usize) -> Option<usize> {
    (spread * 2 < n).then_some(spread * 2)
}
fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

struct Flip {
    from: usize,
    to: usize,
    start: Instant,
    /// Captured when the turn starts, so switching style mid-turn can't break it.
    style: usize,
    secs: f32,
}

struct Reader {
    path: PathBuf,
    title: SharedString,
    pdf: Arc<Pdf>,
    n: usize,
    ratio: f32,
    /// 0-based. In two-page mode always the first page of the visible spread.
    page: usize,
    two_up: bool,
    /// Only pages near the current one are kept rasterised, so a 2,000-page scan opens as fast as a pamphlet.
    images: HashMap<usize, Arc<RenderImage>>,
    pending: HashSet<usize>,
    /// Bumped whenever `images` is invalidated; late renders from an older generation are discarded.
    generation: usize,
    render_h: f32,
    flip: Option<Flip>,
    toc: Vec<TocItem>,
    panel: bool,
    jump: Option<String>,
    idle: bool,
    woke: Instant,
    idle_task: Option<Task<()>>,
    auto_secs: u64,
    auto_task: Option<Task<()>>,
}

struct Flipbook {
    focus: FocusHandle,
    store: Store,
    books: Option<Vec<Entry>>,
    scanning: bool,
    /// Books whose cover thumbnail is on disk.
    covers: HashMap<PathBuf, PathBuf>,
    cover_task: Option<Task<()>>,
    query: String,
    reader: Option<Reader>,
    toast: Option<SharedString>,
    toast_task: Option<Task<()>>,
    /// Bumped on every open/close so a slow load can tell it has been superseded.
    token: usize,
    /// A newer release on GitHub, until the user updates or says later.
    release: Option<update::Release>,
    updating: bool,
    /// Installed copies update themselves; portable ones just open the release page.
    installed: bool,
}

impl Flipbook {
    fn new(cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            focus: cx.focus_handle(),
            store: Store::load(),
            books: None,
            scanning: false,
            covers: HashMap::new(),
            cover_task: None,
            query: String::new(),
            reader: None,
            toast: None,
            toast_task: None,
            token: 0,
            release: None,
            updating: false,
            installed: update::installed(),
        };
        this.check_for_update(cx);
        // startup: a PDF passed on the command line ("Open with"), and the last folder behind it
        if let Some(root) = this.store.root.clone() {
            this.scan(root, cx);
        } else {
            this.make_covers(cx);
        }
        if let Some(file) = std::env::args_os().skip(1).map(PathBuf::from).find(|a| pdf::is_pdf(a)) {
            this.open(file, cx);
        }
        this
    }

    fn dark(&self, window: &Window) -> bool {
        self.store.dark.unwrap_or(matches!(window.appearance(), WindowAppearance::Dark | WindowAppearance::VibrantDark))
    }

    fn say(&mut self, msg: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.toast = Some(msg.into());
        self.toast_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(4)).await;
            this.update(cx, |this, cx| {
                this.toast = None;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    // ---------- updates ----------

    /// Ask GitHub whether there's a newer Flipbook. After a check that finds nothing, wait a day.
    fn check_for_update(&mut self, cx: &mut Context<Self>) {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        if self.store.no_update_check || now.saturating_sub(self.store.last_update_check) < 24 * 60 * 60 {
            return;
        }
        let found = cx.background_executor().spawn(async { update::check() });
        cx.spawn(async move |this, cx| {
            let found = found.await;
            this.update(cx, |this, cx| {
                if found.is_none() {
                    this.store.last_update_check = now;
                    this.store.save();
                }
                this.release = found;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn apply_update(&mut self, cx: &mut Context<Self>) {
        let Some(rel) = self.release.clone() else { return };
        let Some((url, digest)) = rel.installer.filter(|_| self.installed) else {
            return cx.open_url(&rel.page);
        };
        if self.updating {
            return;
        }
        self.updating = true;
        cx.notify();
        let version = rel.version;
        let job = cx.background_executor().spawn(async move {
            update::download(&url, digest.as_deref(), &version).and_then(|installer| update::install(&installer))
        });
        cx.spawn(async move |this, cx| {
            let result = job.await;
            this.update(cx, |this, cx| match result {
                // the installer waits for us to close, then reopens the new version
                Ok(()) => cx.quit(),
                Err(e) => {
                    this.updating = false;
                    this.say(e, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    // ---------- library ----------

    fn pick(&mut self, folder: bool, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: !folder, directories: folder, multiple: false, prompt: None });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                if let Some(p) = paths.into_iter().next() {
                    this.update(cx, |this, cx| if folder { this.scan(p, cx) } else { this.open(p, cx) }).ok();
                }
            }
        })
        .detach();
    }

    fn scan(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        self.scanning = true;
        cx.notify();
        let dir = root.clone();
        let found = cx.background_executor().spawn(async move { pdf::scan(&dir) });
        cx.spawn(async move |this, cx| {
            let found = found.await;
            this.update(cx, |this, cx| {
                this.scanning = false;
                match found {
                    Some(list) => {
                        this.store.root = Some(root);
                        this.store.save();
                        this.books = Some(list);
                        this.query.clear();
                        this.make_covers(cx);
                    }
                    None => this.say("Could not read that folder.", cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Covers are page 1, rendered once in the background and kept as PNGs next to the state file.
    // ponytail: one cover at a time; fan out across cores if first-time scans of huge libraries feel slow
    fn make_covers(&mut self, cx: &mut Context<Self>) {
        let books: Vec<PathBuf> =
            self.store.recent.iter().cloned().chain(self.books.iter().flatten().map(|b| b.path.clone())).collect();
        self.cover_task = Some(cx.spawn(async move |this, cx| {
            for book in books {
                let out = pdf::cover_file(&book);
                let (src, dst) = (book.clone(), out.clone());
                let ok = cx.background_executor().spawn(async move { dst.exists() || pdf::make_cover(&src, &dst) }).await;
                if ok && this.update(cx, |this, cx| {
                        this.covers.insert(book, out);
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    // ---------- reader ----------

    fn open(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.token += 1;
        let token = self.token;
        let file = path.clone();
        let loaded = cx.background_executor().spawn(async move { pdf::load(&file) });
        cx.spawn(async move |this, cx| {
            let loaded = loaded.await;
            this.update(cx, |this, cx| {
                if this.token != token {
                    return; // the user opened something else meanwhile
                }
                let (pdf, toc) = match loaded {
                    Ok(ok) => ok,
                    Err(e) => return this.say(e, cx),
                };
                this.close(None, cx);
                let n = pdf.pages().len();
                this.store.recent.retain(|p| p != &path);
                this.store.recent.insert(0, path.clone());
                this.store.recent.truncate(12);
                let book = this.store.book_mut(&path);
                book.pages = n;
                let page = book.page.min(n - 1);
                this.store.save();
                this.token += 1;
                this.reader = Some(Reader {
                    title: pdf::title(&path).into(),
                    ratio: pdf::ratio(&pdf, 0),
                    path,
                    pdf,
                    n,
                    page,
                    two_up: false,
                    images: HashMap::new(),
                    pending: HashSet::new(),
                    generation: this.token,
                    render_h: 0.0,
                    flip: None,
                    toc,
                    panel: false,
                    jump: None,
                    idle: false,
                    woke: Instant::now(),
                    idle_task: None,
                    auto_secs: 0,
                    auto_task: None,
                });
                this.wake(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Back to the shelf. `window` is the window being updated, if any (GPUI needs it to free textures).
    fn close(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        self.token += 1;
        if let Some(mut r) = self.reader.take() {
            free(r.images.drain().map(|(_, i)| i), window, cx);
            self.make_covers(cx); // the book just read may be new to the shelf
        }
        cx.notify();
    }

    /// Rasterise the pages around the current one on background threads.
    fn ensure(&mut self, cx: &mut Context<Self>) {
        let paper = self.store.paper;
        let Some(r) = &mut self.reader else { return };
        if r.render_h < 1.0 {
            return;
        }
        // nearest first, looking further ahead than behind
        for d in [0isize, 1, 2, -1, 3, -2, 4, 5] {
            let Some(i) = r.page.checked_add_signed(d).filter(|&i| i < r.n) else { continue };
            if r.images.contains_key(&i) || !r.pending.insert(i) {
                continue;
            }
            let (pdf, generation, h) = (r.pdf.clone(), r.generation, r.render_h);
            let image = cx.background_executor().spawn(async move { pdf::page_image(&pdf, i, h, paper) });
            cx.spawn(async move |this, cx| {
                let image = image.await;
                this.update(cx, |this, cx| {
                    let Some(r) = this.reader.as_mut().filter(|r| r.generation == generation) else { return };
                    // a page that failed to render stays in `pending`, so it is left blank instead of retried forever
                    if let Some(image) = image {
                        r.pending.remove(&i);
                        r.images.insert(i, image);
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        }
    }

    /// Throw away every rasterised page (paper or size changed) and start over.
    fn rerender(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.token += 1;
        if let Some(r) = &mut self.reader {
            r.generation = self.token;
            r.pending.clear();
            free(r.images.drain().map(|(_, i)| i), Some(window), cx);
        }
        self.ensure(cx);
        cx.notify();
    }

    fn goto(&mut self, page: usize, animate: bool, cx: &mut Context<Self>) {
        let Some(r) = &mut self.reader else { return };
        let page = page.min(r.n - 1);
        let page = if r.two_up { first_of(spread_of(page)) } else { page };
        if page == r.page {
            return;
        }
        let style = (self.store.turn as usize).min(TURNS.len() - 1);
        let secs = TURN_SECS[style];
        r.flip = (animate && secs > 0.).then(|| Flip { from: r.page, to: page, start: Instant::now(), style, secs });
        r.page = page;
        let path = r.path.clone();
        self.store.book_mut(&path).page = page;
        self.store.save();
        self.ensure(cx);
        cx.notify();
    }

    fn step(&mut self, dir: isize, cx: &mut Context<Self>) {
        let Some(r) = &self.reader else { return };
        let to = if r.two_up {
            first_of(spread_of(r.page).saturating_add_signed(dir).min(r.n / 2))
        } else {
            r.page.saturating_add_signed(dir)
        };
        self.goto(to, true, cx);
    }

    /// Controls fade out while reading and come back on any mouse move or key.
    fn wake(&mut self, cx: &mut Context<Self>) {
        let Some(r) = &mut self.reader else { return };
        if !r.idle && r.woke.elapsed() < Duration::from_millis(250) && r.idle_task.is_some() {
            return; // mouse moves arrive in floods
        }
        if r.idle {
            cx.notify();
        }
        r.idle = false;
        r.woke = Instant::now();
        r.idle_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(2500)).await;
            this.update(cx, |this, cx| {
                if let Some(r) = &mut this.reader {
                    r.idle = !r.panel && r.jump.is_none();
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    fn toggle_bookmark(&mut self, cx: &mut Context<Self>) {
        let Some(r) = &self.reader else { return };
        let (path, page) = (r.path.clone(), r.page);
        let marks = &mut self.store.book_mut(&path).bookmarks;
        let msg = match marks.binary_search(&page) {
            Ok(i) => {
                marks.remove(i);
                "Bookmark removed"
            }
            Err(i) => {
                marks.insert(i, page);
                "Page bookmarked"
            }
        };
        self.store.save();
        self.say(msg, cx);
    }

    /// Hands-free reading: turn the page every 10 / 20 / 40 seconds.
    fn cycle_auto(&mut self, cx: &mut Context<Self>) {
        let Some(r) = &mut self.reader else { return };
        let secs = match r.auto_secs {
            0 => 10,
            10 => 20,
            20 => 40,
            _ => 0,
        };
        r.auto_secs = secs;
        r.auto_task = (secs > 0).then(|| {
            cx.spawn(async move |this, cx| loop {
                cx.background_executor().timer(Duration::from_secs(secs)).await;
                if this.update(cx, |this, cx| this.step(1, cx)).is_err() {
                    break;
                }
            })
        });
        self.say(if secs > 0 { format!("Turning the page every {secs} seconds") } else { "Auto-turn off".into() }, cx);
    }

    fn cycle_paper(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.store.paper = (self.store.paper + 1) % 3;
        self.store.save();
        self.say(format!("Paper: {}", PAPERS[self.store.paper as usize]), cx);
        self.rerender(window, cx);
    }

    fn cycle_turn(&mut self, cx: &mut Context<Self>) {
        self.store.turn = (self.store.turn + 1) % TURNS.len() as u8;
        self.store.save();
        self.say(format!("Page turn: {}", TURNS[self.store.turn as usize]), cx);
    }

    fn toggle_single(&mut self, cx: &mut Context<Self>) {
        self.store.single = !self.store.single;
        self.store.save();
        cx.notify();
    }

    fn toggle_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.store.dark = Some(!self.dark(window));
        self.store.save();
        cx.notify();
    }

    fn toggle_panel(&mut self, cx: &mut Context<Self>) {
        if let Some(r) = &mut self.reader {
            r.panel = !r.panel;
            cx.notify();
        }
    }

    fn key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let (key, mods) = (e.keystroke.key.as_str(), e.keystroke.modifiers);
        if key == "f11" {
            return window.toggle_fullscreen();
        }
        if mods.control || mods.alt || mods.platform {
            return;
        }
        self.wake(cx);
        let Some(r) = &mut self.reader else {
            // on the shelf, typing anywhere searches
            match key {
                "escape" if window.is_fullscreen() && self.query.is_empty() => window.toggle_fullscreen(),
                "escape" => self.query.clear(),
                "backspace" => drop(self.query.pop()),
                _ => match &e.keystroke.key_char {
                    Some(c) if !c.chars().any(char::is_control) => self.query.push_str(c),
                    _ => return,
                },
            }
            return cx.notify();
        };
        // in a book, typing digits jumps to a page
        if key.len() == 1 && key.as_bytes()[0].is_ascii_digit() {
            let jump = r.jump.get_or_insert_default();
            if jump.len() < 6 {
                jump.push_str(key);
            }
            return cx.notify();
        }
        if let Some(jump) = r.jump.take() {
            match key {
                "enter" => {
                    if let Ok(p) = jump.parse::<usize>() {
                        self.goto(p.saturating_sub(1), true, cx);
                    }
                }
                "backspace" => r.jump = Some(jump).map(|mut j| (j.pop(), j).1).filter(|j| !j.is_empty()),
                _ => {}
            }
            return cx.notify();
        }
        match key {
            "right" | "down" | "pagedown" | "space" => self.step(1, cx),
            "left" | "up" | "pageup" => self.step(-1, cx),
            "home" => self.goto(0, true, cx),
            "end" => self.goto(usize::MAX, true, cx),
            "escape" if window.is_fullscreen() => window.toggle_fullscreen(),
            "escape" if r.panel => self.toggle_panel(cx),
            "escape" => self.close(Some(window), cx),
            "f" => window.toggle_fullscreen(),
            "b" => self.toggle_bookmark(cx),
            "t" => self.toggle_panel(cx),
            "d" => self.toggle_single(cx),
            "p" => self.cycle_paper(window, cx),
            "a" => self.cycle_auto(cx),
            "s" => self.cycle_turn(cx),
            _ => {}
        }
    }

    fn dropped(&mut self, paths: &ExternalPaths, window: &mut Window, cx: &mut Context<Self>) {
        let paths = paths.paths();
        if let Some(file) = paths.iter().find(|p| pdf::is_pdf(p)) {
            self.open(file.clone(), cx);
        } else if let Some(dir) = paths.iter().find(|p| p.is_dir()) {
            self.close(Some(window), cx);
            self.scan(dir.clone(), cx);
        } else {
            self.say("Flipbook opens PDFs and folders of PDFs.", cx);
        }
    }
}

/// Release GPU textures. Dropping the `Arc` alone would leak them in the sprite atlas.
fn free(images: impl Iterator<Item = Arc<RenderImage>>, mut window: Option<&mut Window>, cx: &mut App) {
    for image in images {
        cx.drop_image(image, window.as_deref_mut());
    }
}

// ---------- views ----------

fn btn_base(id: &'static str, th: &Theme) -> Stateful<Div> {
    let hover = th.line;
    div()
        .id(id)
        .h(px(34.))
        .px_3()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded_lg()
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .gap_2()
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
}

fn icon(name: &str, color: impl Into<Hsla>) -> Svg {
    svg().path(format!("icons/{name}.svg")).size(px(18.)).flex_none().text_color(color)
}

/// A square button showing one icon, with a tooltip naming what it does and its key.
fn ibtn(id: &'static str, name: &str, tip: impl Into<SharedString>, color: impl Into<Hsla>, th: &Theme) -> Stateful<Div> {
    let tip: SharedString = tip.into();
    btn_base(id, th).min_w(px(34.)).px(px(8.)).child(icon(name, color)).tooltip(move |_, cx| cx.new(|_| Tip(tip.clone())).into())
}

struct Tip(SharedString);

impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().px_2().py_1().rounded_md().bg(rgb(0x2a241e)).text_color(rgb(0xf3eee5)).text_size(px(12.)).shadow_md().child(self.0.clone())
    }
}

/// A vertical band of shadow, `a` dark at its left edge fading to `b` at its right.
fn band(x: f32, w: f32, h: f32, a: f32, b: f32) -> Div {
    div().absolute().top_0().left(px(x)).w(px(w.max(0.))).h(px(h)).bg(linear_gradient(
        90.,
        linear_color_stop(shade(a), 0.),
        linear_color_stop(shade(b), 1.),
    ))
}

impl Flipbook {
    fn tile(&self, id: usize, path: &Path, name: &str, size: Option<u64>, th: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let book = self.store.book(path);
        let (page, n) = (book.page + 1, book.pages);
        let done = n > 0 && page + 1 >= n;
        let meta = if done {
            "Finished".to_string()
        } else if page > 1 {
            format!("p. {page} of {n}")
        } else {
            size.map(fmt_size).unwrap_or_default()
        };
        let open = path.to_path_buf();
        let cover = div()
            .relative()
            .w(px(132.))
            .h(px(198.))
            .rounded(px(4.))
            .overflow_hidden()
            .bg(th.surface)
            .shadow_md()
            .child(match self.covers.get(path) {
                Some(file) => img(file.clone()).size_full().object_fit(ObjectFit::Cover).into_any_element(),
                None => div()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(div().w(px(24.)).h(px(2.)).bg(th.accent))
                    .child(div().font_family(SERIF).font_weight(FontWeight::SEMIBOLD).line_clamp(7).child(name.to_string()))
                    .into_any_element(),
            })
            // the spine
            .child(div().absolute().left_0().top_0().h_full().w(px(7.)).bg(linear_gradient(
                90.,
                linear_color_stop(shade(0.22), 0.),
                linear_color_stop(shade(0.), 1.),
            )))
            .when(page > 1 && n > 0, |d| {
                d.child(
                    div().absolute().left_0().bottom_0().w_full().h(px(3.)).bg(shade(0.28)).child(
                        div().h_full().w(gpui::relative((page as f32 / n as f32).min(1.))).bg(th.accent),
                    ),
                )
            });
        div()
            .id(("book", id))
            .w(px(132.))
            .flex()
            .flex_col()
            .gap_2()
            .cursor_pointer()
            .hover(|s| s.opacity(0.85))
            .on_click(cx.listener(move |this, _, _, cx| this.open(open.clone(), cx)))
            .child(cover)
            .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).line_height(px(17.)).line_clamp(2).child(name.to_string()))
            .child(div().text_size(px(12.)).text_color(th.mute).child(meta))
    }

    fn shelf(&self, head: String, tiles: Vec<Stateful<Div>>, th: &Theme) -> Div {
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(div().text_size(px(13.)).text_color(th.mute).child(head))
            .child(div().flex().flex_wrap().gap_x(px(22.)).gap_y(px(30.)).children(tiles))
    }

    // ponytail: every tile is laid out every frame; switch to gpui::uniform_list if shelves reach thousands of books
    fn library(&self, th: &Theme, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let q = self.query.trim().to_lowercase();
        let where_ = match (&self.books, &self.store.root) {
            (Some(b), Some(root)) => format!("{} · {} {}", root.display(), b.len(), if b.len() == 1 { "book" } else { "books" }),
            _ if self.scanning => "Looking for PDFs…".into(),
            _ => "Your PDFs as page-turning books. They never leave this computer.".into(),
        };
        let mut id = 0;
        let mut next_id = || {
            id += 1;
            id
        };
        let mut shelves = Vec::new();

        let going: Vec<_> = self
            .store
            .recent
            .iter()
            .filter(|p| {
                let b = self.store.book(p);
                b.page > 0 && b.page + 2 < b.pages
            })
            .take(6)
            .map(|p| self.tile(next_id(), p, &pdf::title(p), None, th, cx))
            .collect();
        let reading = !going.is_empty();
        if reading && q.is_empty() {
            shelves.push(self.shelf("Continue reading".into(), going, th));
        }

        let mut shown = 0;
        let books = self.books.as_deref().unwrap_or_default();
        for group in books.chunk_by(|a, b| a.folder == b.folder) {
            let tiles: Vec<_> = group
                .iter()
                .filter(|b| q.is_empty() || b.name.to_lowercase().contains(&q))
                .map(|b| self.tile(next_id(), &b.path, &b.name, Some(b.size), th, cx))
                .collect();
            if !tiles.is_empty() {
                shown += tiles.len();
                shelves.push(self.shelf(format!("{} · {}", group[0].folder, tiles.len()), tiles, th));
            }
        }

        let note = if self.books.is_none() && !reading && !self.scanning {
            Some(
                div()
                    .mt(px(90.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_2()
                    .text_color(th.mute)
                    .child(icon("two-pages", th.accent).size(px(44.)))
                    .child(div().mt_2().font_family(SERIF).text_size(px(21.)).text_color(th.ink).child("Your shelf is empty"))
                    .child("Open a PDF, or choose a folder and every PDF inside it becomes a book.")
                    .child("You can also drop files onto this window."),
            )
        } else if self.books.is_some() && shown == 0 {
            Some(div().text_color(th.mute).child(if q.is_empty() {
                "No PDFs found in that folder.".to_string()
            } else {
                format!("No books match “{}”.", self.query.trim())
            }))
        } else {
            None
        };

        let search = div()
            .h(px(34.))
            .w(px(200.))
            .px_3()
            .flex()
            .items_center()
            .rounded_lg()
            .border_1()
            .border_color(if self.query.is_empty() { th.line } else { th.accent })
            .bg(th.surface)
            .text_size(px(13.))
            .gap_2()
            .overflow_hidden()
            .child(icon("search", th.mute).size(px(16.)))
            .child(if self.query.is_empty() {
                div().text_color(th.mute).child("Type to search")
            } else {
                div().child(format!("{}▏", self.query))
            });

        div().id("library").size_full().overflow_y_scroll().child(
            div()
                .max_w(px(1120.))
                .mx_auto()
                .px(px(32.))
                .pt(px(32.))
                .pb(px(80.))
                .flex()
                .flex_col()
                .gap(px(40.))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_end()
                        .justify_between()
                        .gap_4()
                        .pb_5()
                        .border_b_1()
                        .border_color(th.line)
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .min_w_0()
                                .child(div().font_family(SERIF).font_weight(FontWeight::SEMIBOLD).text_size(px(28.)).child("Flipbook"))
                                .child(div().text_size(px(13.)).text_color(th.mute).truncate().child(where_)),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .when(self.books.as_ref().is_some_and(|b| b.len() > 6), |d| d.child(search))
                                .child(
                                    btn_base("pick-dir", th)
                                        .child(icon("folder", th.ink).size(px(16.)))
                                        .child("Choose folder")
                                        .border_1()
                                        .border_color(th.line)
                                        .bg(th.surface)
                                        .on_click(cx.listener(|this, _, _, cx| this.pick(true, cx))),
                                )
                                .child(
                                    btn_base("pick-file", th)
                                        .child(icon("file-plus", th.on_accent).size(px(16.)))
                                        .child("Open PDF")
                                        .bg(th.accent)
                                        .text_color(th.on_accent)
                                        .on_click(cx.listener(|this, _, _, cx| this.pick(false, cx))),
                                )
                                .child(
                                    ibtn(
                                        "theme",
                                        if self.dark(window) { "sun" } else { "moon" },
                                        if self.dark(window) { "Light theme" } else { "Dark theme" },
                                        th.mute,
                                        th,
                                    )
                                    .on_click(cx.listener(|this, _, window, cx| this.toggle_theme(window, cx))),
                                ),
                        ),
                )
                .children(self.release.as_ref().map(|rel| self.update_banner(rel, th, cx)))
                .children(shelves)
                .children(note),
        )
    }

    fn update_banner(&self, rel: &update::Release, th: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let page = rel.page.clone();
        let self_update = self.installed && rel.installer.is_some();
        let action = match (self_update, self.updating) {
            (true, true) => "Updating…",
            (true, false) => "Update now",
            (false, _) => "Download",
        };
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_3()
            .px_4()
            .py_3()
            .rounded_lg()
            .border_1()
            .border_color(th.accent)
            .bg(th.surface)
            .child(icon("download", th.accent).size(px(22.)))
            .child(
                div()
                    .flex_1()
                    .min_w(px(200.))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(format!("Flipbook {} is available", rel.version)))
                    .child(div().text_size(px(12.)).text_color(th.mute).child(if self_update {
                        format!("You have {}. Updating takes a few seconds and keeps your books and progress.", update::CURRENT)
                    } else {
                        format!("You have {}.", update::CURRENT)
                    })),
            )
            .child(btn_base("whats-new", th).text_color(th.mute).child("What's new").on_click(move |_, _, cx| cx.open_url(&page)))
            .when(!self.updating, |d| {
                d.child(btn_base("later", th).text_color(th.mute).child("Later").on_click(cx.listener(|this, _, _, cx| {
                    this.release = None;
                    cx.notify();
                })))
            })
            .child(
                btn_base("update", th)
                    .bg(th.accent)
                    .text_color(th.on_accent)
                    .when(self.updating, |b| b.opacity(0.7))
                    .child(action)
                    .on_click(cx.listener(|this, _, _, cx| this.apply_update(cx))),
            )
    }

    /// One sheet of paper at `x`, `w` wide. Unrendered pages are blank paper.
    fn sheet(&self, r: &Reader, page: Option<usize>, x: f32, w: f32, h: f32, fit: ObjectFit) -> Option<Div> {
        let image = r.images.get(&page?).cloned();
        Some(
            div()
                .absolute()
                .top_0()
                .left(px(x))
                .w(px(w))
                .h(px(h))
                .overflow_hidden()
                .shadow_lg()
                .bg(rgb(pdf::paper_rgb(self.store.paper)))
                .children(image.map(|i| img(i).w(px(w)).h(px(h)).object_fit(fit))),
        )
    }

    /// Part of a page: drawn as if it lay at `img_x`, but only `[clip_x, clip_x + clip_w]` shows.
    /// This is how the curl reveals a page bit by bit without squashing it.
    #[allow(clippy::too_many_arguments)]
    fn clipped(&self, r: &Reader, page: Option<usize>, clip_x: f32, clip_w: f32, img_x: f32, w: f32, h: f32) -> Option<Div> {
        if clip_w < 0.5 {
            return None;
        }
        let sheet = self.sheet(r, page, img_x - clip_x, w, h, ObjectFit::Contain)?;
        Some(div().absolute().top_0().left(px(clip_x)).w(px(clip_w)).h(px(h)).overflow_hidden().child(sheet))
    }

    /// The page or spread lying open at `page`, shifted right by `dx`.
    fn spread(&self, r: &Reader, page: usize, dx: f32, w: f32, h: f32) -> Vec<Div> {
        let (left, right, rx) = if r.two_up {
            let s = spread_of(page);
            (left_of(s), right_of(s, r.n), w)
        } else {
            (None, Some(page), 0.)
        };
        [self.sheet(r, left, dx, w, h, ObjectFit::Contain), self.sheet(r, right, dx + rx, w, h, ObjectFit::Contain)]
            .into_iter()
            .flatten()
            .collect()
    }

    /// The book itself. `w` and `h` are the size of one page.
    fn book(&self, r: &Reader, w: f32, h: f32) -> Div {
        let full = if r.two_up { w * 2. } else { w };
        let book = div().relative().h(px(h)).w(px(full));
        let Some(f) = &r.flip else {
            let s = spread_of(r.page);
            return book
                .children(self.spread(r, r.page, 0., w, h))
                .when(r.two_up && left_of(s).is_some(), |b| b.child(band(w - 18., 18., h, 0., 0.16)))
                .when(r.two_up && right_of(s, r.n).is_some(), |b| b.child(band(w, 18., h, 0.16, 0.)));
        };
        let t = ease(f.start.elapsed().as_secs_f32() / f.secs);
        match f.style {
            1 => self.fold(book, r, f, t, w, h),
            2 => {
                // the whole spread slides away and the next slides in behind it
                let gap = full + 40.;
                let (a, b) = if f.to > f.from { (-gap * t, gap * (1. - t)) } else { (gap * t, -gap * (1. - t)) };
                book.overflow_hidden().children(self.spread(r, f.from, a, w, h)).children(self.spread(r, f.to, b, w, h))
            }
            3 => book
                .children(self.spread(r, f.to, 0., w, h))
                .child(div().absolute().size_full().opacity(1. - t).children(self.spread(r, f.from, 0., w, h))),
            _ => self.curl(book, r, f, t, w, h),
        }
    }

    /// The page folds over and its crease sweeps toward the spine, as if turned and pressed flat.
    /// A backward turn is a forward turn played in reverse.
    fn curl(&self, book: Div, r: &Reader, f: &Flip, t: f32, w: f32, h: f32) -> Div {
        let (from, to, p) = if f.to > f.from { (f.from, f.to, t) } else { (f.to, f.from, 1. - t) };
        let lift = (std::f32::consts::PI * p).sin(); // how far the page stands off the paper
        if r.two_up {
            let (a, b) = (spread_of(from), spread_of(to));
            let x = 2. * w - w * p; // the crease: outer edge -> spine
            let back = 2. * x - 2. * w; // left edge of the turned-over part
            let bent = (x - back).min(50.);
            book.children(self.sheet(r, left_of(a), 0., w, h, ObjectFit::Contain))
                .children(self.sheet(r, right_of(b, r.n), w, w, h, ObjectFit::Contain))
                // what is left of the page still lying flat
                .children(self.clipped(r, right_of(a, r.n), w, back - w, w, w, h))
                // shadow the lifted page casts on the page it uncovers
                .child(band(x, (2. * w - x).min(70.), h, 0.38 * lift, 0.))
                // shadow under the turned page's outer edge
                .child(band((back - 24.).max(0.), 24., h, 0., 0.3 * lift))
                // the back of the turning page, already showing what will be the left page
                .children(self.clipped(r, left_of(b), back, x - back, back, w, h))
                // darker toward the crease, where the paper bends away from the light
                .child(band(x - bent, bent, h, 0., 0.25 * lift))
        } else {
            let x = w * (1. - p); // the crease: right edge -> left edge
            let back = 2. * x - w;
            let bent = (x - back).min(50.);
            // past halfway the turned page leaves the book to the left; clip it there
            book.overflow_hidden()
                .children(self.sheet(r, Some(to), 0., w, h, ObjectFit::Contain))
                .children(self.clipped(r, Some(from), 0., back, 0., w, h))
                .child(band(x, (w - x).min(70.), h, 0.38 * lift, 0.))
                // the back of a single page is blank paper (usize::MAX is never a rendered page)
                .children(self.clipped(r, Some(usize::MAX), back, x - back, back, w, h).map(|d| {
                    d.child(band(0., x - back, h, 0.05, 0.05)).child(band(x - back - bent, bent, h, 0., 0.25 * lift))
                }))
        }
    }

    /// The turning leaf narrows toward the spine as it stands up, then lays down on the other side.
    fn fold(&self, book: Div, r: &Reader, f: &Flip, t: f32, w: f32, h: f32) -> Div {
        // the spine side of a standing leaf is in shadow
        let leaf = |page, x: f32, lw: f32, dim: f32, spine_left: bool| {
            self.sheet(r, page, x, lw, h, ObjectFit::Fill).map(|s| {
                let (a, b) = if spine_left { (0.5 * dim, 0.12 * dim) } else { (0.12 * dim, 0.5 * dim) };
                s.child(band(0., lw, h, a, b))
            })
        };
        if r.two_up {
            let (a, b) = (spread_of(f.from), spread_of(f.to));
            let forward = b > a;
            let c = (std::f32::consts::PI * t).cos();
            let lw = w * c.abs();
            let dim = 1. - c.abs();
            let (under_l, under_r) = if forward { (left_of(a), right_of(b, r.n)) } else { (left_of(b), right_of(a, r.n)) };
            // first half: the departing page folds up; second half: the arriving page lays down opposite
            let (page, on_right) = match (forward, c > 0.) {
                (true, true) => (right_of(a, r.n), true),
                (true, false) => (left_of(b), false),
                (false, true) => (left_of(a), false),
                (false, false) => (right_of(b, r.n), true),
            };
            let shadow = (w - lw).min(60.);
            book.children(self.sheet(r, under_l, 0., w, h, ObjectFit::Contain))
                .children(self.sheet(r, under_r, w, w, h, ObjectFit::Contain))
                .child(if on_right { band(w + lw, shadow, h, 0.3 * dim, 0.) } else { band(w - lw - shadow, shadow, h, 0., 0.3 * dim) })
                .children(leaf(page, if on_right { w } else { w - lw }, lw, dim, on_right))
        } else {
            // single pages fold away to the left, or come back from it
            let (under, top, lw) = if f.to > f.from { (f.to, f.from, w * (1. - t)) } else { (f.from, f.to, w * t) };
            let dim = 1. - lw / w;
            book.children(self.sheet(r, Some(under), 0., w, h, ObjectFit::Contain))
                .child(band(lw, (w - lw).min(60.), h, 0.3 * dim, 0.))
                .children(leaf(Some(top), 0., lw, dim, true))
        }
    }

    fn panel(&self, r: &Reader, th: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let row = |id: (&'static str, usize), label: String, page: usize, indent: usize, cx: &mut Context<Self>| {
            let hover = th.line;
            div()
                .id(id)
                .flex()
                .gap_2()
                .px_2()
                .py_1()
                .pl(px(8. + 14. * indent as f32))
                .rounded_md()
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .when(indent == 0, |d| d.font_weight(FontWeight::MEDIUM))
                .on_click(cx.listener(move |this, _, _, cx| this.goto(page, true, cx)))
                .child(div().flex_1().min_w_0().truncate().child(label))
                .child(div().text_color(th.mute).child((page + 1).to_string()))
        };
        let head = |s: &'static str| div().px_2().pt_3().pb_1().text_size(px(12.)).text_color(th.mute).child(s);
        let marks = self.store.book(&r.path).bookmarks;
        div()
            .id("panel")
            .absolute()
            .left_0()
            .top(px(BAR))
            .bottom(px(FOOT))
            .w(px(300.))
            .p_2()
            .overflow_y_scroll()
            .occlude()
            .bg(th.surface)
            .border_1()
            .border_color(th.line)
            .rounded_r_lg()
            .shadow_lg()
            .text_size(px(13.))
            .child(head("BOOKMARKS"))
            .when(marks.is_empty(), |d| d.child(div().px_2().text_color(th.mute).child("Press B to bookmark a page.")))
            .children(marks.iter().enumerate().map(|(i, &p)| row(("mark", i), format!("Page {}", p + 1), p, 0, cx)))
            .when(!r.toc.is_empty(), |d| d.child(head("CONTENTS")))
            .children(r.toc.iter().enumerate().map(|(i, t)| row(("toc", i), t.title.clone(), t.page, t.depth.min(4), cx)))
    }

    fn reader(&mut self, th: &Theme, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = window.viewport_size();
        let (vw, vh) = (f32::from(view.width), f32::from(view.height));
        let single = self.store.single;
        let paper = self.store.paper;
        let dark = self.dark(window);

        // ----- layout, and the state that depends on it -----
        let r = self.reader.as_mut().unwrap();
        let (avail_w, avail_h) = ((vw - 40.).max(100.), (vh - BAR - FOOT - 16.).max(100.));
        let two_up = !single && r.n > 1 && avail_w >= avail_h * r.ratio * 1.3;
        let h = avail_h.min(avail_w / (r.ratio * if two_up { 2. } else { 1. })).floor();
        let w = (h * r.ratio).floor();
        if two_up != r.two_up {
            r.two_up = two_up;
            r.flip = None;
            if two_up {
                r.page = first_of(spread_of(r.page));
            }
        }
        if r.flip.as_ref().is_some_and(|f| f.start.elapsed().as_secs_f32() >= f.secs) {
            r.flip = None;
        }
        if r.flip.is_some() {
            window.request_animation_frame();
        }
        // keep textures only near the reader (and under a turn in progress)
        let (page, from) = (r.page, r.flip.as_ref().map(|f| f.from));
        let far: Vec<usize> =
            r.images.keys().copied().filter(|&i| i.abs_diff(page) > 8 && from.is_none_or(|f| i.abs_diff(f) > 2)).collect();
        let stale: Vec<_> = far.iter().filter_map(|i| r.images.remove(i)).collect();
        // rasterise at the size on screen, in steps so dragging the window edge doesn't re-render every pixel
        let want = ((h * window.scale_factor() / 160.).ceil() * 160.).min(3200.);
        let resized = want != r.render_h;
        r.render_h = want;
        free(stale.into_iter(), Some(window), cx);
        if resized {
            self.rerender(window, cx);
        } else {
            self.ensure(cx);
        }

        // ----- elements -----
        let r = self.reader.as_ref().unwrap();
        let hidden = r.idle && r.flip.is_none();
        let marked = self.store.book(&r.path).bookmarks.binary_search(&r.page).is_ok();
        let last = if r.two_up { right_of(spread_of(r.page), r.n).unwrap_or(r.page).max(r.page) } else { r.page };
        let count = match &r.jump {
            Some(j) => format!("Go to {j}▏"),
            None if last > r.page => format!("{}–{} / {}", r.page + 1, last + 1, r.n),
            None => format!("{} / {}", r.page + 1, r.n),
        };

        // seek bar: fixed widths so a click's x maps straight to a page
        let track = (vw * 0.45).min(380.);
        let track_x = (vw - (track + 212.)) / 2. + 46.;
        let frac = if r.n > 1 { r.page as f32 / (r.n - 1) as f32 } else { 1. };
        let n = r.n;
        let seek = move |this: &mut Self, at: Point<Pixels>, cx: &mut Context<Self>| {
            let f = ((f32::from(at.x) - track_x) / track).clamp(0., 1.);
            this.goto((f * (n - 1) as f32).round() as usize, false, cx);
        };
        let nav = |id, name: &str, tip: &'static str, off: bool| ibtn(id, name, tip, th.ink, th).when(off, |b| b.opacity(0.3));
        let turn = (self.store.turn as usize).min(TURNS.len() - 1);
        let full = window.is_fullscreen();
        let on = |yes: bool| if yes { th.accent } else { th.mute };

        let bar = div()
            .h(px(BAR))
            .px_3()
            .flex()
            .flex_none()
            .items_center()
            .gap_1()
            .text_color(th.mute)
            .child(ibtn("back", "arrow-left", "Back to the shelf (Esc)", th.mute, th).on_click(cx.listener(|this, _, window, cx| this.close(Some(window), cx))))
            .child(ibtn("contents", "list", "Contents and bookmarks (T)", on(r.panel), th).on_click(cx.listener(|this, _, _, cx| this.toggle_panel(cx))))
            .child(div().flex_1().min_w_0().truncate().text_center().font_family(SERIF).text_size(px(16.)).text_color(th.ink).child(r.title.clone()))
            .child(
                ibtn("mark", if marked { "bookmark-fill" } else { "bookmark" }, if marked { "Remove bookmark (B)" } else { "Bookmark this page (B)" }, on(marked), th)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_bookmark(cx))),
            )
            .child(ibtn("turn", "page-turn", format!("Page turn: {} (S)", TURNS[turn]), th.mute, th).on_click(cx.listener(|this, _, _, cx| this.cycle_turn(cx))))
            .child(ibtn("paper", "paper", format!("Paper: {} (P)", PAPERS[paper.min(2) as usize]), th.mute, th).on_click(cx.listener(|this, _, window, cx| this.cycle_paper(window, cx))))
            .child(
                ibtn("pages", if single { "one-page" } else { "two-pages" }, if single { "One page (D)" } else { "Two pages (D)" }, th.mute, th)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_single(cx))),
            )
            .child(
                ibtn("auto", "timer", if r.auto_secs > 0 { format!("Auto-turn every {} seconds (A)", r.auto_secs) } else { "Auto-turn (A)".into() }, on(r.auto_secs > 0), th)
                    .when(r.auto_secs > 0, |b| b.child(div().text_size(px(12.)).text_color(th.accent).child(format!("{}s", r.auto_secs))))
                    .on_click(cx.listener(|this, _, _, cx| this.cycle_auto(cx))),
            )
            .child(ibtn("theme", if dark { "sun" } else { "moon" }, if dark { "Light theme" } else { "Dark theme" }, th.mute, th).on_click(cx.listener(|this, _, window, cx| this.toggle_theme(window, cx))))
            .child(ibtn("full", if full { "minimize" } else { "maximize" }, if full { "Leave full screen (F)" } else { "Full screen (F)" }, th.mute, th).on_click(cx.listener(|_, _, window, _| window.toggle_fullscreen())));

        let stage = div()
            .id("stage")
            .flex_1()
            .min_h_0()
            .flex()
            .items_center()
            .justify_center()
            // click the left or right half of the window to turn, or use the wheel
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, e: &gpui::MouseDownEvent, _, cx| {
                this.step(if f32::from(e.position.x) < vw / 2. { -1 } else { 1 }, cx)
            }))
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, cx| {
                if this.reader.as_ref().is_some_and(|r| r.flip.is_none()) {
                    let dy = f32::from(e.delta.pixel_delta(px(20.)).y);
                    if dy != 0. {
                        this.step(if dy < 0. { 1 } else { -1 }, cx);
                    }
                }
            }))
            .child(self.book(r, w, h).when(marked && r.flip.is_none(), |b| {
                b.child(div().absolute().top(px(-4.)).right(px(22.)).w(px(14.)).h(px(34.)).rounded_b_sm().bg(th.accent).shadow_md())
            }));

        let foot = div()
            .h(px(FOOT))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(10.))
            .child(nav("prev", "chevron-left", "Previous page (←)", r.page == 0).on_click(cx.listener(|this, _, _, cx| this.step(-1, cx))))
            .child(
                div()
                    .id("seek")
                    .relative()
                    .flex_none()
                    .w(px(track))
                    .h(px(24.))
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, cx.listener(move |this, e: &gpui::MouseDownEvent, _, cx| seek(this, e.position, cx)))
                    .on_mouse_move(cx.listener(move |this, e: &gpui::MouseMoveEvent, _, cx| {
                        if e.pressed_button == Some(MouseButton::Left) {
                            seek(this, e.position, cx);
                        }
                    }))
                    .child(div().absolute().top(px(10.)).w_full().h(px(4.)).rounded_full().bg(th.line))
                    .child(div().absolute().top(px(10.)).w(px(track * frac)).h(px(4.)).rounded_full().bg(th.accent))
                    .child(div().absolute().top(px(5.)).left(px(track * frac - 7.)).size(px(14.)).rounded_full().bg(th.accent).border_2().border_color(th.surface)),
            )
            .child(div().flex_none().w(px(110.)).text_center().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(count))
            .child(nav("next", "chevron-right", "Next page (→)", last + 1 >= r.n).on_click(cx.listener(|this, _, _, cx| this.step(1, cx))));

        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(th.stage)
            .on_mouse_move(cx.listener(|this, _, _, cx| this.wake(cx)))
            // hidden controls are swapped for spacers: GPUI's opacity doesn't reliably hide nested children
            .child(if hidden { div().h(px(BAR)).flex_none() } else { bar })
            .child(stage)
            .child(if hidden { div().h(px(FOOT)).flex_none() } else { foot })
            .when(r.panel, |d| d.child(self.panel(r, th, cx)))
    }
}

impl Render for Flipbook {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focus.is_focused(window) {
            window.focus(&self.focus);
        }
        let th = theme(self.dark(window));
        let body = if self.reader.is_some() {
            self.reader(&th, window, cx).into_any_element()
        } else {
            self.library(&th, window, cx).into_any_element()
        };
        div()
            .size_full()
            .relative()
            .bg(th.bg)
            .text_color(th.ink)
            .text_size(px(14.))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key))
            .on_drop(cx.listener(Self::dropped))
            .drag_over::<ExternalPaths>(|s, _, _, _| s.opacity(0.6))
            .child(body)
            .children(self.toast.clone().map(|msg| {
                div().absolute().bottom(px(72.)).w_full().flex().justify_center().child(
                    div().px_4().py_2().rounded_lg().bg(th.ink).text_color(th.bg).text_size(px(13.)).shadow_lg().child(msg),
                )
            }))
    }
}

fn main() {
    Application::new().with_assets(icons::Icons).run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions { title: Some("Flipbook".into()), ..Default::default() }),
                window_min_size: Some(size(px(600.), px(480.))),
                app_id: Some("flipbook".into()),
                ..Default::default()
            },
            |_, cx| cx.new(Flipbook::new),
        )
        .unwrap();
        cx.on_window_closed(|cx| cx.quit()).detach();
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spreads() {
        // 5 pages: [_,0] [1,2] [3,4]
        assert_eq!([0, 1, 2, 3, 4].map(spread_of), [0, 1, 1, 2, 2]);
        assert_eq!([0, 1, 2].map(first_of), [0, 1, 3]);
        assert_eq!((left_of(0), right_of(0, 5)), (None, Some(0)));
        assert_eq!((left_of(2), right_of(2, 5)), (Some(3), Some(4)));
        // 4 pages end on a lone left page: [3,_]
        assert_eq!((left_of(2), right_of(2, 4)), (Some(3), None));
        assert_eq!(first_of(4 / 2), 3);
    }
}
