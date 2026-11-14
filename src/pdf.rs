//! Everything that touches a PDF or the disk: folder scan, rasterising, covers, table of contents.

use gpui::RenderImage;
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::object::{Array, Dict, Object, ObjectIdentifier, String as PdfString};
use hayro::hayro_syntax::{LoadPdfError, Pdf};
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{render, PixmapSettings, RenderCache, RenderSettings};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use walkdir::{DirEntry, WalkDir};

pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub folder: String,
    pub size: u64,
}

pub struct TocItem {
    pub depth: usize,
    pub title: String,
    pub page: usize,
}

pub fn is_pdf(p: &Path) -> bool {
    p.extension().and_then(|x| x.to_str()).is_some_and(|x| x.eq_ignore_ascii_case("pdf"))
}

/// "Rust Book.pdf" -> "Rust Book"
pub fn title(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn skip(e: &DirEntry) -> bool {
    e.depth() > 0
        && e.file_name().to_str().is_some_and(|s| {
            s.starts_with('.') || s == "node_modules" || s == "$RECYCLE.BIN" || s == "System Volume Information"
        })
}

/// "Chapter 2" before "Chapter 10": digit runs compare as numbers, the rest case-insensitively.
pub fn natural(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let num = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut n = 0u64;
                    while let Some(d) = it.peek().and_then(|c| c.to_digit(10)) {
                        n = n.saturating_mul(10).saturating_add(d as u64);
                        it.next();
                    }
                    n
                };
                match num(&mut a).cmp(&num(&mut b)) {
                    Ordering::Equal => {}
                    o => return o,
                }
            }
            (Some(x), Some(y)) => {
                match x.to_lowercase().cmp(y.to_lowercase()) {
                    Ordering::Equal => {}
                    o => return o,
                }
                a.next();
                b.next();
            }
        }
    }
}

/// Every PDF under `root`, tagged with its sub-folder, sorted by folder then name.
pub fn scan(root: &Path) -> Option<Vec<Entry>> {
    if !root.is_dir() {
        return None;
    }
    let root_name = root.file_name().map_or_else(|| root.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned());
    let mut out = Vec::new();
    for e in WalkDir::new(root).follow_links(false).max_depth(12).into_iter().filter_entry(|e| !skip(e)).filter_map(Result::ok) {
        let p = e.path();
        if !e.file_type().is_file() || !is_pdf(p) {
            continue;
        }
        let rel = p
            .parent()
            .and_then(|d| d.strip_prefix(root).ok())
            .map(|d| d.to_string_lossy().replace('\\', "/").replace('/', " / "))
            .unwrap_or_default();
        out.push(Entry {
            path: p.to_path_buf(),
            name: title(p),
            folder: if rel.is_empty() { root_name.clone() } else { format!("{root_name} / {rel}") },
            size: e.metadata().map(|m| m.len()).unwrap_or(0),
        });
    }
    out.sort_by(|a, b| natural(&a.folder, &b.folder).then_with(|| natural(&a.name, &b.name)));
    Some(out)
}

pub fn load(path: &Path) -> Result<(Arc<Pdf>, Vec<TocItem>), String> {
    if !is_pdf(path) {
        return Err("Flipbook opens PDFs and folders of PDFs.".into());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("Could not open that file: {e}"))?;
    // PDFs are untrusted input and the parser is young: a panic must not take the app down.
    let pdf = catch_unwind(|| Pdf::new(bytes)).map_err(|_| "Could not read that PDF.".to_string())?.map_err(|e| match e {
        LoadPdfError::Decryption(_) => "This PDF is password-protected.".to_string(),
        LoadPdfError::Invalid => "Could not read that PDF.".to_string(),
    })?;
    if pdf.pages().is_empty() {
        return Err("That PDF has no pages.".into());
    }
    let toc = catch_unwind(AssertUnwindSafe(|| outline(&pdf))).unwrap_or_default();
    Ok((Arc::new(pdf), toc))
}

/// width / height of a page as displayed.
pub fn ratio(pdf: &Pdf, idx: usize) -> f32 {
    pdf.pages().get(idx).map(|p| p.render_dimensions()).map_or(0.7, |(w, h)| w / h.max(1.0))
}

/// Paper tints, applied per pixel after rasterising. `rgb` is one RGBA pixel.
fn tint(paper: u8, rgb: &mut [u8]) {
    match paper {
        // sepia: multiply by warm paper
        1 => {
            for (c, m) in rgb.iter_mut().zip([0xf4u16, 0xec, 0xd8]) {
                *c = (*c as u16 * m / 255) as u8;
            }
        }
        // night: invert onto dark paper. ponytail: photos invert too; a real night mode would leave images alone.
        2 => {
            for (c, (bg, fg)) in rgb.iter_mut().zip([(0x1au16, 0xd8u16), (0x1a, 0xd0), (0x1a, 0xc0)]) {
                *c = (bg + (255 - *c as u16) * (fg - bg) / 255) as u8;
            }
        }
        _ => {}
    }
}

/// The colour of an empty sheet for each paper, so unrendered pages don't flash white at night.
pub fn paper_rgb(paper: u8) -> u32 {
    [0xffffff, 0xf4ecd8, 0x1a1a1a][paper.min(2) as usize]
}

/// Rasterise one page to RGBA at roughly `height` pixels tall.
fn raster(pdf: &Pdf, idx: usize, height: f32, paper: u8) -> Option<(u32, u32, Vec<u8>)> {
    catch_unwind(AssertUnwindSafe(|| {
        let page = pdf.pages().get(idx)?;
        let (w, h) = page.render_dimensions();
        if w < 1.0 || h < 1.0 {
            return None;
        }
        // vello_cpu pixmaps are u16-sized; stay well inside that for very wide pages
        let scale = (height / h).min(6000.0 / w.max(h));
        // A fresh cache per page: it has no way to be cleared, so a shared one would grow with the book.
        let pix = render(
            page,
            &RenderCache::new(),
            &InterpreterSettings::default(),
            &RenderSettings::default(),
            &PixmapSettings { x_scale: scale, y_scale: scale, bg_color: WHITE },
        );
        let mut data = pix.data_as_u8_slice().to_vec();
        if paper != 0 {
            data.chunks_exact_mut(4).for_each(|px| tint(paper, &mut px[..3]));
        }
        Some((pix.width() as u32, pix.height() as u32, data))
    }))
    .ok()
    .flatten()
}

/// One page as a GPU-ready image (GPUI wants BGRA).
pub fn page_image(pdf: &Pdf, idx: usize, height: f32, paper: u8) -> Option<Arc<RenderImage>> {
    let (w, h, mut data) = raster(pdf, idx, height, paper)?;
    data.chunks_exact_mut(4).for_each(|px| px.swap(0, 2));
    let frame = image::Frame::new(image::RgbaImage::from_raw(w, h, data)?);
    Some(Arc::new(RenderImage::new(smallvec::SmallVec::from_elem(frame, 1))))
}

/// Where the cover thumbnail of a book lives on disk.
pub fn cover_file(book: &Path) -> PathBuf {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    book.hash(&mut h);
    crate::store::data_dir().join("covers").join(format!("{:016x}.png", h.finish()))
}

/// Render page 1 to `out`. Returns false for unreadable PDFs, which keep their title cover.
// ponytail: reads the whole file for one page; fine for books, slow for 500 MB scans
pub fn make_cover(book: &Path, out: &Path) -> bool {
    let Ok(bytes) = std::fs::read(book) else { return false };
    let Ok(Ok(pdf)) = catch_unwind(|| Pdf::new(bytes)) else { return false };
    let Some((w, h, data)) = raster(&pdf, 0, 420.0, 0) else { return false };
    out.parent().is_some_and(|d| std::fs::create_dir_all(d).is_ok())
        && image::save_buffer(out, &data, w, h, image::ColorType::Rgba8).is_ok()
}

// ---------- table of contents ----------

/// PDF text strings are UTF-16BE with a BOM, UTF-8 with a BOM, or (roughly) Latin-1.
fn text(b: &[u8]) -> String {
    let s = if let Some(u) = b.strip_prefix(&[0xfe, 0xff]) {
        String::from_utf16_lossy(&u.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect::<Vec<_>>())
    } else if let Some(u) = b.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        String::from_utf8_lossy(u).into_owned()
    } else {
        b.iter().map(|&c| c as char).collect()
    };
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

struct Toc<'a> {
    catalog: Dict<'a>,
    pages: HashMap<ObjectIdentifier, usize>,
    out: Vec<TocItem>,
    /// Outline items visited; bounds hostile files whose /Next pointers loop.
    budget: usize,
}

fn outline(pdf: &Pdf) -> Vec<TocItem> {
    let xref = pdf.xref();
    let Some(catalog) = xref.get::<Dict>(xref.root_id()) else { return Vec::new() };
    let pages = pdf.pages().iter().enumerate().filter_map(|(i, p)| Some((p.raw().obj_id()?, i))).collect();
    let first = catalog.get::<Dict>(b"Outlines").and_then(|o| o.get::<Dict>(b"First"));
    let mut toc = Toc { catalog, pages, out: Vec::new(), budget: 5000 };
    if let Some(first) = first {
        toc.walk(first, 0);
    }
    toc.out
}

impl<'a> Toc<'a> {
    fn walk(&mut self, mut item: Dict<'a>, depth: usize) {
        loop {
            if self.budget == 0 {
                return;
            }
            self.budget -= 1;
            let dest = item.get::<Object>(b"Dest").or_else(|| item.get::<Dict>(b"A")?.get::<Object>(b"D"));
            let title = item.get::<PdfString>(b"Title").map(|s| text(&s)).unwrap_or_default();
            if let (false, Some(page)) = (title.is_empty(), dest.and_then(|d| self.page_of(d, 0))) {
                self.out.push(TocItem { depth, title, page });
            }
            if depth < 6 {
                if let Some(kid) = item.get::<Dict>(b"First") {
                    self.walk(kid, depth + 1);
                }
            }
            match item.get::<Dict>(b"Next") {
                Some(next) => item = next,
                None => return,
            }
        }
    }

    /// A destination is `[page /Fit ...]`, a dict wrapping one in /D, or a name to look up.
    fn page_of(&self, dest: Object<'a>, hops: usize) -> Option<usize> {
        if hops > 4 {
            return None;
        }
        if let Some(arr) = dest.clone().into_array() {
            let id: ObjectIdentifier = arr.raw_iter().next()?.as_obj_ref()?.into();
            return self.pages.get(&id).copied();
        }
        if let Some(d) = dest.clone().into_dict() {
            return self.page_of(d.get::<Object>(b"D")?, hops + 1);
        }
        if let Some(name) = dest.clone().into_name() {
            return self.page_of(self.catalog.get::<Dict>(b"Dests")?.get::<Object>(&*name)?, hops + 1);
        }
        let key = dest.into_string()?;
        let tree = self.catalog.get::<Dict>(b"Names")?.get::<Dict>(b"Dests")?;
        self.page_of(find_name(&tree, &key, 0)?, hops + 1)
    }
}

// ponytail: linear walk of the name tree per lookup (ignores /Limits); binary search it if huge TOCs open slowly
fn find_name<'a>(node: &Dict<'a>, key: &[u8], depth: usize) -> Option<Object<'a>> {
    if let Some(names) = node.get::<Array>(b"Names") {
        let mut it = names.iter::<Object>();
        while let (Some(k), Some(v)) = (it.next(), it.next()) {
            if k.into_string().is_some_and(|s| &*s == key) {
                return Some(v);
            }
        }
    }
    if depth < 12 {
        for kid in node.get::<Array>(b"Kids")?.iter::<Dict>() {
            if let Some(v) = find_name(&kid, key, depth + 1) {
                return Some(v);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order() {
        let mut v = vec!["Chapter 10", "chapter 2", "Chapter 1", "Appendix"];
        v.sort_by(|a, b| natural(a, b));
        assert_eq!(v, ["Appendix", "Chapter 1", "chapter 2", "Chapter 10"]);
    }

    #[test]
    fn pdf_text() {
        assert_eq!(text(&[0xfe, 0xff, 0, b'H', 0, b'i']), "Hi");
        assert_eq!(text(b"  Caf\xe9  au\nlait "), "Café au lait");
    }

    #[test]
    fn tints() {
        let mut white = [255, 255, 255];
        tint(2, &mut white);
        assert_eq!(white, [0x1a, 0x1a, 0x1a]);
        let mut black = [0, 0, 0];
        tint(2, &mut black);
        assert_eq!(black, [0xd8, 0xd0, 0xc0]);
    }
}
