use std::time::{Duration, Instant};

use eframe::egui;
use egui::{
    Color32, Context, FontId, Key, Pos2, Rect, RichText, Rounding, Sense, Stroke, Vec2,
    ViewportCommand,
};

use crate::player::backend::PlaybackState;
use crate::player::session::PlaybackSession;
use crate::player::PlayerController;
use crate::ui::timeline::VideoTimeline;
use crate::ui::url_dialog::UrlDialog;

#[derive(PartialEq, Eq)]
enum SettingsSubmenu {
    Main,
    Quality,
    Speed,
    Subtitles,
}

pub struct MediaPlayerApp {
    player: PlayerController,
    video_texture: Option<egui::TextureHandle>,
    video_dimensions: [f32; 2],

    // UI state
    url_dialog: UrlDialog,
    is_fullscreen: bool,
    is_seeking: bool,
    seek_slider_value: f64,
    show_remaining_time: bool,

    // Settings popup (Screenshot 2)
    show_settings_popup: bool,
    settings_submenu: SettingsSubmenu,

    // OSD feedback
    osd_text: Option<String>,
    osd_expires_at: Option<Instant>,

    // Fullscreen auto-hide & smooth fading
    last_mouse_activity: Instant,
    last_cursor_pos: Option<Pos2>,
    controls_opacity: f32,

    // Session persistence & resume prompt
    resume_session: Option<PlaybackSession>,
    last_session_save: Instant,
}

impl MediaPlayerApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Result<Self, anyhow::Error> {
        let player = PlayerController::new()?;
        let resume_session = PlaybackSession::load();

        Ok(Self {
            player,
            video_texture: None,
            video_dimensions: [0.0, 0.0],
            url_dialog: UrlDialog::default(),
            is_fullscreen: false,
            is_seeking: false,
            seek_slider_value: 0.0,
            show_remaining_time: false,
            show_settings_popup: false,
            settings_submenu: SettingsSubmenu::Main,
            osd_text: None,
            osd_expires_at: None,
            last_mouse_activity: Instant::now(),
            last_cursor_pos: None,
            controls_opacity: 1.0,
            resume_session,
            last_session_save: Instant::now(),
        })
    }

    fn show_osd(&mut self, text: impl Into<String>) {
        let t = text.into();
        let truncated = crate::player::tracks::truncate_ellipsis(&t, 46);
        self.osd_text = Some(truncated);
        self.osd_expires_at = Some(Instant::now() + Duration::from_millis(1600));
    }

    fn toggle_fullscreen(&mut self, ctx: &Context) {
        self.is_fullscreen = !self.is_fullscreen;
        ctx.send_viewport_cmd(ViewportCommand::Fullscreen(self.is_fullscreen));
        self.last_mouse_activity = Instant::now();
        if !self.is_fullscreen {
            self.controls_opacity = 1.0;
            ctx.set_cursor_icon(egui::CursorIcon::Default);
        }
    }

    fn resume_saved_session(&mut self) {
        if let Some(session) = self.resume_session.take() {
            let mut opened = false;
            if let Some(ref folder_str) = session.season_folder {
                let folder = std::path::Path::new(folder_str);
                if folder.exists() {
                    if let Ok(_) = self.player.open_season(folder) {
                        let ep = session.episode_index.unwrap_or(0);
                        let _ = self.player.play_episode(ep);
                        opened = true;
                    }
                }
            }
            if !opened {
                if let Some(ref file_path) = session.file_path {
                    let p = std::path::Path::new(file_path);
                    if p.exists() {
                        if let Ok(_) = self.player.open_file(p) {
                            opened = true;
                        }
                    }
                } else if let Some(ref url) = session.url {
                    if let Ok(_) = self.player.open_url(url) {
                        opened = true;
                    }
                }
            }
            if opened {
                let seek_pos = Duration::from_secs_f64(session.position_secs);
                let _ = self.player.seek(seek_pos);
                if let Some(audio_id) = session.audio_track {
                    self.player.set_audio_track(audio_id);
                }
                if let Some(sub_id) = session.subtitle_track {
                    self.player.set_subtitle_track(sub_id);
                }
                let _ = self.player.play();
                self.show_osd(format!(
                    "▶ Продолжаем: {} ({})",
                    session.clean_title,
                    Self::format_duration(seek_pos)
                ));
            } else {
                self.show_osd("Не удалось открыть сохранённый файл");
                PlaybackSession::clear();
            }
        }
    }

    fn open_file_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter(
                "Media Files",
                &[
                    "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "mp3", "flac", "wav", "ts",
                    "m4v",
                ],
            )
            .pick_file()
        {
            self.resume_session = None;
            if let Err(e) = self.player.open_file(&path) {
                self.show_osd(format!("Ошибка: {}", e));
            } else {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let clean = crate::player::tracks::clean_title(&name);
                self.show_osd(format!("▶ {}", clean));
                let _ = self.player.play();
            }
        }
    }

    fn open_season_dialog(&mut self) {
        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
            self.resume_session = None;
            match self.player.open_season(&folder) {
                Ok(count) => {
                    if let Some(item) = self.player.playlist.current_item() {
                        let clean = crate::player::tracks::clean_title(&item.title);
                        self.show_osd(format!("Сезон ({} эп.) • 1: {}", count, clean));
                    }
                }
                Err(e) => {
                    self.show_osd(format!("Ошибка: {}", e));
                }
            }
        }
    }

    fn open_subtitle_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Subtitles", &["srt", "vtt", "ass", "ssa", "sub"])
            .pick_file()
        {
            if let Err(e) = self.player.load_external_subtitle(&path) {
                self.show_osd(format!("Ошибка: {}", e));
            } else {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let clean = crate::player::tracks::clean_title(&name);
                self.show_osd(format!("💬 Субтитры: {}", clean));
            }
        }
    }

    fn render_bottom_controls(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &Context,
        opacity: f32,
        seek_to: &mut Option<Duration>,
        toggle_play_pause: &mut bool,
        gear_button_rect: &mut Option<Rect>,
    ) {
        ui.set_opacity(opacity);

        // Full-width slim timeline at the top of controls
        let timeline = VideoTimeline::new(
            self.player.position,
            self.player.duration,
            &mut self.is_seeking,
            &mut self.seek_slider_value,
        )
        .opacity(opacity);

        let output = timeline.show(ui);
        if let Some(target) = output.seek_requested {
            *seek_to = Some(target);
        }

        ui.add_space(4.0);

        // Controls row below timeline (Screenshot 3)
        ui.horizontal(|ui| {
            // --- LEFT GROUP: Play + Time readout (00:03 / 23:45) ---
            let is_playing = self.player.state == PlaybackState::Playing;
            let play_icon = if is_playing { "⏸" } else { "▶" };

            let play_btn = egui::Button::new(
                RichText::new(play_icon).size(16.0).color(Color32::WHITE),
            )
            .frame(false)
            .min_size(Vec2::new(26.0, 26.0));

            if ui.add(play_btn).clicked() {
                *toggle_play_pause = true;
            }

            ui.add_space(6.0);

            // Time display (Placed cleanly to the right of Play, NEVER overlapping video!)
            let cur_str = Self::format_duration(self.player.position);
            let total_dur = self.player.duration.unwrap_or(Duration::ZERO);
            let dur_str = if self.show_remaining_time && total_dur > self.player.position {
                format!("-{}", Self::format_duration(total_dur - self.player.position))
            } else {
                Self::format_duration(total_dur)
            };

            let time_label = format!("{} / {}", cur_str, dur_str);
            let time_btn = ui.add(
                egui::Button::new(
                    RichText::new(time_label)
                        .font(FontId::monospace(13.0))
                        .color(Color32::from_rgb(215, 220, 230)),
                )
                .frame(false),
            );
            if time_btn.clicked() {
                self.show_remaining_time = !self.show_remaining_time;
            }

            // --- RIGHT GROUP: Volume + Slider + Gear(HD) + Fullscreen ---
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(12.0, 0.0);

                // Fullscreen icon (Screenshot 2: outward arrows ⛶ / shrink 🗗)
                let fs_icon = if self.is_fullscreen { "🗗" } else { "⛶" };
                let fs_btn = egui::Button::new(
                    RichText::new(fs_icon).size(17.0).color(Color32::WHITE),
                )
                .frame(false);
                if ui.add(fs_btn).clicked() {
                    self.toggle_fullscreen(ctx);
                }

                // Settings Gear Button with Blue HD badge (Screenshot 2)
                let gear_btn = egui::Button::new(
                    RichText::new("⚙").size(18.0).color(Color32::WHITE),
                )
                .frame(false);
                let gear_resp = ui.add(gear_btn);
                *gear_button_rect = Some(gear_resp.rect);

                // Blue HD badge in corner of gear button
                let badge_pos = gear_resp.rect.right_top() - Vec2::new(4.0, -2.0);
                let badge_rect = Rect::from_min_size(badge_pos, Vec2::new(18.0, 10.0));
                ui.painter().rect_filled(
                    badge_rect,
                    Rounding::same(2.0_f32),
                    Color32::from_rgba_premultiplied(59, 130, 246, (255.0 * opacity) as u8),
                );
                ui.painter().text(
                    badge_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "HD",
                    FontId::proportional(8.5_f32),
                    Color32::from_rgba_premultiplied(255, 255, 255, (255.0 * opacity) as u8),
                );

                if gear_resp.clicked() {
                    self.show_settings_popup = !self.show_settings_popup;
                    self.settings_submenu = SettingsSubmenu::Main;
                }

                // Horizontal Volume Pill Bar (Screenshot 2 & 3)
                let mut vol = if self.player.is_muted { 0.0 } else { self.player.volume };
                let (vol_rect, vol_resp) = ui.allocate_exact_size(Vec2::new(65.0, 14.0), Sense::click_and_drag());
                if vol_resp.dragged() || vol_resp.clicked() {
                    if let Some(mpos) = vol_resp.interact_pointer_pos() {
                        let v = ((mpos.x - vol_rect.left()) / vol_rect.width()).clamp(0.0, 1.0);
                        vol = v as f64;
                        self.player.set_volume(vol);
                    }
                }

                // Paint volume rail & pill progress
                let v_rail = Rect::from_center_size(vol_rect.center(), Vec2::new(vol_rect.width(), 4.0_f32));
                ui.painter().rect_filled(
                    v_rail,
                    Rounding::same(2.0_f32),
                    Color32::from_rgba_premultiplied(60, 64, 76, (255.0 * opacity) as u8),
                );
                let v_fill_w = (v_rail.width() * vol as f32).max(4.0_f32);
                let v_fill = Rect::from_min_size(v_rail.min, Vec2::new(v_fill_w, 4.0_f32));
                ui.painter().rect_filled(
                    v_fill,
                    Rounding::same(2.0_f32),
                    Color32::from_rgba_premultiplied(255, 255, 255, (255.0 * opacity) as u8),
                );

                // Speaker icon (toggles mute)
                let mute_icon = if self.player.is_muted || self.player.volume == 0.0 {
                    "🔇"
                } else if self.player.volume < 0.4 {
                    "🔈"
                } else {
                    "🔊"
                };

                let mute_btn = egui::Button::new(
                    RichText::new(mute_icon).size(15.0).color(Color32::WHITE),
                )
                .frame(false);
                if ui.add(mute_btn).clicked() {
                    self.player.toggle_mute();
                }
            });
        });
    }

    fn format_duration(d: Duration) -> String {
        let total_secs = d.as_secs();
        let hours = total_secs / 3600;
        let mins = (total_secs % 3600) / 60;
        let secs = total_secs % 60;

        if hours > 0 {
            format!("{:02}:{:02}:{:02}", hours, mins, secs)
        } else {
            format!("{:02}:{:02}", mins, secs)
        }
    }
}

impl eframe::App for MediaPlayerApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(session) = self.player.create_session() {
            session.save();
        }
    }

    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // Track pointer movement
        let current_pointer_pos = ctx.input(|i| i.pointer.hover_pos());
        if current_pointer_pos != self.last_cursor_pos {
            self.last_cursor_pos = current_pointer_pos;
            self.last_mouse_activity = Instant::now();
        }

        // Smooth controls opacity and cursor auto-hide in fullscreen
        let is_interacting = self.is_seeking || self.show_settings_popup;
        let is_mouse_recent = self.last_mouse_activity.elapsed() < Duration::from_millis(2200);

        let target_opacity: f32 = if !self.is_fullscreen {
            1.0
        } else if is_interacting || is_mouse_recent {
            1.0
        } else {
            0.0
        };

        // Smooth transition animation (~300ms)
        let dt = ctx.input(|i| i.stable_dt).clamp(0.001, 0.1);
        let speed = 6.0_f32;
        self.controls_opacity += (target_opacity - self.controls_opacity) * (speed * dt).min(1.0);
        if (self.controls_opacity - target_opacity).abs() < 0.005 {
            self.controls_opacity = target_opacity;
        } else {
            ctx.request_repaint();
        }

        // Auto-hide mouse cursor in fullscreen when idle
        if self.is_fullscreen && self.controls_opacity <= 0.05 {
            ctx.set_cursor_icon(egui::CursorIcon::None);
        } else {
            ctx.set_cursor_icon(egui::CursorIcon::Default);
        }

        // Periodic session persistence every 3s while playing
        if self.player.state == PlaybackState::Playing {
            if self.last_session_save.elapsed() > Duration::from_secs(3) {
                if let Some(session) = self.player.create_session() {
                    session.save();
                }
                self.last_session_save = Instant::now();
            }
        }

        // Drain player events
        for event in self.player.poll_events() {
            match event {
                crate::player::PlayerEvent::Error(err) => {
                    self.show_osd(format!("Ошибка: {}", err));
                }
                crate::player::PlayerEvent::StateChanged(state) => match state {
                    PlaybackState::Buffering(percent) => {
                        self.show_osd(format!("Буферизация: {}%", percent));
                    }
                    PlaybackState::Ended => {
                        PlaybackSession::clear();
                        self.resume_session = None;
                        if self.player.playlist.auto_next && self.player.playlist.has_next() {
                            if let Ok(true) = self.player.next_episode() {
                                if let Some(item) = self.player.playlist.current_item() {
                                    let clean = crate::player::tracks::clean_title(&item.title);
                                    self.show_osd(format!("⏭ Следующая: {}", clean));
                                }
                            }
                        }
                    }
                    _ => (),
                },
                _ => (),
            }
        }

        // Receive video frames (zero UI lag, ColorImage prepared on background thread)
        let mut latest_frame = None;
        while let Some(frame) = self.player.try_recv_frame() {
            latest_frame = Some(frame);
        }

        if let Some(frame) = latest_frame {
            // Zero-copy texture upload by passing Arc<ColorImage> directly
            let image_data = egui::ImageData::Color(frame.image.clone());
            if let Some(texture) = &mut self.video_texture {
                texture.set(image_data, egui::TextureOptions::LINEAR);
            } else {
                self.video_texture = Some(ctx.load_texture(
                    "video-stream",
                    image_data,
                    egui::TextureOptions::LINEAR,
                ));
            }
            self.video_dimensions = [frame.width as f32, frame.height as f32];
        }

        // --- Global hotkeys ---
        if ctx.input(|i| i.key_pressed(Key::F11)) || ctx.input(|i| i.key_pressed(Key::F)) {
            self.toggle_fullscreen(ctx);
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            if self.show_settings_popup {
                self.show_settings_popup = false;
            } else if self.is_fullscreen {
                self.toggle_fullscreen(ctx);
            }
        }
        if ctx.input(|i| i.key_pressed(Key::Space)) || ctx.input(|i| i.key_pressed(Key::K)) {
            let _ = self.player.toggle_play_pause();
            let label = match self.player.state {
                PlaybackState::Playing => "⏸ Пауза",
                _ => "▶ Воспроизведение",
            };
            self.show_osd(label);
        }

        // Hotkey: A - Cycle Audio Tracks (Озвучки)
        if ctx.input(|i| i.key_pressed(Key::A)) {
            if let Some(track_name) = self.player.cycle_audio_track() {
                self.show_osd(format!("🗣 {}", track_name));
            } else {
                self.show_osd("🗣 Одна аудиодорожка");
            }
        }

        // Hotkey: S - Cycle Subtitles (Субтитры)
        if ctx.input(|i| i.key_pressed(Key::S)) {
            if let Some(sub_name) = self.player.cycle_subtitle_track() {
                self.show_osd(format!("💬 {}", sub_name));
            }
        }

        // Rewind & Fast Forward (Left/Right)
        let shift_down = ctx.input(|i| i.modifiers.shift);
        if ctx.input(|i| i.key_pressed(Key::ArrowLeft)) || ctx.input(|i| i.key_pressed(Key::J)) {
            let delta = if shift_down { -10.0 } else { -5.0 };
            let _ = self.player.seek_relative(delta);
            self.show_osd(format!(
                "⏪ {}с ({})",
                delta as i32,
                Self::format_duration(self.player.position)
            ));
        }
        if ctx.input(|i| i.key_pressed(Key::ArrowRight)) || ctx.input(|i| i.key_pressed(Key::L)) {
            let delta = if shift_down { 10.0 } else { 5.0 };
            let _ = self.player.seek_relative(delta);
            self.show_osd(format!(
                "⏩ +{}с ({})",
                delta as i32,
                Self::format_duration(self.player.position)
            ));
        }

        // Volume (Up/Down/M)
        if ctx.input(|i| i.key_pressed(Key::ArrowUp)) {
            let new_vol = (self.player.volume + 0.05).min(1.0);
            self.player.set_volume(new_vol);
            self.show_osd(format!("🔊 {:.0}%", new_vol * 100.0));
        }
        if ctx.input(|i| i.key_pressed(Key::ArrowDown)) {
            let new_vol = (self.player.volume - 0.05).max(0.0);
            self.player.set_volume(new_vol);
            self.show_osd(format!("🔉 {:.0}%", new_vol * 100.0));
        }
        if ctx.input(|i| i.key_pressed(Key::M)) {
            self.player.toggle_mute();
            if self.player.is_muted {
                self.show_osd("🔇 Без звука");
            } else {
                self.show_osd(format!("🔊 {:.0}%", self.player.volume * 100.0));
            }
        }

        // Episodes navigation (N / P)
        if ctx.input(|i| i.key_pressed(Key::N)) {
            if let Ok(true) = self.player.next_episode() {
                if let Some(item) = self.player.playlist.current_item() {
                    let clean = crate::player::tracks::clean_title(&item.title);
                    self.show_osd(format!("⏭ {}", clean));
                }
            }
        }
        if ctx.input(|i| i.key_pressed(Key::P)) {
            if let Ok(true) = self.player.prev_episode() {
                if let Some(item) = self.player.playlist.current_item() {
                    let clean = crate::player::tracks::clean_title(&item.title);
                    self.show_osd(format!("⏮ {}", clean));
                }
            }
        }

        // Handle URL dialog
        if let Some(url) = self.url_dialog.show(ctx) {
            self.resume_session = None;
            if let Err(e) = self.player.open_url(&url) {
                self.show_osd(format!("Ошибка: {}", e));
            } else {
                self.show_osd(format!("📡 Поток: {}", url));
                let _ = self.player.play();
            }
        }

        // =========================================================================
        // 1. TOP BAR: KODIK-STYLE VOICE & EPISODE SELECTOR (Screenshot 1)
        // =========================================================================
        if !self.is_fullscreen {
            let mut play_episode_idx = None;
            let mut switch_audio_id = None;

            egui::TopBottomPanel::top("top_kodik_header")
                .frame(
                    egui::Frame::none()
                        .fill(Color32::from_rgb(20, 22, 28))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 48)))
                        .inner_margin(Vec2::new(14.0, 10.0)),
                )
                .show(ctx, |ui| {
                    // Row 1: Selectors (Озвучка AniDUB, Плеер / Папка)
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(8.0, 0.0);

                        // Audio track selector (Озвучка AniDUB (24 эп.))
                        let current_audio_id = self.player.current_audio;
                        let current_track = self
                            .player
                            .audio_tracks
                            .iter()
                            .find(|t| t.id == current_audio_id);

                        let ep_count_suffix = if self.player.playlist.is_empty() {
                            String::new()
                        } else {
                            format!(" ({} эп.)", self.player.playlist.len())
                        };

                        let audio_title = if let Some(t) = current_track {
                            format!("Озвучка {}{}", t.short_name(), ep_count_suffix)
                        } else if self.player.audio_tracks.is_empty() {
                            format!("Озвучка (По умолчанию){}", ep_count_suffix)
                        } else {
                            format!("Озвучка #{}{}", current_audio_id + 1, ep_count_suffix)
                        };

                        let audio_btn_text = crate::player::tracks::truncate_ellipsis(&audio_title, 26);

                        let combo_resp = egui::ComboBox::from_id_source("kodik_audio_select")
                            .selected_text(RichText::new(audio_btn_text).color(Color32::from_rgb(220, 225, 235)).size(13.0))
                            .width(215.0)
                            .show_ui(ui, |ui| {
                                if self.player.audio_tracks.is_empty() {
                                    ui.label(RichText::new("Нет других дорожек").color(Color32::GRAY));
                                } else {
                                    for track in &self.player.audio_tracks {
                                        let is_selected = track.id == current_audio_id;
                                        let full_name = track.display_name();
                                        let short_name = crate::player::tracks::truncate_ellipsis(&full_name, 36);
                                        let label_resp = ui.selectable_label(is_selected, short_name)
                                            .on_hover_text(&full_name);
                                        if !is_selected && label_resp.clicked() {
                                            switch_audio_id = Some((track.id, track.short_name()));
                                        }
                                    }
                                }
                            });

                        if let Some(t) = current_track {
                            combo_resp.response.on_hover_text(t.display_name());
                        }

                        // Source / Season selector (Плеер Kodik / Папка / Сезон)
                        let season_label = if let Some(item) = self.player.playlist.current_item() {
                            let clean = crate::player::tracks::clean_title(&item.title);
                            crate::player::tracks::truncate_ellipsis(&clean, 20)
                        } else if self.player.playlist.is_empty() {
                            "Источник: Файл".to_string()
                        } else {
                            format!("Сезон ({})", self.player.playlist.len())
                        };

                        egui::ComboBox::from_id_source("kodik_season_select")
                            .selected_text(RichText::new(season_label).color(Color32::from_rgb(200, 205, 215)).size(13.0))
                            .width(175.0)
                            .show_ui(ui, |ui| {
                                if ui.button("📂 Открыть один файл...").clicked() {
                                    self.open_file_dialog();
                                    ui.close_menu();
                                }
                                if ui.button("📁 Открыть сезон / папку...").clicked() {
                                    self.open_season_dialog();
                                    ui.close_menu();
                                }
                                if ui.button("🔗 Открыть ссылку (URL)...").clicked() {
                                    self.url_dialog.open();
                                    ui.close_menu();
                                }
                            });

                        ui.add_space(8.0);

                        // Quick Open Button
                        if ui.add(
                            egui::Button::new(RichText::new("📂 Открыть видео").color(Color32::WHITE).size(12.5))
                                .fill(Color32::from_rgb(38, 42, 54))
                                .min_size(Vec2::new(110.0, 28.0))
                        ).clicked() {
                            self.open_file_dialog();
                        }
                    });

                    ui.add_space(8.0);

                    // Row 2: EPISODE BUTTONS GRID (Screenshot 1: 1, 2, 3... 24)
                    let total_episodes = if self.player.playlist.is_empty() {
                        1
                    } else {
                        self.player.playlist.len()
                    };

                    let current_episode_idx = self.player.playlist.current_index.unwrap_or(0);

                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(5.0, 5.0);

                        for ep_idx in 0..total_episodes {
                            let ep_num = ep_idx + 1;
                            let is_active = ep_idx == current_episode_idx;

                            // Active episode is vibrant emerald green #22c55e with black text (Screenshot 1: ep 24)
                            let (bg_color, text_color) = if is_active {
                                (Color32::from_rgb(34, 197, 94), Color32::from_rgb(10, 15, 10))
                            } else {
                                (Color32::from_rgb(45, 48, 58), Color32::from_rgb(220, 225, 235))
                            };

                            let ep_btn = egui::Button::new(
                                RichText::new(format!("{}", ep_num))
                                    .font(FontId::proportional(13.0))
                                    .color(text_color)
                                    .strong(),
                            )
                            .fill(bg_color)
                            .min_size(Vec2::new(34.0, 30.0))
                            .rounding(Rounding::same(4.0_f32));

                            let response = ui.add(ep_btn);

                            // Subtle purple top-edge highlight on inactive buttons (Screenshot 1)
                            if !is_active {
                                let top_rect = Rect::from_min_size(
                                    response.rect.min,
                                    Vec2::new(response.rect.width(), 2.5_f32),
                                );
                                ui.painter().rect_filled(
                                    top_rect,
                                    Rounding::same(1.0_f32),
                                    Color32::from_rgb(124, 58, 237), // Purple tab
                                );
                            }

                            if response.clicked() {
                                play_episode_idx = Some(ep_idx);
                            }
                        }
                    });
                });

            if let Some((id, name)) = switch_audio_id {
                self.player.set_audio_track(id);
                self.show_osd(format!("🗣 {}", name));
            }
            if let Some(idx) = play_episode_idx {
                if !self.player.playlist.is_empty() {
                    let _ = self.player.play_episode(idx);
                }
            }
        }

        // =========================================================================
        // 2. BOTTOM CONTROLS & TIMELINE (Screenshots 2 & 3)
        // =========================================================================
        let mut seek_to = None;
        let mut toggle_play_pause = false;
        let mut gear_button_rect = None;

        // In windowed mode: standard docked bottom panel
        if !self.is_fullscreen {
            egui::TopBottomPanel::bottom("bottom_kodik_bar")
                .frame(
                    egui::Frame::none()
                        .fill(Color32::from_rgb(14, 15, 19))
                        .inner_margin(Vec2::new(16.0, 6.0)),
                )
                .show(ctx, |ui| {
                    self.render_bottom_controls(
                        ui,
                        ctx,
                        1.0,
                        &mut seek_to,
                        &mut toggle_play_pause,
                        &mut gear_button_rect,
                    );
                });
        }

        // =========================================================================
        // 3. CENTRAL VIDEO VIEWPORT & RESUME PROMPT (Letterboxed video / Kodik Card)
        // =========================================================================
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(Color32::from_rgb(0, 0, 0)))
            .show(ctx, |ui| {
                let available_size = ui.available_size();
                let mut toggle_fullscreen_click = false;

                if let Some(texture) = &self.video_texture {
                    let [vw, vh] = self.video_dimensions;
                    if vw > 0.0 && vh > 0.0 {
                        let aspect = vw / vh;
                        let mut target_w = available_size.x;
                        let mut target_h = target_w / aspect;

                        if target_h > available_size.y {
                            target_h = available_size.y;
                            target_w = target_h * aspect;
                        }

                        let x_offset = (available_size.x - target_w) / 2.0;
                        let y_offset = (available_size.y - target_h) / 2.0;

                        let rect = Rect::from_min_size(
                            ui.min_rect().min + Vec2::new(x_offset, y_offset),
                            Vec2::new(target_w, target_h),
                        );

                        let response = ui.allocate_rect(rect, Sense::click());
                        if response.double_clicked() {
                            toggle_fullscreen_click = true;
                        } else if response.clicked() {
                            toggle_play_pause = true;
                        }

                        ui.painter().image(
                            texture.id(),
                            rect,
                            Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
                            Color32::WHITE,
                        );

                        // If video is paused: Coral-Red Center Play Icon (Screenshot 1)
                        if self.player.state == PlaybackState::Paused {
                            let center = rect.center();
                            let radius = 32.0_f32;

                            // Outer translucent ring
                            ui.painter().circle_filled(
                                center,
                                radius + 10.0_f32,
                                Color32::from_rgba_premultiplied(239, 68, 68, 60),
                            );

                            // Coral red circle
                            ui.painter().circle_filled(
                                center,
                                radius,
                                Color32::from_rgb(239, 68, 68), // Coral red
                            );

                            // Play triangle in center
                            let tri_p1 = center + Vec2::new(-9.0_f32, -14.0_f32);
                            let tri_p2 = center + Vec2::new(-9.0_f32, 14.0_f32);
                            let tri_p3 = center + Vec2::new(14.0_f32, 0.0_f32);
                            ui.painter().add(egui::Shape::convex_polygon(
                                vec![tri_p1, tri_p2, tri_p3],
                                Color32::WHITE,
                                Stroke::NONE,
                            ));
                        }

                        // Floating OSD Badge in top-left
                        if let Some(expires_at) = self.osd_expires_at {
                            if Instant::now() < expires_at {
                                if let Some(ref text) = self.osd_text {
                                    let font_id = FontId::proportional(15.0_f32);
                                    let layout = ui.painter().layout_no_wrap(
                                        text.clone(),
                                        font_id,
                                        Color32::WHITE,
                                    );
                                    let padding = Vec2::new(14.0_f32, 8.0_f32);
                                    let osd_size = layout.size() + padding * 2.0_f32;

                                    let osd_rect = Rect::from_min_size(
                                        rect.min + Vec2::new(20.0_f32, 20.0_f32),
                                        osd_size,
                                    );

                                    ui.painter().rect_filled(
                                        osd_rect,
                                        Rounding::same(6.0_f32),
                                        Color32::from_rgba_premultiplied(16, 18, 24, 230),
                                    );
                                    ui.painter().rect_stroke(
                                        osd_rect,
                                        Rounding::same(6.0_f32),
                                        Stroke::new(1.0_f32, Color32::from_rgb(50, 56, 72)),
                                    );
                                    ui.painter().galley(osd_rect.min + padding, layout, Color32::WHITE);
                                }
                            }
                        }
                    }
                } else if let Some(session) = self.resume_session.clone() {
                    // Kodik/AnimeGo-style prompt to resume previous session!
                    let center = ui.min_rect().center();
                    let card_w = 480.0_f32.min(available_size.x - 32.0);
                    let card_h = 165.0_f32;
                    let card_rect = Rect::from_center_size(center, Vec2::new(card_w, card_h));

                    // Elegant card container
                    ui.painter().rect_filled(
                        card_rect,
                        Rounding::same(10.0_f32),
                        Color32::from_rgb(18, 20, 26),
                    );
                    ui.painter().rect_stroke(
                        card_rect,
                        Rounding::same(10.0_f32),
                        Stroke::new(1.0_f32, Color32::from_rgb(45, 52, 68)),
                    );

                    let mut resume_clicked = false;
                    let mut cancel_clicked = false;
                    let mut open_dialog_clicked = false;

                    ui.allocate_ui_at_rect(card_rect.shrink(18.0_f32), |ui| {
                        ui.vertical_centered(|ui| {
                            // Section header with Kodik purple
                            ui.label(
                                RichText::new("⏱  ПРОДОЛЖИТЬ ПРОСМОТР")
                                    .font(FontId::proportional(11.5_f32))
                                    .color(Color32::from_rgb(139, 92, 246))
                                    .strong(),
                            );

                            ui.add_space(5.0);

                            // Clean Title without technical junk tags
                            let clean_display = crate::player::tracks::truncate_ellipsis(&session.clean_title, 42);
                            ui.label(
                                RichText::new(format!("«{}»", clean_display))
                                    .font(FontId::proportional(16.0_f32))
                                    .color(Color32::WHITE)
                                    .strong(),
                            );

                            ui.add_space(4.0);

                            // Position & duration
                            let pos_str = Self::format_duration(Duration::from_secs_f64(session.position_secs));
                            let dur_str = session.duration_secs
                                .map(|d| format!(" / {}", Self::format_duration(Duration::from_secs_f64(d))))
                                .unwrap_or_default();
                            let ep_str = if let Some(idx) = session.episode_index {
                                format!("Серия {} • ", idx + 1)
                            } else {
                                String::new()
                            };

                            ui.label(
                                RichText::new(format!("Остановились на {}{}{}", ep_str, pos_str, dur_str))
                                    .font(FontId::monospace(12.5_f32))
                                    .color(Color32::from_rgb(160, 165, 180)),
                            );

                            ui.add_space(14.0);

                            // Action buttons
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing = Vec2::new(10.0, 0.0);

                                // Primary Resume Button: Emerald green (Screenshot 1 active episode style)
                                let resume_btn = egui::Button::new(
                                    RichText::new("▶ Продолжить просмотр")
                                        .font(FontId::proportional(13.0_f32))
                                        .color(Color32::from_rgb(10, 15, 10))
                                        .strong(),
                                )
                                .fill(Color32::from_rgb(34, 197, 94))
                                .min_size(Vec2::new(170.0, 32.0))
                                .rounding(Rounding::same(6.0_f32));

                                if ui.add(resume_btn).clicked() {
                                    resume_clicked = true;
                                }

                                // Cancel / Dismiss button
                                let cancel_btn = egui::Button::new(
                                    RichText::new("✕ Сбросить")
                                        .font(FontId::proportional(12.5_f32))
                                        .color(Color32::from_rgb(200, 205, 215)),
                                )
                                .fill(Color32::from_rgb(38, 42, 54))
                                .min_size(Vec2::new(95.0, 32.0))
                                .rounding(Rounding::same(6.0_f32));

                                if ui.add(cancel_btn).clicked() {
                                    cancel_clicked = true;
                                }

                                // Open another file button
                                let open_btn = egui::Button::new(
                                    RichText::new("📂 Выбрать другой")
                                        .font(FontId::proportional(12.5_f32))
                                        .color(Color32::from_rgb(180, 185, 200)),
                                )
                                .fill(Color32::from_rgb(28, 30, 40))
                                .min_size(Vec2::new(120.0, 32.0))
                                .rounding(Rounding::same(6.0_f32));

                                if ui.add(open_btn).clicked() {
                                    open_dialog_clicked = true;
                                }
                            });
                        });
                    });

                    if resume_clicked {
                        self.resume_saved_session();
                    } else if cancel_clicked {
                        self.resume_session = None;
                        PlaybackSession::clear();
                    } else if open_dialog_clicked {
                        self.open_file_dialog();
                    }
                } else {
                    // Empty state with Coral Red Play Icon in center
                    let center = ui.min_rect().center();
                    let radius = 38.0_f32;

                    let click_resp = ui.allocate_rect(
                        Rect::from_center_size(center, Vec2::splat(radius * 2.5_f32)),
                        Sense::click(),
                    );
                    if click_resp.clicked() {
                        self.open_file_dialog();
                    }

                    ui.painter().circle_filled(
                        center,
                        radius + 12.0_f32,
                        Color32::from_rgba_premultiplied(239, 68, 68, 40),
                    );
                    ui.painter().circle_filled(
                        center,
                        radius,
                        Color32::from_rgb(239, 68, 68),
                    );

                    let tri_p1 = center + Vec2::new(-11.0_f32, -16.0_f32);
                    let tri_p2 = center + Vec2::new(-11.0_f32, 16.0_f32);
                    let tri_p3 = center + Vec2::new(16.0_f32, 0.0_f32);
                    ui.painter().add(egui::Shape::convex_polygon(
                        vec![tri_p1, tri_p2, tri_p3],
                        Color32::WHITE,
                        Stroke::NONE,
                    ));

                    ui.painter().text(
                        center + Vec2::new(0.0_f32, radius + 28.0_f32),
                        egui::Align2::CENTER_CENTER,
                        "Нажмите, чтобы открыть видео",
                        FontId::proportional(14.0_f32),
                        Color32::from_rgb(160, 165, 180),
                    );
                }

                if toggle_fullscreen_click {
                    self.toggle_fullscreen(ctx);
                }
            });

        // =========================================================================
        // 4. FULLSCREEN FLOATING OVERLAY WITH SMOOTH FADE
        // =========================================================================
        if self.is_fullscreen && self.controls_opacity > 0.01 {
            let screen_rect = ctx.screen_rect();
            let bar_height = 56.0_f32;
            let overlay_pos = Pos2::new(0.0, screen_rect.max.y - bar_height);
            let overlay_size = Vec2::new(screen_rect.width(), bar_height);

            egui::Area::new(egui::Id::new("fs_bottom_bar_overlay"))
                .fixed_pos(overlay_pos)
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    ui.set_min_size(overlay_size);
                    ui.set_max_size(overlay_size);

                    let bg_alpha = (235.0 * self.controls_opacity) as u8;
                    egui::Frame::none()
                        .fill(Color32::from_rgba_premultiplied(12, 14, 18, bg_alpha))
                        .inner_margin(Vec2::new(16.0, 6.0))
                        .show(ui, |ui| {
                            self.render_bottom_controls(
                                ui,
                                ctx,
                                self.controls_opacity,
                                &mut seek_to,
                                &mut toggle_play_pause,
                                &mut gear_button_rect,
                            );
                        });
                });
        }

        // =========================================================================
        // 5. SETTINGS POPUP MENU (Screenshot 2: Качество 720p HD, Скорость 1x)
        // =========================================================================
        if self.show_settings_popup && (self.controls_opacity > 0.05 || !self.is_fullscreen) {
            if let Some(gear_rect) = gear_button_rect {
                let popup_size = Vec2::new(220.0, 110.0);
                let popup_pos = Pos2::new(
                    (gear_rect.center().x - popup_size.x + 20.0).max(10.0),
                    gear_rect.top() - popup_size.y - 14.0,
                );

                egui::Area::new(egui::Id::new("kodik_settings_popup"))
                    .fixed_pos(popup_pos)
                    .order(egui::Order::Foreground)
                    .show(ctx, |ui| {
                        ui.set_opacity(self.controls_opacity);
                        egui::Frame::none()
                            .fill(Color32::from_rgb(14, 16, 22))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 42, 54)))
                            .rounding(Rounding::same(8.0_f32))
                            .inner_margin(Vec2::new(12.0, 10.0))
                            .show(ui, |ui| {
                                match self.settings_submenu {
                                    SettingsSubmenu::Main => {
                                        // Row 1: ⚙ Качество | 720p HD
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new("⚙  Качество").size(13.5).color(Color32::WHITE));
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                let q_text = match self.player.connection_speed_kbps {
                                                    0 => "1080p HD",
                                                    5000 => "720p HD",
                                                    2500 => "480p",
                                                    _ => "Auto",
                                                };
                                                if ui.button(RichText::new(q_text).color(Color32::from_rgb(150, 160, 180))).clicked() {
                                                    self.settings_submenu = SettingsSubmenu::Quality;
                                                }
                                            });
                                        });

                                        ui.add_space(8.0);

                                        // Row 2: ⏱ Скорость | 1x
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new("⏱  Скорость").size(13.5).color(Color32::WHITE));
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                let s_text = format!("{:.2}x", self.player.rate);
                                                if ui.button(RichText::new(s_text).color(Color32::from_rgb(150, 160, 180))).clicked() {
                                                    self.settings_submenu = SettingsSubmenu::Speed;
                                                }
                                            });
                                        });

                                        ui.add_space(8.0);

                                        // Row 3: 💬 Субтитры
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new("💬  Субтитры").size(13.5).color(Color32::WHITE));
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                let sub_name = if self.player.current_subtitle < 0 {
                                                    "Откл.".to_string()
                                                } else {
                                                    "Вкл.".to_string()
                                                };
                                                if ui.button(RichText::new(sub_name).color(Color32::from_rgb(150, 160, 180))).clicked() {
                                                    self.settings_submenu = SettingsSubmenu::Subtitles;
                                                }
                                            });
                                        });
                                    }
                                    SettingsSubmenu::Speed => {
                                        ui.horizontal(|ui| {
                                            if ui.button("◀ Назад").clicked() {
                                                self.settings_submenu = SettingsSubmenu::Main;
                                            }
                                            ui.label(RichText::new("Скорость").strong());
                                        });
                                        ui.separator();
                                        for s in [0.5, 0.75, 1.0, 1.25, 1.5, 2.0] {
                                            let label = format!("{:.2}x", s);
                                            if ui.selectable_label(self.player.rate == s, label).clicked() {
                                                let _ = self.player.set_rate(s);
                                                self.show_osd(format!("Скорость: {:.2}x", s));
                                                self.settings_submenu = SettingsSubmenu::Main;
                                            }
                                        }
                                    }
                                    SettingsSubmenu::Quality => {
                                        ui.horizontal(|ui| {
                                            if ui.button("◀ Назад").clicked() {
                                                self.settings_submenu = SettingsSubmenu::Main;
                                            }
                                            ui.label(RichText::new("Качество").strong());
                                        });
                                        ui.separator();
                                        for (speed, label) in [
                                            (0, "1080p HD (Авто)"),
                                            (5000, "720p HD"),
                                            (2500, "480p"),
                                            (1000, "360p"),
                                        ] {
                                            if ui.selectable_label(self.player.connection_speed_kbps == speed, label).clicked() {
                                                self.player.set_connection_speed(speed);
                                                self.show_osd(format!("Качество: {}", label));
                                                self.settings_submenu = SettingsSubmenu::Main;
                                            }
                                        }
                                    }
                                    SettingsSubmenu::Subtitles => {
                                        ui.horizontal(|ui| {
                                            if ui.button("◀ Назад").clicked() {
                                                self.settings_submenu = SettingsSubmenu::Main;
                                            }
                                            ui.label(RichText::new("Субтитры").strong());
                                        });
                                        ui.separator();
                                        if ui.button("➕ Загрузить файл .srt/.vtt").clicked() {
                                            self.open_subtitle_dialog();
                                            self.settings_submenu = SettingsSubmenu::Main;
                                        }
                                        let mut pick_sub = None;
                                        for track in &self.player.subtitle_tracks {
                                            let is_selected = track.id == self.player.current_subtitle;
                                            let full_name = track.display_name();
                                            let short_name = crate::player::tracks::truncate_ellipsis(&full_name, 30);
                                            let label_resp = ui.selectable_label(is_selected, short_name)
                                                .on_hover_text(&full_name);
                                            if !is_selected && label_resp.clicked() {
                                                pick_sub = Some((track.id, track.short_name()));
                                            }
                                        }
                                        if let Some((id, name)) = pick_sub {
                                            self.player.set_subtitle_track(id);
                                            self.show_osd(format!("💬 {}", name));
                                            self.settings_submenu = SettingsSubmenu::Main;
                                        }
                                    }
                                }
                            });
                    });
            }
        }

        // Apply seek requested from timeline
        if let Some(target) = seek_to {
            let _ = self.player.seek(target);
            if let Some(session) = self.player.create_session() {
                session.save();
            }
            self.show_osd(format!("Перемотка: {}", Self::format_duration(target)));
        }

        // Apply play/pause toggles
        if toggle_play_pause {
            let _ = self.player.toggle_play_pause();
            if let Some(session) = self.player.create_session() {
                session.save();
            }
            let label = match self.player.state {
                PlaybackState::Playing => "⏸ Пауза",
                _ => "▶ Воспроизведение",
            };
            self.show_osd(label);
        }

        // Repaint request for smooth 60fps playback, controls fade animation, or OSD animation
        if self.player.state == PlaybackState::Playing
            || self.osd_expires_at.map(|e| Instant::now() < e).unwrap_or(false)
            || (self.controls_opacity > 0.0 && self.controls_opacity < 1.0)
            || self.is_seeking
        {
            ctx.request_repaint();
        }
    }
}
