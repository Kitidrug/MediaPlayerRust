use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use url::Url;

use crate::player::backend::{PlaybackState, PlayerBackend, PlayerEvent, VideoFrame};
use crate::player::playlist::Playlist;
use crate::player::tracks::{AudioTrack, SubtitleTrack};

pub struct PlayerController {
    backend: PlayerBackend,
    pub state: PlaybackState,
    pub duration: Option<Duration>,
    pub position: Duration,
    pub volume: f64,
    pub is_muted: bool,
    pub last_non_mute_volume: f64,
    pub rate: f64,
    pub audio_tracks: Vec<AudioTrack>,
    pub current_audio: i32,
    pub subtitle_tracks: Vec<SubtitleTrack>,
    pub current_subtitle: i32,
    #[allow(dead_code)]
    pub connection_speed_kbps: u64, // 0 = auto
    pub playlist: Playlist,

    // File and source tracking for session persistence
    pub current_file_path: Option<PathBuf>,
    pub current_url: Option<String>,
    pub season_folder_path: Option<PathBuf>,

    // Debouncing for smooth rapid track switching
    pending_audio_track: Option<(i32, Instant)>,
    pending_subtitle_track: Option<(i32, Instant)>,
}

impl PlayerController {
    pub fn new() -> Result<Self> {
        let backend = PlayerBackend::new()?;
        Ok(Self {
            backend,
            state: PlaybackState::Stopped,
            duration: None,
            position: Duration::ZERO,
            volume: 1.0,
            is_muted: false,
            last_non_mute_volume: 1.0,
            rate: 1.0,
            audio_tracks: Vec::new(),
            current_audio: -1,
            subtitle_tracks: vec![SubtitleTrack {
                id: -1,
                language: String::new(),
                language_name: String::new(),
                title: "Отключены (Off)".to_string(),
            }],
            current_subtitle: -1,
            connection_speed_kbps: 0,
            playlist: Playlist::new(),
            current_file_path: None,
            current_url: None,
            season_folder_path: None,
            pending_audio_track: None,
            pending_subtitle_track: None,
        })
    }

    pub fn open_file(&mut self, path: &Path) -> Result<()> {
        let url = Url::from_file_path(path)
            .map_err(|_| anyhow!("Failed to convert file path to URI: {:?}", path))?;
        self.pending_audio_track = None;
        self.pending_subtitle_track = None;
        self.current_file_path = Some(path.to_path_buf());
        self.current_url = None;
        self.backend.set_uri(url.as_str())
    }

    pub fn open_season(&mut self, folder: &Path) -> Result<usize> {
        self.season_folder_path = Some(folder.to_path_buf());
        let count = self.playlist.load_folder(folder);
        if count > 0 {
            self.play_episode(0)?;
        }
        Ok(count)
    }

    #[allow(dead_code)]
    pub fn add_season_files(&mut self, files: Vec<PathBuf>) -> Result<usize> {
        let count = self.playlist.add_files(files);
        if self.playlist.current_index.is_some() && self.state == PlaybackState::Stopped {
            self.play_episode(0)?;
        }
        Ok(count)
    }

    pub fn play_episode(&mut self, index: usize) -> Result<()> {
        if let Some(item) = self.playlist.set_current(index).cloned() {
            self.open_file(&item.path)?;
            self.play()?;
            Ok(())
        } else {
            Err(anyhow!("Invalid episode index {}", index))
        }
    }

    pub fn next_episode(&mut self) -> Result<bool> {
        if let Some(item) = self.playlist.next().cloned() {
            self.open_file(&item.path)?;
            self.play()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn prev_episode(&mut self) -> Result<bool> {
        if let Some(item) = self.playlist.prev().cloned() {
            self.open_file(&item.path)?;
            self.play()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn open_url(&mut self, url_str: &str) -> Result<()> {
        let url = Url::parse(url_str).context("Invalid network URL")?;
        if url.scheme().is_empty() {
            return Err(anyhow!("URL is missing protocol scheme (http, https, rtsp, etc.)"));
        }
        self.pending_audio_track = None;
        self.pending_subtitle_track = None;
        self.current_url = Some(url_str.to_string());
        self.current_file_path = None;
        self.backend.set_uri(url.as_str())
    }

    pub fn load_external_subtitle(&mut self, path: &Path) -> Result<()> {
        let url = Url::from_file_path(path)
            .map_err(|_| anyhow!("Failed to convert subtitle path to URI: {:?}", path))?;
        self.backend.set_subtitle_uri(url.as_str());
        Ok(())
    }

    pub fn play(&mut self) -> Result<()> {
        self.backend.play()
    }

    pub fn pause(&mut self) -> Result<()> {
        self.backend.pause()
    }

    pub fn toggle_play_pause(&mut self) -> Result<()> {
        match self.state {
            PlaybackState::Playing => self.pause(),
            _ => self.play(),
        }
    }

    #[allow(dead_code)]
    pub fn stop(&mut self) -> Result<()> {
        self.backend.stop()?;
        self.position = Duration::ZERO;
        self.state = PlaybackState::Stopped;
        self.pending_audio_track = None;
        self.pending_subtitle_track = None;
        Ok(())
    }

    pub fn seek(&mut self, position: Duration) -> Result<()> {
        self.position = position;
        self.backend.seek(position)
    }

    pub fn seek_relative(&mut self, delta_secs: f64) -> Result<()> {
        let current = self.position.as_secs_f64();
        let duration = self.duration.map(|d| d.as_secs_f64()).unwrap_or(f64::MAX);
        let target = (current + delta_secs).clamp(0.0, duration);
        self.seek(Duration::from_secs_f64(target))
    }

    pub fn set_volume(&mut self, volume: f64) {
        let vol = volume.clamp(0.0, 1.0);
        self.volume = vol;
        if vol > 0.0 {
            self.last_non_mute_volume = vol;
            if self.is_muted {
                self.is_muted = false;
                self.backend.set_mute(false);
            }
        }
        self.backend.set_volume(vol);
    }

    pub fn toggle_mute(&mut self) {
        self.is_muted = !self.is_muted;
        self.backend.set_mute(self.is_muted);
    }

    pub fn set_rate(&mut self, rate: f64) -> Result<()> {
        self.rate = rate;
        self.backend.set_rate(rate)
    }

    pub fn set_audio_track(&mut self, track_id: i32) {
        self.current_audio = track_id;
        // Debounce GStreamer pipeline reconfiguration by 100ms
        self.pending_audio_track = Some((track_id, Instant::now() + Duration::from_millis(100)));
    }

    pub fn cycle_audio_track(&mut self) -> Option<String> {
        if self.audio_tracks.is_empty() {
            return None;
        }
        let current = self.current_audio;
        let next_idx = if current + 1 < self.audio_tracks.len() as i32 {
            current + 1
        } else {
            0
        };
        self.set_audio_track(next_idx);
        self.audio_tracks
            .iter()
            .find(|t| t.id == next_idx)
            .map(|t| t.short_name())
    }

    pub fn set_subtitle_track(&mut self, track_id: i32) {
        self.current_subtitle = track_id;
        self.pending_subtitle_track = Some((track_id, Instant::now() + Duration::from_millis(100)));
    }

    pub fn cycle_subtitle_track(&mut self) -> Option<String> {
        if self.subtitle_tracks.is_empty() {
            return None;
        }
        let current_pos = self
            .subtitle_tracks
            .iter()
            .position(|t| t.id == self.current_subtitle)
            .unwrap_or(0);
        let next_pos = (current_pos + 1) % self.subtitle_tracks.len();
        let next_id = self.subtitle_tracks[next_pos].id;
        self.set_subtitle_track(next_id);
        Some(self.subtitle_tracks[next_pos].short_name())
    }

    #[allow(dead_code)]
    pub fn set_connection_speed(&mut self, kbps: u64) {
        self.connection_speed_kbps = kbps;
        self.backend.set_connection_speed(kbps);
    }

    pub fn poll_events(&mut self) -> Vec<PlayerEvent> {
        // Dispatch debounced audio track switch if time has arrived
        if let Some((track_id, due_time)) = self.pending_audio_track {
            if Instant::now() >= due_time {
                self.backend.set_audio_track(track_id);
                self.pending_audio_track = None;
            }
        }

        // Dispatch debounced subtitle track switch if time has arrived
        if let Some((track_id, due_time)) = self.pending_subtitle_track {
            if Instant::now() >= due_time {
                self.backend.set_subtitle_track(track_id);
                self.pending_subtitle_track = None;
            }
        }

        let mut events = Vec::new();
        while let Ok(event) = self.backend.event_receiver().try_recv() {
            match &event {
                PlayerEvent::StateChanged(state) => self.state = *state,
                PlayerEvent::DurationChanged(duration) => self.duration = *duration,
                PlayerEvent::TracksUpdated {
                    audio_tracks,
                    current_audio,
                    subtitle_tracks,
                    current_subtitle,
                } => {
                    self.audio_tracks = audio_tracks.clone();
                    self.subtitle_tracks = subtitle_tracks.clone();
                    // Only update current_audio from backend if no user switch is pending
                    if self.pending_audio_track.is_none() {
                        self.current_audio = *current_audio;
                    }
                    if self.pending_subtitle_track.is_none() {
                        self.current_subtitle = *current_subtitle;
                    }
                }
                PlayerEvent::Error(..) => self.state = PlaybackState::Error,
            }
            events.push(event);
        }

        if let Some(pos) = self.backend.position() {
            self.position = pos;
        }

        events
    }

    pub fn try_recv_frame(&self) -> Option<VideoFrame> {
        self.backend.try_recv_frame()
    }

    pub fn create_session(&self) -> Option<crate::player::session::PlaybackSession> {
        let title = if let Some(item) = self.playlist.current_item() {
            crate::player::tracks::clean_title(&item.title)
        } else if let Some(ref path) = self.current_file_path {
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            crate::player::tracks::clean_title(&name)
        } else if let Some(ref url) = self.current_url {
            url.clone()
        } else {
            return None;
        };

        let pos_secs = self.position.as_secs_f64();
        if pos_secs < 3.0 {
            return None;
        }

        Some(crate::player::session::PlaybackSession {
            file_path: self.current_file_path.as_ref().map(|p| p.to_string_lossy().to_string()),
            url: self.current_url.clone(),
            clean_title: title,
            position_secs: pos_secs,
            duration_secs: self.duration.map(|d| d.as_secs_f64()),
            audio_track: if self.current_audio >= 0 { Some(self.current_audio) } else { None },
            subtitle_track: if self.current_subtitle >= 0 { Some(self.current_subtitle) } else { None },
            season_folder: self.season_folder_path.as_ref().map(|p| p.to_string_lossy().to_string()),
            episode_index: self.playlist.current_index,
            saved_at_epoch: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        })
    }
}
