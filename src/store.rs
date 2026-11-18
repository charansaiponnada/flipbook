//! Everything Flipbook remembers between runs: one small JSON file.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// `%APPDATA%\Flipbook` on Windows, `~/.local/share/Flipbook` elsewhere.
pub fn data_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Flipbook")
}

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Book {
    /// 0-based page the reader was last on.
    pub page: usize,
    pub pages: usize,
    pub bookmarks: Vec<usize>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub root: Option<PathBuf>,
    /// None = follow the system theme.
    pub dark: Option<bool>,
    /// 0 day, 1 sepia, 2 night.
    pub paper: u8,
    pub single: bool,
    /// Page-turn style, an index into TURNS in main.rs. 0 = curl.
    pub turn: u8,
    /// Newest first.
    pub recent: Vec<PathBuf>,
    pub books: HashMap<String, Book>,
}

impl Store {
    pub fn load() -> Self {
        std::fs::read(data_dir().join("state.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// Write to a temp file and rename, so a crash mid-write can't eat the reading progress.
    pub fn save(&self) {
        let dir = data_dir();
        let tmp = dir.join("state.json.tmp");
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(json) = serde_json::to_vec(self) {
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, dir.join("state.json"));
            }
        }
    }

    pub fn book(&self, path: &Path) -> Book {
        self.books.get(&*path.to_string_lossy()).cloned().unwrap_or_default()
    }

    pub fn book_mut(&mut self, path: &Path) -> &mut Book {
        self.books.entry(path.to_string_lossy().into_owned()).or_default()
    }
}
