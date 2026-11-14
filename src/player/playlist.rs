use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct PlaylistItem {
    pub path: PathBuf,
    pub title: String,
    #[allow(dead_code)]
    pub duration: Option<Duration>,
}

impl PlaylistItem {
    pub fn from_path(path: PathBuf) -> Self {
        let title = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());
        Self {
            path,
            title,
            duration: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Playlist {
    pub items: Vec<PlaylistItem>,
    pub current_index: Option<usize>,
    pub auto_next: bool,
    #[allow(dead_code)]
    pub search_query: String,
}

impl Default for Playlist {
    fn default() -> Self {
        Self::new()
    }
}

impl Playlist {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            current_index: None,
            auto_next: true,
            search_query: String::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn current_item(&self) -> Option<&PlaylistItem> {
        self.current_index.and_then(|idx| self.items.get(idx))
    }

    pub fn set_current(&mut self, index: usize) -> Option<&PlaylistItem> {
        if index < self.items.len() {
            self.current_index = Some(index);
            self.items.get(index)
        } else {
            None
        }
    }

    pub fn next(&mut self) -> Option<&PlaylistItem> {
        if self.items.is_empty() {
            return None;
        }
        let next_idx = match self.current_index {
            Some(idx) if idx + 1 < self.items.len() => idx + 1,
            _ => return None,
        };
        self.current_index = Some(next_idx);
        self.items.get(next_idx)
    }

    pub fn prev(&mut self) -> Option<&PlaylistItem> {
        if self.items.is_empty() {
            return None;
        }
        let prev_idx = match self.current_index {
            Some(idx) if idx > 0 => idx - 1,
            _ => return None,
        };
        self.current_index = Some(prev_idx);
        self.items.get(prev_idx)
    }

    pub fn has_next(&self) -> bool {
        match self.current_index {
            Some(idx) => idx + 1 < self.items.len(),
            None => !self.items.is_empty(),
        }
    }

    #[allow(dead_code)]
    pub fn has_prev(&self) -> bool {
        match self.current_index {
            Some(idx) => idx > 0,
            None => false,
        }
    }

    pub fn add_item(&mut self, path: PathBuf) {
        self.items.push(PlaylistItem::from_path(path));
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.current_index = None;
    }

    pub fn load_folder(&mut self, folder: &Path) -> usize {
        self.clear();
        let mut found_files = Vec::new();
        Self::scan_directory(folder, &mut found_files);

        // Natural human sort so "Episode 2" precedes "Episode 10"
        found_files.sort_by(|a, b| {
            let name_a = a.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
            let name_b = b.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
            natural_cmp(&name_a, &name_b)
        });

        for file in found_files {
            self.add_item(file);
        }

        if !self.items.is_empty() {
            self.current_index = Some(0);
        }

        self.items.len()
    }

    #[allow(dead_code)]
    pub fn add_files(&mut self, files: Vec<PathBuf>) -> usize {
        let mut sorted = files;
        sorted.sort_by(|a, b| {
            let name_a = a.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
            let name_b = b.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
            natural_cmp(&name_a, &name_b)
        });

        let start_len = self.items.len();
        for file in sorted {
            self.add_item(file);
        }
        if self.current_index.is_none() && !self.items.is_empty() {
            self.current_index = Some(0);
        }
        self.items.len() - start_len
    }

    fn scan_directory(dir: &Path, results: &mut Vec<PathBuf>) {
        let video_extensions = ["mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "ts"];
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                        if video_extensions.contains(&ext.to_lowercase().as_str()) {
                            results.push(path);
                        }
                    }
                } else if path.is_dir() {
                    Self::scan_directory(&path, results);
                }
            }
        }
    }
}

/// Natural human sorting: "Ep 2" comes before "Ep 10"
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let mut chars_a = a.chars().peekable();
    let mut chars_b = b.chars().peekable();

    loop {
        match (chars_a.peek(), chars_b.peek()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(ca), Some(cb)) => {
                if ca.is_ascii_digit() && cb.is_ascii_digit() {
                    let mut num_a_str = String::new();
                    while let Some(&c) = chars_a.peek() {
                        if c.is_ascii_digit() {
                            num_a_str.push(c);
                            chars_a.next();
                        } else {
                            break;
                        }
                    }
                    let mut num_b_str = String::new();
                    while let Some(&c) = chars_b.peek() {
                        if c.is_ascii_digit() {
                            num_b_str.push(c);
                            chars_b.next();
                        } else {
                            break;
                        }
                    }
                    let num_a: u128 = num_a_str.parse().unwrap_or(0);
                    let num_b: u128 = num_b_str.parse().unwrap_or(0);
                    match num_a.cmp(&num_b) {
                        std::cmp::Ordering::Equal => continue,
                        ord => return ord,
                    }
                } else {
                    let ca_lower = ca.to_lowercase().next().unwrap_or(*ca);
                    let cb_lower = cb.to_lowercase().next().unwrap_or(*cb);
                    match ca_lower.cmp(&cb_lower) {
                        std::cmp::Ordering::Equal => {
                            chars_a.next();
                            chars_b.next();
                        }
                        ord => return ord,
                    }
                }
            }
        }
    }
}
