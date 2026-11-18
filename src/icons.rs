//! The app's icons: 24×24 stroke drawings, served to GPUI as `icons/<name>.svg`.
//! GPUI uses an SVG's shape as a mask and paints it in the element's text colour.

use gpui::{AssetSource, Result, SharedString};
use std::borrow::Cow;

const ICONS: &[(&str, &str)] = &[
    ("arrow-left", r#"<path d="M19 12H5M12 19l-7-7 7-7"/>"#),
    ("list", r#"<path d="M9 6h12M9 12h12M9 18h12M4 6h.01M4 12h.01M4 18h.01"/>"#),
    ("bookmark", r#"<path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z"/>"#),
    ("bookmark-fill", r##"<path fill="#000" d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z"/>"##),
    // a page with its corner turning over
    ("page-turn", r#"<path d="M14 21H6a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h12a2 2 0 0 1 2 2v9z"/><path d="M20 14h-4a2 2 0 0 0-2 2v5"/>"#),
    // half-filled circle: paper tint
    ("paper", r##"<circle cx="12" cy="12" r="9"/><path fill="#000" d="M12 3a9 9 0 0 1 0 18z"/>"##),
    ("one-page", r#"<rect x="6" y="3" width="12" height="18" rx="2"/>"#),
    ("two-pages", r#"<path d="M2 5h6a4 4 0 0 1 4 4v12a3 3 0 0 0-3-3H2zM22 5h-6a4 4 0 0 0-4 4v12a3 3 0 0 1 3-3h7z"/>"#),
    ("timer", r#"<circle cx="12" cy="13" r="8"/><path d="M12 9v4l2.5 2.5M9 2h6"/>"#),
    ("sun", r#"<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M6.3 17.7l-1.4 1.4M19.1 4.9l-1.4 1.4"/>"#),
    ("moon", r#"<path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9z"/>"#),
    ("maximize", r#"<path d="M8 3H5a2 2 0 0 0-2 2v3M21 8V5a2 2 0 0 0-2-2h-3M16 21h3a2 2 0 0 0 2-2v-3M3 16v3a2 2 0 0 0 2 2h3"/>"#),
    ("minimize", r#"<path d="M8 3v3a2 2 0 0 1-2 2H3M21 8h-3a2 2 0 0 1-2-2V3M16 21v-3a2 2 0 0 1 2-2h3M3 16h3a2 2 0 0 1 2 2v3"/>"#),
    ("chevron-left", r#"<path d="M15 18l-6-6 6-6"/>"#),
    ("chevron-right", r#"<path d="M9 18l6-6-6-6"/>"#),
    ("folder", r#"<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>"#),
    ("file-plus", r#"<path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M14 2v6h6M12 12v6M9 15h6"/>"#),
    ("search", r#"<circle cx="11" cy="11" r="7"/><path d="M20 20l-3.5-3.5"/>"#),
];

pub fn svg_for(name: &str) -> Option<String> {
    ICONS.iter().find(|(n, _)| *n == name).map(|(_, body)| {
        format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{body}</svg>"##
        )
    })
}

pub struct Icons;

impl AssetSource for Icons {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let name = path.strip_prefix("icons/").and_then(|p| p.strip_suffix(".svg"));
        Ok(name.and_then(svg_for).map(|s| Cow::Owned(s.into_bytes())))
    }

    fn list(&self, _path: &str) -> Result<Vec<SharedString>> {
        Ok(ICONS.iter().map(|(n, _)| format!("icons/{n}.svg").into()).collect())
    }
}
