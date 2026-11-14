// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod pdf;
mod store;

use gpui::{
    div, img, linear_color_stop, linear_gradient, prelude::*, px, rgb, size, App, Application, Bounds, Context, Div,
    ExternalPaths, FocusHandle, FontWeight, Hsla, KeyDownEvent, MouseButton, ObjectFit, PathPromptOptions, Pixels,
    Point, RenderImage, Rgba, ScrollWheelEvent, SharedString, Stateful, Task, TitlebarOptions, Window,
    WindowAppearance, WindowBounds, WindowOptions,
};
use hayro::hayro_syntax::Pdf;
use pdf::{Entry, TocItem};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use store::Store;

const FLIP: f32 = 0.55; // seconds per page turn
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
        };
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
        r.flip = animate.then(|| Flip { from: r.page, to: page, start: Instant::now() });
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
        self.rerender(window, cx);
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

fn btn(id: &'static str, label: impl Into<SharedString>, th: &Theme) -> Stateful<Div> {
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
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .child(label.into())
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
            _ => "Your PDFs as page-turning books. Nothing leaves this computer.".into(),
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
                    .child(div().font_family(SERIF).text_size(px(21.)).text_color(th.ink).child("Your shelf is empty"))
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
            .overflow_hidden()
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
                                    btn("pick-dir", "Choose folder", th)
                                        .border_1()
                                        .border_color(th.line)
                                        .bg(th.surface)
                                        .on_click(cx.listener(|this, _, _, cx| this.pick(true, cx))),
                                )
                                .child(
                                    btn("pick-file", "Open PDF", th)
                                        .bg(th.accent)
                                        .text_color(th.on_accent)
                                        .on_click(cx.listener(|this, _, _, cx| this.pick(false, cx))),
                                )
                                .child(
                                    btn("theme", if self.dark(window) { "Light" } else { "Dark" }, th)
                                        .text_color(th.mute)
                                        .on_click(cx.listener(|this, _, window, cx| this.toggle_theme(window, cx))),
                                ),
                        ),
                )
                .children(shelves)
                .children(note),
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

    /// The book itself. `w` and `h` are the size of one page.
    fn book(&self, r: &Reader, w: f32, h: f32) -> Div {
        let t = r.flip.as_ref().map(|f| (f, ease(f.start.elapsed().as_secs_f32() / FLIP)));
        let book = div().relative().h(px(h)).w(px(if r.two_up { w * 2. } else { w }));
        // the turning leaf is squashed toward the spine, and darkens as it stands up
        let leaf = |page, x, lw: f32, dim: f32| {
            self.sheet(r, page, x, lw, h, ObjectFit::Fill).map(|s| s.child(div().absolute().size_full().bg(shade(0.4 * dim))))
        };
        match (r.two_up, t) {
            (true, None) => {
                let s = spread_of(r.page);
                let gutter = |x: f32, a: f32, b: f32| {
                    div().absolute().top_0().left(px(x)).w(px(18.)).h(px(h)).bg(linear_gradient(
                        90.,
                        linear_color_stop(shade(a), 0.),
                        linear_color_stop(shade(b), 1.),
                    ))
                };
                book.children(self.sheet(r, left_of(s), 0., w, h, ObjectFit::Contain))
                    .children(self.sheet(r, right_of(s, r.n), w, w, h, ObjectFit::Contain))
                    .when(left_of(s).is_some(), |b| b.child(gutter(w - 18., 0., 0.16)))
                    .when(right_of(s, r.n).is_some(), |b| b.child(gutter(w, 0.16, 0.)))
            }
            (true, Some((f, t))) => {
                let (a, b) = (spread_of(f.from), spread_of(f.to));
                let forward = b > a;
                let c = (std::f32::consts::PI * t).cos();
                let lw = w * c.abs();
                let (under_l, under_r) = if forward { (left_of(a), right_of(b, r.n)) } else { (left_of(b), right_of(a, r.n)) };
                // first half: the departing page folds up; second half: the arriving page lays down opposite
                let (page, on_right) = match (forward, c > 0.) {
                    (true, true) => (right_of(a, r.n), true),
                    (true, false) => (left_of(b), false),
                    (false, true) => (left_of(a), false),
                    (false, false) => (right_of(b, r.n), true),
                };
                book.children(self.sheet(r, under_l, 0., w, h, ObjectFit::Contain))
                    .children(self.sheet(r, under_r, w, w, h, ObjectFit::Contain))
                    .children(leaf(page, if on_right { w } else { w - lw }, lw, 1. - c.abs()))
            }
            (false, None) => book.children(self.sheet(r, Some(r.page), 0., w, h, ObjectFit::Contain)),
            (false, Some((f, t))) => {
                // single pages peel off to the left, or come back from it
                let (under, top, lw) = if f.to > f.from { (f.to, f.from, w * (1. - t)) } else { (f.from, f.to, w * t) };
                book.children(self.sheet(r, Some(under), 0., w, h, ObjectFit::Contain)).children(leaf(Some(top), 0., lw, 1. - lw / w))
            }
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
        if r.flip.as_ref().is_some_and(|f| f.start.elapsed().as_secs_f32() >= FLIP) {
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
        let nav = |id, label: &'static str, off: bool| btn(id, label, th).w(px(36.)).px_0().text_size(px(20.)).when(off, |b| b.opacity(0.3));

        let bar = div()
            .h(px(BAR))
            .px_3()
            .flex()
            .flex_none()
            .items_center()
            .gap_1()
            .text_color(th.mute)
            .child(btn("back", "← Library", th).on_click(cx.listener(|this, _, window, cx| this.close(Some(window), cx))))
            .child(btn("contents", "Contents", th).on_click(cx.listener(|this, _, _, cx| this.toggle_panel(cx))))
            .child(div().flex_1().min_w_0().truncate().text_center().font_family(SERIF).text_size(px(16.)).text_color(th.ink).child(r.title.clone()))
            .child(btn("mark", if marked { "Bookmarked" } else { "Bookmark" }, th).when(marked, |b| b.text_color(th.accent)).on_click(cx.listener(|this, _, _, cx| this.toggle_bookmark(cx))))
            .child(btn("paper", PAPERS[paper.min(2) as usize], th).on_click(cx.listener(|this, _, window, cx| this.cycle_paper(window, cx))))
            .child(btn("pages", if single { "1 page" } else { "2 pages" }, th).on_click(cx.listener(|this, _, _, cx| this.toggle_single(cx))))
            .child(
                btn("auto", if r.auto_secs > 0 { format!("Auto {}s", r.auto_secs) } else { "Auto".into() }, th)
                    .when(r.auto_secs > 0, |b| b.text_color(th.accent))
                    .on_click(cx.listener(|this, _, _, cx| this.cycle_auto(cx))),
            )
            .child(btn("theme", if dark { "Light" } else { "Dark" }, th).on_click(cx.listener(|this, _, window, cx| this.toggle_theme(window, cx))))
            .child(btn("full", "Full screen", th).on_click(cx.listener(|_, _, window, _| window.toggle_fullscreen())));

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
            .child(nav("prev", "‹", r.page == 0).on_click(cx.listener(|this, _, _, cx| this.step(-1, cx))))
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
            .child(nav("next", "›", last + 1 >= r.n).on_click(cx.listener(|this, _, _, cx| this.step(1, cx))));

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
    Application::new().run(|cx: &mut App| {
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
