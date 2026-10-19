// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::Serialize;
use std::path::Path;
use tauri::ipc::Response;
use walkdir::{DirEntry, WalkDir};

#[derive(Serialize)]
struct Entry {
    path: String,
    name: String,
    folder: String,
    size: u64,
}

fn is_pdf(p: &Path) -> bool {
    p.extension()
        .and_then(|x| x.to_str())
        .map(|x| x.eq_ignore_ascii_case("pdf"))
        .unwrap_or(false)
}

fn skip(e: &DirEntry) -> bool {
    e.depth() > 0
        && e.file_name()
            .to_str()
            .map(|s| {
                s.starts_with('.')
                    || s == "node_modules"
                    || s == "$RECYCLE.BIN"
                    || s == "System Volume Information"
            })
            .unwrap_or(false)
}

/// Walk a folder and return every PDF inside it, tagged with its sub-folder.
#[tauri::command]
async fn scan_folder(root: String) -> Result<Vec<Entry>, String> {
    let root_path = Path::new(&root);
    if !root_path.is_dir() {
        return Err("That path is not a folder.".into());
    }
    let root_name = root_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| root.clone());

    let mut out = Vec::new();
    for e in WalkDir::new(root_path)
        .follow_links(false)
        .max_depth(12)
        .into_iter()
        .filter_entry(|e| !skip(e))
        .filter_map(Result::ok)
    {
        let p = e.path();
        if !e.file_type().is_file() || !is_pdf(p) {
            continue;
        }
        let rel = p
            .parent()
            .and_then(|d| d.strip_prefix(root_path).ok())
            .map(|d| d.to_string_lossy().replace('\\', "/").replace('/', " / "))
            .unwrap_or_default();
        let folder = if rel.is_empty() {
            root_name.clone()
        } else {
            format!("{root_name} / {rel}")
        };
        out.push(Entry {
            path: p.to_string_lossy().to_string(),
            name: p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            folder,
            size: e.metadata().map(|m| m.len()).unwrap_or(0),
        });
    }
    Ok(out)
}

/// Return the raw bytes of a PDF (sent to the UI as an ArrayBuffer, no JSON overhead).
#[tauri::command]
async fn read_pdf(path: String) -> Result<Response, String> {
    let p = Path::new(&path);
    if !is_pdf(p) {
        return Err("Only PDF files can be opened.".into());
    }
    std::fs::read(p).map(Response::new).map_err(|e| e.to_string())
}

/// A PDF passed on the command line (Windows "Open with" / file association).
#[tauri::command]
fn initial_file() -> Option<String> {
    std::env::args().skip(1).find(|a| is_pdf(Path::new(a)))
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![scan_folder, read_pdf, initial_file])
        .run(tauri::generate_context!())
        .expect("error while running Flipbook");
}
