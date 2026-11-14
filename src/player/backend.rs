use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use crossbeam_channel::{Receiver, Sender};
use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;

use crate::player::tracks::{AudioTrack, SubtitleTrack};

pub struct VideoFrame {
    pub width: usize,
    pub height: usize,
    pub image: Arc<egui::ColorImage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
    Buffering(u32),
    Ended,
    Error,
}

#[derive(Debug, Clone)]
pub enum PlayerEvent {
    StateChanged(PlaybackState),
    DurationChanged(Option<Duration>),
    TracksUpdated {
        audio_tracks: Vec<AudioTrack>,
        current_audio: i32,
        subtitle_tracks: Vec<SubtitleTrack>,
        current_subtitle: i32,
    },
    Error(String),
}

pub struct PlayerBackend {
    pipeline: gst::Element,
    event_rx: Receiver<PlayerEvent>,
    latest_frame: Arc<std::sync::Mutex<Option<VideoFrame>>>,
    is_running: Arc<AtomicBool>,
    last_track_switch: Arc<std::sync::Mutex<Instant>>,
}

impl PlayerBackend {
    pub fn new() -> Result<Self> {
        gst::init().context("Failed to initialize GStreamer")?;

        // Instantiate playbin (battle-tested for flawless audio track switching) with fallback to playbin3
        let pipeline = gst::ElementFactory::make("playbin")
            .build()
            .or_else(|_| gst::ElementFactory::make("playbin3").build())
            .context("Failed to create playbin element")?;

        // Setup AppSink to receive decoded RGBA video frames
        let latest_frame = Arc::new(std::sync::Mutex::new(None));
        let frame_slot_preroll = latest_frame.clone();
        let frame_slot_sample = latest_frame.clone();

        let appsink = gst_app::AppSink::builder()
            .caps(
                &gst_video::VideoCapsBuilder::new()
                    .format(gst_video::VideoFormat::Rgba)
                    .build(),
            )
            .max_buffers(1)
            .drop(true)
            .sync(true)
            .qos(true)
            .build();

        let process_sample = |sample: gst::Sample, slot: &Arc<std::sync::Mutex<Option<VideoFrame>>>| -> Result<(), gst::FlowError> {
            let buffer = sample.buffer().ok_or(gst::FlowError::Error)?;
            let caps = sample.caps().ok_or(gst::FlowError::Error)?;
            let video_info =
                gst_video::VideoInfo::from_caps(caps).map_err(|_| gst::FlowError::Error)?;
            let map = buffer.map_readable().map_err(|_| gst::FlowError::Error)?;

            let width = video_info.width() as usize;
            let height = video_info.height() as usize;

            // Ultra-fast zero-copy memory cast via bytemuck:
            let pixels = bytemuck::cast_slice::<u8, egui::Color32>(map.as_slice()).to_vec();
            let color_image = egui::ColorImage {
                size: [width, height],
                pixels,
            };

            let frame = VideoFrame {
                width,
                height,
                image: Arc::new(color_image),
            };

            if let Ok(mut s) = slot.lock() {
                *s = Some(frame);
            }
            Ok(())
        };

        appsink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_preroll(move |sink| {
                    // CRITICAL: Pull preroll buffers immediately so pipeline state changes and
                    // track-switching never deadlock waiting for sink preroll
                    let sample = sink.pull_preroll().map_err(|_| gst::FlowError::Error)?;
                    let _ = process_sample(sample, &frame_slot_preroll);
                    Ok(gst::FlowSuccess::Ok)
                })
                .new_sample(move |sink| {
                    let sample = sink.pull_sample().map_err(|_| gst::FlowError::Error)?;
                    let _ = process_sample(sample, &frame_slot_sample);
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );

        pipeline.set_property("video-sink", &appsink);

        // Resilient audio sink with automatic format/channel conversion to prevent audio driver stalls
        if let Ok(audio_sink) = gst::parse::bin_from_description(
            "audioconvert ! audioresample ! autoaudiosink",
            true,
        ) {
            pipeline.set_property("audio-sink", &audio_sink);
        }

        // Configure pipeline buffer duration to prevent audio dropouts during disk/network I/O
        let _ = pipeline.set_property("buffer-duration", 300_000_000_i64);

        // Setup Bus message monitoring
        let (event_tx, event_rx) = crossbeam_channel::unbounded::<PlayerEvent>();
        let bus = pipeline.bus().context("Pipeline without bus")?;
        let is_running = Arc::new(AtomicBool::new(true));
        let is_running_clone = is_running.clone();
        let pipeline_weak = pipeline.downgrade();

        let last_track_switch = Arc::new(std::sync::Mutex::new(Instant::now()));
        let last_track_switch_clone = last_track_switch.clone();

        std::thread::Builder::new()
            .name("gst-bus-listener".to_string())
            .spawn(move || {
                while is_running_clone.load(Ordering::Relaxed) {
                    if let Some(msg) = bus.timed_pop(gst::ClockTime::from_mseconds(100)) {
                        Self::handle_bus_message(&msg, &pipeline_weak, &event_tx, &last_track_switch_clone);
                    }
                }
            })
            .context("Failed to spawn bus listener thread")?;

        Ok(Self {
            pipeline,
            event_rx,
            latest_frame,
            is_running,
            last_track_switch,
        })
    }

    fn handle_bus_message(
        msg: &gst::Message,
        pipeline_weak: &glib::WeakRef<gst::Element>,
        event_tx: &Sender<PlayerEvent>,
        last_track_switch: &Arc<std::sync::Mutex<Instant>>,
    ) {
        use gst::MessageView;

        match msg.view() {
            MessageView::Eos(..) => {
                let _ = event_tx.send(PlayerEvent::StateChanged(PlaybackState::Ended));
            }
            MessageView::Error(err) => {
                let error_debug = format!("{:?}", err.debug());

                let is_recent_switch = {
                    if let Ok(last) = last_track_switch.lock() {
                        last.elapsed() < Duration::from_millis(1500)
                    } else {
                        false
                    }
                };

                let is_transient_stream_error = error_debug.contains("Internal data stream error")
                    || error_debug.contains("streaming stopped, reason error")
                    || error_debug.contains("flushing")
                    || error_debug.contains("not-negotiated")
                    || error_debug.contains("not-linked");

                if is_recent_switch && is_transient_stream_error {
                    // Suppress transient stream unlink/relink glitches during rapid track switching
                    // Resync on a detached background thread to prevent deadlocking the bus listener loop
                    if let Some(pipeline) = pipeline_weak.upgrade() {
                        std::thread::spawn(move || {
                            std::thread::sleep(Duration::from_millis(60));
                            let _ = pipeline.set_state(gst::State::Paused);
                            let _ = pipeline.set_state(gst::State::Playing);
                        });
                    }
                } else {
                    let error_msg = format!("{}: {:?}", err.error(), err.debug());
                    let _ = event_tx.send(PlayerEvent::Error(error_msg));
                    let _ = event_tx.send(PlayerEvent::StateChanged(PlaybackState::Error));
                }
            }
            MessageView::Buffering(buffering) => {
                let percent = buffering.percent();
                if percent < 100 {
                    let _ = event_tx.send(PlayerEvent::StateChanged(PlaybackState::Buffering(
                        percent as u32,
                    )));
                } else {
                    let _ = event_tx.send(PlayerEvent::StateChanged(PlaybackState::Playing));
                }
            }
            MessageView::StateChanged(state_changed) => {
                if let Some(pipeline) = pipeline_weak.upgrade() {
                    if msg.src().as_deref() == Some(pipeline.upcast_ref::<gst::Object>()) {
                        let current_state = state_changed.current();
                        let state = match current_state {
                            gst::State::Playing => PlaybackState::Playing,
                            gst::State::Paused => PlaybackState::Paused,
                            gst::State::Ready | gst::State::Null => PlaybackState::Stopped,
                            _ => PlaybackState::Stopped,
                        };
                        let _ = event_tx.send(PlayerEvent::StateChanged(state));

                        if current_state == gst::State::Playing || current_state == gst::State::Paused {
                            Self::extract_tracks(&pipeline, event_tx);
                        }
                    }
                }
            }
            MessageView::DurationChanged(..) => {
                if let Some(pipeline) = pipeline_weak.upgrade() {
                    let duration = pipeline
                        .query_duration::<gst::ClockTime>()
                        .map(|d| Duration::from_nanos(d.nseconds()));
                    let _ = event_tx.send(PlayerEvent::DurationChanged(duration));
                }
            }
            MessageView::AsyncDone(..) => {
                if let Some(pipeline) = pipeline_weak.upgrade() {
                    Self::extract_tracks(&pipeline, event_tx);
                    let duration = pipeline
                        .query_duration::<gst::ClockTime>()
                        .map(|d| Duration::from_nanos(d.nseconds()));
                    let _ = event_tx.send(PlayerEvent::DurationChanged(duration));
                }
            }
            MessageView::Tag(..) => {
                // Keep tag handling lightweight so audio playback stream is never interrupted
                if let Some(pipeline) = pipeline_weak.upgrade() {
                    let duration = pipeline
                        .query_duration::<gst::ClockTime>()
                        .map(|d| Duration::from_nanos(d.nseconds()));
                    if duration.is_some() {
                        let _ = event_tx.send(PlayerEvent::DurationChanged(duration));
                    }
                }
            }
            _ => (),
        }
    }

    fn extract_tracks(pipeline: &gst::Element, event_tx: &Sender<PlayerEvent>) {
        let n_audio = pipeline.property::<i32>("n-audio");
        let current_audio = pipeline.property::<i32>("current-audio");
        let mut audio_tracks = Vec::new();

        for i in 0..n_audio {
            let mut lang = String::new();
            let mut lang_name = String::new();
            let mut codec = String::new();
            let mut title = String::new();
            let mut bitrate = None;
            let mut channels = None;
            let mut sample_rate = None;

            // Extract tags
            if let Some(tags) =
                pipeline.emit_by_name::<Option<gst::TagList>>("get-audio-tags", &[&i])
            {
                if let Some(t) = tags.get::<gst::tags::Title>() {
                    title = t.get().to_string();
                } else if let Some(d) = tags.get::<gst::tags::Description>() {
                    title = d.get().to_string();
                }

                if let Some(l) = tags.get::<gst::tags::LanguageCode>() {
                    lang = l.get().to_string();
                }
                if let Some(ln) = tags.get::<gst::tags::LanguageName>() {
                    lang_name = ln.get().to_string();
                }
                if let Some(c) = tags.get::<gst::tags::AudioCodec>() {
                    codec = c.get().to_string();
                }
                if let Some(br) = tags.get::<gst::tags::NominalBitrate>() {
                    bitrate = Some(br.get());
                } else if let Some(br) = tags.get::<gst::tags::Bitrate>() {
                    bitrate = Some(br.get());
                }
            }

            // Extract pad caps for audio stream properties (channels, rate)
            if let Some(pad) = pipeline.emit_by_name::<Option<gst::Pad>>("get-audio-pad", &[&i]) {
                if let Some(caps) = pad.current_caps() {
                    if let Some(structure) = caps.structure(0) {
                        if let Ok(ch) = structure.get::<i32>("channels") {
                            channels = Some(ch);
                        }
                        if let Ok(rate) = structure.get::<i32>("rate") {
                            sample_rate = Some(rate);
                        }
                    }
                }
            }

            audio_tracks.push(AudioTrack {
                id: i,
                language: lang,
                language_name: lang_name,
                codec,
                title,
                channels,
                sample_rate,
                bitrate,
            });
        }

        let n_text = pipeline.property::<i32>("n-text");
        let current_text = pipeline.property::<i32>("current-text");
        let mut subtitle_tracks = Vec::new();

        // Always provide an "Off" entry
        subtitle_tracks.push(SubtitleTrack {
            id: -1,
            language: String::new(),
            language_name: String::new(),
            title: "Отключены (Off)".to_string(),
        });

        for i in 0..n_text {
            let mut lang = String::new();
            let mut lang_name = String::new();
            let mut title = String::new();

            if let Some(tags) =
                pipeline.emit_by_name::<Option<gst::TagList>>("get-text-tags", &[&i])
            {
                if let Some(t) = tags.get::<gst::tags::Title>() {
                    title = t.get().to_string();
                } else if let Some(d) = tags.get::<gst::tags::Description>() {
                    title = d.get().to_string();
                }

                if let Some(l) = tags.get::<gst::tags::LanguageCode>() {
                    lang = l.get().to_string();
                }
                if let Some(ln) = tags.get::<gst::tags::LanguageName>() {
                    lang_name = ln.get().to_string();
                }
            }

            subtitle_tracks.push(SubtitleTrack {
                id: i,
                language: lang,
                language_name: lang_name,
                title,
            });
        }

        let _ = event_tx.send(PlayerEvent::TracksUpdated {
            audio_tracks,
            current_audio,
            subtitle_tracks,
            current_subtitle: current_text,
        });
    }

    // --- Control commands ---

    pub fn set_uri(&self, uri: &str) -> Result<()> {
        self.stop()?;
        self.pipeline.set_property("uri", uri);
        self.play()
    }

    pub fn set_subtitle_uri(&self, sub_uri: &str) {
        self.pipeline.set_property("suburi", sub_uri);
    }

    pub fn play(&self) -> Result<()> {
        self.pipeline
            .set_state(gst::State::Playing)
            .map_err(|_| anyhow!("Failed to set pipeline to Playing"))?;
        Ok(())
    }

    pub fn pause(&self) -> Result<()> {
        self.pipeline
            .set_state(gst::State::Paused)
            .map_err(|_| anyhow!("Failed to set pipeline to Paused"))?;
        Ok(())
    }

    pub fn stop(&self) -> Result<()> {
        self.pipeline
            .set_state(gst::State::Null)
            .map_err(|_| anyhow!("Failed to set pipeline to Null"))?;
        Ok(())
    }

    pub fn seek(&self, position: Duration) -> Result<()> {
        let clock_time = gst::ClockTime::from_nseconds(position.as_nanos() as u64);
        self.pipeline
            .seek_simple(gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT, clock_time)
            .map_err(|_| anyhow!("Failed to seek to position"))?;
        Ok(())
    }

    pub fn set_rate(&self, rate: f64) -> Result<()> {
        let position = self
            .pipeline
            .query_position::<gst::ClockTime>()
            .unwrap_or(gst::ClockTime::ZERO);

        let (start, stop) = if rate > 0.0 {
            (position, gst::ClockTime::NONE)
        } else {
            (gst::ClockTime::ZERO, Some(position))
        };

        // GStreamer's playbin automatically activates scaletempo to keep audio pitch
        self.pipeline
            .seek(
                rate,
                gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                gst::SeekType::Set,
                start,
                gst::SeekType::None,
                stop,
            )
            .map_err(|_| anyhow!("Failed to change playback speed to {}", rate))?;
        Ok(())
    }

    pub fn set_volume(&self, volume: f64) {
        self.pipeline.set_property("volume", volume.clamp(0.0, 1.0));
    }

    pub fn set_mute(&self, mute: bool) {
        self.pipeline.set_property("mute", mute);
    }

    pub fn set_audio_track(&self, index: i32) {
        if let Ok(mut last) = self.last_track_switch.lock() {
            *last = Instant::now();
        }
        let n_audio = self.pipeline.property::<i32>("n-audio");
        if index >= 0 && index < n_audio {
            let pos = self.pipeline.query_position::<gst::ClockTime>().unwrap_or(gst::ClockTime::ZERO);
            self.pipeline.set_property("current-audio", index);
            // Instant flush seek to current position realigns the demuxer queues and stream synchronizer,
            // preventing the pipeline from stalling waiting for unbuffered audio timestamps.
            let _ = self.pipeline.seek_simple(
                gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
                pos,
            );
        }
    }

    pub fn set_subtitle_track(&self, index: i32) {
        if let Ok(mut last) = self.last_track_switch.lock() {
            *last = Instant::now();
        }
        let n_text = self.pipeline.property::<i32>("n-text");
        if index < 0 || index < n_text {
            let pos = self.pipeline.query_position::<gst::ClockTime>().unwrap_or(gst::ClockTime::ZERO);
            self.pipeline.set_property("current-text", index);
            let _ = self.pipeline.seek_simple(
                gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
                pos,
            );
        }
    }

    #[allow(dead_code)]
    pub fn set_connection_speed(&self, kbps: u64) {
        self.pipeline.set_property("connection-speed", kbps);
    }

    pub fn position(&self) -> Option<Duration> {
        self.pipeline
            .query_position::<gst::ClockTime>()
            .map(|t| Duration::from_nanos(t.nseconds()))
    }

    #[allow(dead_code)]
    pub fn duration(&self) -> Option<Duration> {
        self.pipeline
            .query_duration::<gst::ClockTime>()
            .map(|t| Duration::from_nanos(t.nseconds()))
    }

    pub fn event_receiver(&self) -> &Receiver<PlayerEvent> {
        &self.event_rx
    }

    pub fn try_recv_frame(&self) -> Option<VideoFrame> {
        if let Ok(mut slot) = self.latest_frame.lock() {
            slot.take()
        } else {
            None
        }
    }
}

impl Drop for PlayerBackend {
    fn drop(&mut self) {
        self.is_running.store(false, Ordering::Relaxed);
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}
