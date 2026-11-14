use std::fs;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackSession {
    pub file_path: Option<String>,
    pub url: Option<String>,
    pub clean_title: String,
    pub position_secs: f64,
    pub duration_secs: Option<f64>,
    pub audio_track: Option<i32>,
    pub subtitle_track: Option<i32>,
    pub season_folder: Option<String>,
    pub episode_index: Option<usize>,
    pub saved_at_epoch: u64,
}

impl PlaybackSession {
    fn storage_path() -> PathBuf {
        if let Ok(appdata) = std::env::var("APPDATA") {
            let dir = Path::new(&appdata).join("MediaPlayer");
            let _ = fs::create_dir_all(&dir);
            dir.join("last_session.json")
        } else {
            PathBuf::from(".media_player_session.json")
        }
    }

    pub fn save(&self) {
        let path = Self::storage_path();
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(path, json);
        }
    }

    pub fn load() -> Option<Self> {
        let path = Self::storage_path();
        let content = fs::read_to_string(path).ok()?;
        let session: Self = serde_json::from_str(&content).ok()?;

        // Verify that local file still exists
        if let Some(ref file_path) = session.file_path {
            if !Path::new(file_path).exists() {
                return None;
            }
        }

        // Only offer resume if position is meaningfully into the video (> 4 seconds)
        // and not right at the end (< duration - 10s)
        if session.position_secs < 4.0 {
            return None;
        }
        if let Some(dur) = session.duration_secs {
            if dur > 15.0 && session.position_secs >= (dur - 10.0) {
                return None;
            }
        }

        Some(session)
    }

    pub fn clear() {
        let path = Self::storage_path();
        let _ = fs::remove_file(path);
    }
}
