use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};

use eframe::egui;
use egui::{Color32, FontId, RichText, Rounding, Stroke, Vec2};

use crate::extractor::{
    extract_audio, extract_subtitle, find_ffmpeg, inspect_file, AudioExportFormat,
    ExtractedAudioTrack, ExtractedSubtitleTrack, MediaFileInfo, SubtitleExportFormat,
};

#[derive(Debug, Clone)]
pub enum ExportResult {
    Success { message: String, output_path: PathBuf },
    Error { message: String },
}

pub struct ExtractorTabUi {
    pub current_file: Option<PathBuf>,
    pub file_info: Option<MediaFileInfo>,
    pub is_inspecting: bool,
    pub inspect_error: Option<String>,

    // Selected export formats per stream
    pub audio_export_formats: HashMap<usize, AudioExportFormat>,
    pub sub_export_formats: HashMap<usize, SubtitleExportFormat>,

    // Background job communication
    inspect_receiver: Option<Receiver<Result<MediaFileInfo, String>>>,
    export_receiver: Option<Receiver<ExportResult>>,

    // Active export status
    pub is_exporting: bool,
    pub export_task_name: Option<String>,
    pub last_export_result: Option<ExportResult>,
}

impl Default for ExtractorTabUi {
    fn default() -> Self {
        Self {
            current_file: None,
            file_info: None,
            is_inspecting: false,
            inspect_error: None,
            audio_export_formats: HashMap::new(),
            sub_export_formats: HashMap::new(),
            inspect_receiver: None,
            export_receiver: None,
            is_exporting: false,
            export_task_name: None,
            last_export_result: None,
        }
    }
}

impl ExtractorTabUi {
    pub fn load_file(&mut self, path: PathBuf) {
        if self.current_file.as_ref() == Some(&path) && self.file_info.is_some() {
            return;
        }

        self.current_file = Some(path.clone());
        self.is_inspecting = true;
        self.inspect_error = None;
        self.file_info = None;

        let (tx, rx): (Sender<Result<MediaFileInfo, String>>, Receiver<Result<MediaFileInfo, String>>) =
            channel();
        self.inspect_receiver = Some(rx);

        std::thread::spawn(move || {
            let res = inspect_file(&path);
            let _ = tx.send(res);
        });
    }

    pub fn poll_background_tasks(&mut self) {
        if let Some(ref rx) = self.inspect_receiver {
            if let Ok(result) = rx.try_recv() {
                self.is_inspecting = false;
                match result {
                    Ok(info) => {
                        // Pre-populate default export formats
                        for track in &info.audio_tracks {
                            self.audio_export_formats
                                .entry(track.stream_index)
                                .or_insert(AudioExportFormat::Copy);
                        }
                        for track in &info.subtitle_tracks {
                            let default_fmt = if track.codec.to_lowercase().contains("ass")
                                || track.codec.to_lowercase().contains("ssa")
                            {
                                SubtitleExportFormat::Ass
                            } else {
                                SubtitleExportFormat::Srt
                            };
                            self.sub_export_formats
                                .entry(track.stream_index)
                                .or_insert(default_fmt);
                        }
                        self.file_info = Some(info);
                        self.inspect_error = None;
                    }
                    Err(err) => {
                        self.inspect_error = Some(err);
                        self.file_info = None;
                    }
                }
                self.inspect_receiver = None;
            }
        }

        if let Some(ref rx) = self.export_receiver {
            if let Ok(res) = rx.try_recv() {
                self.is_exporting = false;
                self.export_task_name = None;
                self.last_export_result = Some(res);
                self.export_receiver = None;
            }
        }
    }

    fn format_bytes(bytes: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;

        if bytes >= GB {
            format!("{:.2} GB", bytes as f64 / GB as f64)
        } else if bytes >= MB {
            format!("{:.1} MB", bytes as f64 / MB as f64)
        } else if bytes >= KB {
            format!("{:.0} KB", bytes as f64 / KB as f64)
        } else {
            format!("{} B", bytes)
        }
    }

    fn format_duration_secs(secs_opt: Option<f64>) -> String {
        if let Some(total_secs) = secs_opt {
            let secs = total_secs as u64;
            let hours = secs / 3600;
            let mins = (secs % 3600) / 60;
            let rem_secs = secs % 60;
            if hours > 0 {
                format!("{:02}:{:02}:{:02}", hours, mins, rem_secs)
            } else {
                format!("{:02}:{:02}", mins, rem_secs)
            }
        } else {
            "—".to_string()
        }
    }

    fn start_audio_export(
        &mut self,
        input_file: PathBuf,
        track: &ExtractedAudioTrack,
        format: AudioExportFormat,
        output_file: PathBuf,
    ) {
        self.is_exporting = true;
        self.export_task_name = Some(format!("Озвучка: {} ({})", track.display_name(), format.display_label()));
        self.last_export_result = None;

        let (tx, rx) = channel();
        self.export_receiver = Some(rx);

        let stream_index = track.stream_index;
        let track_name = track.display_name();
        let target_path = output_file.clone();

        std::thread::spawn(move || {
            let res = extract_audio(&input_file, stream_index, format, &target_path);
            let final_result = match res {
                Ok(_) => ExportResult::Success {
                    message: format!("Озвучка «{}» успешно сохранена!", track_name),
                    output_path: target_path,
                },
                Err(err) => ExportResult::Error {
                    message: format!("Ошибка извлечения аудио: {}", err),
                },
            };
            let _ = tx.send(final_result);
        });
    }

    fn start_subtitle_export(
        &mut self,
        input_file: PathBuf,
        track: &ExtractedSubtitleTrack,
        format: SubtitleExportFormat,
        output_file: PathBuf,
    ) {
        self.is_exporting = true;
        self.export_task_name = Some(format!("Субтитры: {} ({})", track.display_name(), format.display_label()));
        self.last_export_result = None;

        let (tx, rx) = channel();
        self.export_receiver = Some(rx);

        let stream_index = track.stream_index;
        let track_name = track.display_name();
        let source_codec = track.codec.clone();
        let target_path = output_file.clone();

        std::thread::spawn(move || {
            let res = extract_subtitle(&input_file, stream_index, &source_codec, format, &target_path);
            let final_result = match res {
                Ok(_) => ExportResult::Success {
                    message: format!("Субтитры «{}» успешно извлечены с оригинальными таймингами!", track_name),
                    output_path: target_path,
                },
                Err(err) => ExportResult::Error {
                    message: format!("Ошибка извлечения субтитров: {}", err),
                },
            };
            let _ = tx.send(final_result);
        });
    }

    fn open_in_explorer(path: &Path) {
        #[cfg(target_os = "windows")]
        {
            let mut cmd = std::process::Command::new("explorer");
            cmd.arg("/select,").arg(path);
            let _ = cmd.spawn();
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, player_file: Option<&Path>) {
        self.poll_background_tasks();

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(8.0);

                // --- 1. TOP HEADER & TOOL STATUS ---
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("📥 Экстрактор озвучек и субтитров")
                            .font(FontId::proportional(19.0))
                            .color(Color32::WHITE)
                            .strong(),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let has_ffmpeg = find_ffmpeg().is_some();
                        let (badge_text, badge_bg, text_color) = if has_ffmpeg {
                            ("✓ FFmpeg готов к работе", Color32::from_rgb(22, 101, 52), Color32::from_rgb(187, 247, 208))
                        } else {
                            ("⚠ FFmpeg не найден", Color32::from_rgb(153, 27, 27), Color32::from_rgb(254, 202, 202))
                        };

                        ui.label(
                            RichText::new(badge_text)
                                .font(FontId::proportional(11.5))
                                .color(text_color)
                                .background_color(badge_bg),
                        );
                    });
                });

                ui.add_space(2.0);
                ui.label(
                    RichText::new("Извлекайте и скачивайте дорожки озвучки (MP3, FLAC, оригинал) и субтитры аниме (с сохранением стилей .ass или таймингов .srt)")
                        .font(FontId::proportional(12.5))
                        .color(Color32::from_rgb(150, 158, 175)),
                );

                ui.add_space(14.0);

                // --- 2. FILE SELECTION CARD ---
                let mut reload_path = None;
                let mut load_player_path = None;
                let mut pick_new_path = false;

                egui::Frame::none()
                    .fill(Color32::from_rgb(20, 23, 31))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 43, 58)))
                    .rounding(Rounding::same(8.0))
                    .inner_margin(Vec2::new(14.0, 12.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let open_btn = ui.add(
                                egui::Button::new(
                                    RichText::new("📂 Выбрать видеофайл...")
                                        .font(FontId::proportional(13.0))
                                        .color(Color32::WHITE)
                                        .strong(),
                                )
                                .fill(Color32::from_rgb(79, 70, 229)) // Kodik accent Indigo
                                .min_size(Vec2::new(165.0, 32.0))
                                .rounding(Rounding::same(6.0)),
                            );

                            if open_btn.clicked() {
                                pick_new_path = true;
                            }

                            // If player has a file loaded, offer quick button
                            if let Some(pf) = player_file {
                                if Some(pf) != self.current_file.as_deref() {
                                    let pf_name = pf.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                                    let clean = crate::player::tracks::clean_title(&pf_name);
                                    let short = crate::player::tracks::truncate_ellipsis(&clean, 28);
                                    let use_player_btn = ui.add(
                                        egui::Button::new(
                                            RichText::new(format!("↺ Взять из плеера: «{}»", short))
                                                .font(FontId::proportional(12.5))
                                                .color(Color32::from_rgb(220, 225, 240)),
                                        )
                                        .fill(Color32::from_rgb(34, 38, 52))
                                        .min_size(Vec2::new(160.0, 32.0))
                                        .rounding(Rounding::same(6.0)),
                                    );
                                    if use_player_btn.clicked() {
                                        load_player_path = Some(pf.to_path_buf());
                                    }
                                }
                            }

                            if let Some(ref path) = self.current_file {
                                let p_clone = path.clone();
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button("↺ Пересканировать").clicked() {
                                        reload_path = Some(p_clone);
                                    }
                                });
                            }
                        });

                        // File details row
                        if let Some(ref info) = self.file_info {
                            ui.add_space(10.0);
                            ui.separator();
                            ui.add_space(8.0);

                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("🎬")
                                        .font(FontId::proportional(16.0)),
                                );
                                ui.label(
                                    RichText::new(&info.clean_title)
                                        .font(FontId::proportional(14.0))
                                        .color(Color32::WHITE)
                                        .strong(),
                                );
                            });

                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing = Vec2::new(14.0, 0.0);

                                ui.label(
                                    RichText::new(format!("Контейнер: {}", info.container_format.to_uppercase()))
                                        .font(FontId::proportional(12.0))
                                        .color(Color32::from_rgb(160, 168, 185)),
                                );
                                ui.label(
                                    RichText::new(format!("Размер: {}", Self::format_bytes(info.file_size_bytes)))
                                        .font(FontId::proportional(12.0))
                                        .color(Color32::from_rgb(160, 168, 185)),
                                );
                                ui.label(
                                    RichText::new(format!("Длительность: {}", Self::format_duration_secs(info.duration_secs)))
                                        .font(FontId::proportional(12.0))
                                        .color(Color32::from_rgb(160, 168, 185)),
                                );
                                ui.label(
                                    RichText::new(format!("🗣 Озвучек: {}", info.audio_tracks.len()))
                                        .font(FontId::proportional(12.0))
                                        .color(Color32::from_rgb(52, 211, 153)) // Emerald green
                                        .strong(),
                                );
                                ui.label(
                                    RichText::new(format!("💬 Субтитров: {}", info.subtitle_tracks.len()))
                                        .font(FontId::proportional(12.0))
                                        .color(Color32::from_rgb(167, 139, 250)) // Purple
                                        .strong(),
                                );
                            });
                        }
                    });

                if pick_new_path {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter(
                            "Video Files",
                            &["mkv", "mp4", "avi", "webm", "ts", "m4v", "mov", "flv"],
                        )
                        .pick_file()
                    {
                        self.load_file(path);
                    }
                }
                if let Some(p) = load_player_path {
                    self.load_file(p);
                }
                if let Some(p) = reload_path {
                    self.load_file(p);
                }

                // --- 3. LOADING / ERROR STATES ---
                if self.is_inspecting {
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| {
                        ui.spinner();
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new("Анализ файла и чтение дорожек...")
                                .font(FontId::proportional(14.0))
                                .color(Color32::from_rgb(180, 190, 210)),
                        );
                    });
                    return;
                }

                if let Some(ref err) = self.inspect_error {
                    ui.add_space(14.0);
                    egui::Frame::none()
                        .fill(Color32::from_rgb(40, 18, 22))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(153, 27, 27)))
                        .rounding(Rounding::same(6.0))
                        .inner_margin(Vec2::new(12.0, 10.0))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(format!("❌ {}", err))
                                    .color(Color32::from_rgb(254, 202, 202))
                                    .font(FontId::proportional(13.0)),
                            );
                        });
                    return;
                }

                // If no file loaded yet
                if self.file_info.is_none() {
                    ui.add_space(40.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("📥")
                                .font(FontId::proportional(42.0)),
                        );
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new("Выберите аниме или фильм, чтобы извлечь субтитры или озвучки")
                                .font(FontId::proportional(15.0))
                                .color(Color32::from_rgb(180, 185, 200)),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("Поддерживается извлечение стилей аниме (.ass), классических субтитров с таймингами (.srt), и аудио в MP3 / FLAC / оригинал")
                                .font(FontId::proportional(12.5))
                                .color(Color32::GRAY),
                        );
                    });
                    return;
                }

                let info = self.file_info.as_ref().unwrap().clone();
                let current_input_path = self.current_file.as_ref().unwrap().clone();

                // --- 4. EXPORT STATUS BANNER ---
                if self.is_exporting {
                    ui.add_space(10.0);
                    egui::Frame::none()
                        .fill(Color32::from_rgb(26, 32, 50))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(79, 70, 229)))
                        .rounding(Rounding::same(6.0))
                        .inner_margin(Vec2::new(14.0, 10.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.add_space(6.0);
                                let task_desc = self.export_task_name.as_deref().unwrap_or("Извлечение...");
                                ui.label(
                                    RichText::new(format!("⏳ Экспорт в процессе: {}...", task_desc))
                                        .color(Color32::WHITE)
                                        .font(FontId::proportional(13.0))
                                        .strong(),
                                );
                            });
                        });
                } else if let Some(ref res) = self.last_export_result {
                    ui.add_space(10.0);
                    match res {
                        ExportResult::Success { message, output_path } => {
                            egui::Frame::none()
                                .fill(Color32::from_rgb(16, 42, 28))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 197, 94)))
                                .rounding(Rounding::same(6.0))
                                .inner_margin(Vec2::new(14.0, 10.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(format!("✅ {}", message))
                                                .color(Color32::from_rgb(187, 247, 208))
                                                .font(FontId::proportional(13.0))
                                                .strong(),
                                        );

                                        let path_clone = output_path.clone();
                                        if ui.add(
                                            egui::Button::new(
                                                RichText::new("📂 Показать в проводнике")
                                                    .color(Color32::WHITE)
                                                    .size(12.0),
                                            )
                                            .fill(Color32::from_rgb(22, 101, 52))
                                            .rounding(Rounding::same(4.0)),
                                        ).clicked() {
                                            Self::open_in_explorer(&path_clone);
                                        }
                                    });
                                });
                        }
                        ExportResult::Error { message } => {
                            egui::Frame::none()
                                .fill(Color32::from_rgb(45, 18, 22))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(239, 68, 68)))
                                .rounding(Rounding::same(6.0))
                                .inner_margin(Vec2::new(14.0, 10.0))
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(format!("❌ {}", message))
                                            .color(Color32::from_rgb(254, 202, 202))
                                            .font(FontId::proportional(13.0)),
                                    );
                                });
                        }
                    }
                }

                ui.add_space(16.0);

                // --- 5. SUBTITLES SECTION (СУБТИТРЫ С ТАЙМИНГАМИ И СТИЛЯМИ) ---
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("💬 Дорожки субтитров (Тайминги и стили)")
                            .font(FontId::proportional(16.0))
                            .color(Color32::from_rgb(167, 139, 250)) // Purple
                            .strong(),
                    );
                    ui.label(
                        RichText::new(format!("(найдено: {})", info.subtitle_tracks.len()))
                            .font(FontId::proportional(13.0))
                            .color(Color32::GRAY),
                    );
                });

                ui.add_space(6.0);

                if info.subtitle_tracks.is_empty() {
                    egui::Frame::none()
                        .fill(Color32::from_rgb(18, 20, 27))
                        .rounding(Rounding::same(6.0))
                        .inner_margin(Vec2::new(14.0, 10.0))
                        .show(ui, |ui| {
                            ui.label(RichText::new("В этом медиафайле нет встроенных субтитров").color(Color32::GRAY));
                        });
                } else {
                    for track in &info.subtitle_tracks {
                        let stream_idx = track.stream_index;
                        let mut chosen_fmt = self
                            .sub_export_formats
                            .get(&stream_idx)
                            .copied()
                            .unwrap_or(SubtitleExportFormat::Ass);

                        egui::Frame::none()
                            .fill(Color32::from_rgb(18, 21, 29))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 52)))
                            .rounding(Rounding::same(6.0))
                            .inner_margin(Vec2::new(12.0, 10.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    // Number badge
                                    ui.label(
                                        RichText::new(format!("#{}", track.relative_index + 1))
                                            .font(FontId::monospace(13.0))
                                            .color(Color32::from_rgb(167, 139, 250))
                                            .strong(),
                                    );

                                    // Title & language
                                    let sub_title = if !track.title.is_empty() {
                                        track.title.clone()
                                    } else {
                                        format!("Субтитры {}", track.relative_index + 1)
                                    };

                                    ui.label(
                                        RichText::new(&sub_title)
                                            .font(FontId::proportional(13.5))
                                            .color(Color32::WHITE)
                                            .strong(),
                                    );

                                    if !track.language.is_empty() && track.language != "und" {
                                        ui.label(
                                            RichText::new(format!("[{}]", track.language.to_uppercase()))
                                                .font(FontId::proportional(11.5))
                                                .color(Color32::from_rgb(124, 58, 237)) // Purple
                                                .background_color(Color32::from_rgb(30, 26, 46)),
                                        );
                                    }

                                    // Source format badge
                                    let is_ass = track.codec.to_lowercase().contains("ass") || track.codec.to_lowercase().contains("ssa");
                                    let (src_codec_label, src_color) = if is_ass {
                                        ("ASS (Стили аниме)", Color32::from_rgb(192, 132, 252))
                                    } else {
                                        ("SRT", Color32::from_rgb(148, 163, 184))
                                    };
                                    ui.label(
                                        RichText::new(src_codec_label)
                                            .font(FontId::proportional(11.5))
                                            .color(src_color),
                                    );

                                    // Right-side controls: Format selector + Download button
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let download_btn = ui.add_enabled(
                                            !self.is_exporting,
                                            egui::Button::new(
                                                RichText::new("⬇ Скачать субтитры")
                                                    .font(FontId::proportional(12.5))
                                                    .color(Color32::from_rgb(10, 15, 10))
                                                    .strong(),
                                            )
                                            .fill(Color32::from_rgb(34, 197, 94)) // Emerald green
                                            .min_size(Vec2::new(140.0, 28.0))
                                            .rounding(Rounding::same(5.0)),
                                        );

                                        if download_btn.clicked() {
                                            let ext = chosen_fmt.extension();
                                            let clean_sub_name = crate::player::tracks::clean_title(&sub_title);
                                            let lang_part = if !track.language.is_empty() && track.language != "und" {
                                                format!(".{}", track.language)
                                            } else {
                                                String::new()
                                            };
                                            let default_file_name = format!(
                                                "{}{}.{}.{}",
                                                info.clean_title,
                                                lang_part,
                                                clean_sub_name.replace(' ', "_"),
                                                ext
                                            );

                                            if let Some(save_path) = rfd::FileDialog::new()
                                                .set_file_name(&default_file_name)
                                                .add_filter(&format!("Субтитры (*.{})", ext), &[ext])
                                                .save_file()
                                            {
                                                self.start_subtitle_export(
                                                    current_input_path.clone(),
                                                    track,
                                                    chosen_fmt,
                                                    save_path,
                                                );
                                            }
                                        }

                                        // Format dropdown selector
                                        egui::ComboBox::from_id_source(format!("sub_fmt_select_{}", stream_idx))
                                            .selected_text(RichText::new(chosen_fmt.display_label()).size(12.0))
                                            .width(230.0)
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(
                                                    &mut chosen_fmt,
                                                    SubtitleExportFormat::Ass,
                                                    SubtitleExportFormat::Ass.display_label(),
                                                );
                                                ui.selectable_value(
                                                    &mut chosen_fmt,
                                                    SubtitleExportFormat::Srt,
                                                    SubtitleExportFormat::Srt.display_label(),
                                                );
                                                ui.selectable_value(
                                                    &mut chosen_fmt,
                                                    SubtitleExportFormat::Vtt,
                                                    SubtitleExportFormat::Vtt.display_label(),
                                                );
                                            });

                                        self.sub_export_formats.insert(stream_idx, chosen_fmt);
                                    });
                                });
                            });
                        ui.add_space(4.0);
                    }
                }

                ui.add_space(18.0);

                // --- 6. AUDIO TRACKS SECTION (ОЗВУЧКИ) ---
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("🗣 Дорожки озвучки (Аудиопотоки)")
                            .font(FontId::proportional(16.0))
                            .color(Color32::from_rgb(52, 211, 153)) // Emerald Green
                            .strong(),
                    );
                    ui.label(
                        RichText::new(format!("(найдено: {})", info.audio_tracks.len()))
                            .font(FontId::proportional(13.0))
                            .color(Color32::GRAY),
                    );
                });

                ui.add_space(6.0);

                if info.audio_tracks.is_empty() {
                    egui::Frame::none()
                        .fill(Color32::from_rgb(18, 20, 27))
                        .rounding(Rounding::same(6.0))
                        .inner_margin(Vec2::new(14.0, 10.0))
                        .show(ui, |ui| {
                            ui.label(RichText::new("В этом медиафайле нет аудиодорожек").color(Color32::GRAY));
                        });
                } else {
                    for track in &info.audio_tracks {
                        let stream_idx = track.stream_index;
                        let mut chosen_fmt = self
                            .audio_export_formats
                            .get(&stream_idx)
                            .copied()
                            .unwrap_or(AudioExportFormat::Copy);

                        egui::Frame::none()
                            .fill(Color32::from_rgb(18, 21, 29))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 52)))
                            .rounding(Rounding::same(6.0))
                            .inner_margin(Vec2::new(12.0, 10.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    // Number badge
                                    ui.label(
                                        RichText::new(format!("#{}", track.relative_index + 1))
                                            .font(FontId::monospace(13.0))
                                            .color(Color32::from_rgb(52, 211, 153))
                                            .strong(),
                                    );

                                    // Studio / Title
                                    let audio_title = if !track.title.is_empty() {
                                        track.title.clone()
                                    } else {
                                        format!("Озвучка #{}", track.relative_index + 1)
                                    };

                                    ui.label(
                                        RichText::new(&audio_title)
                                            .font(FontId::proportional(13.5))
                                            .color(Color32::WHITE)
                                            .strong(),
                                    );

                                    // Language tag
                                    if !track.language.is_empty() && track.language != "und" {
                                        ui.label(
                                            RichText::new(format!("[{}]", track.language.to_uppercase()))
                                                .font(FontId::proportional(11.5))
                                                .color(Color32::from_rgb(16, 185, 129))
                                                .background_color(Color32::from_rgb(18, 38, 30)),
                                        );
                                    }

                                    // Channels & codec badge
                                    let ch_label = match track.channels {
                                        Some(6) => "5.1",
                                        Some(8) => "7.1",
                                        Some(2) => "2.0 Stereo",
                                        Some(1) => "Mono",
                                        _ => "",
                                    };
                                    if !ch_label.is_empty() {
                                        ui.label(
                                            RichText::new(ch_label)
                                                .font(FontId::proportional(11.5))
                                                .color(Color32::from_rgb(148, 163, 184)),
                                        );
                                    }

                                    ui.label(
                                        RichText::new(track.codec.to_uppercase())
                                            .font(FontId::proportional(11.5))
                                            .color(Color32::from_rgb(96, 165, 250)),
                                    );

                                    if let Some(br) = track.bit_rate {
                                        ui.label(
                                            RichText::new(format!("{}k", br / 1000))
                                                .font(FontId::proportional(11.5))
                                                .color(Color32::GRAY),
                                        );
                                    }

                                    // Right-side controls: Format selector + Download button
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let download_btn = ui.add_enabled(
                                            !self.is_exporting,
                                            egui::Button::new(
                                                RichText::new("⬇ Скачать озвучку")
                                                    .font(FontId::proportional(12.5))
                                                    .color(Color32::WHITE)
                                                    .strong(),
                                            )
                                            .fill(Color32::from_rgb(79, 70, 229)) // Kodik Indigo
                                            .min_size(Vec2::new(140.0, 28.0))
                                            .rounding(Rounding::same(5.0)),
                                        );

                                        if download_btn.clicked() {
                                            let ext = chosen_fmt.extension(&track.codec);
                                            let clean_audio_name = crate::player::tracks::clean_title(&audio_title);
                                            let lang_part = if !track.language.is_empty() && track.language != "und" {
                                                format!(".{}", track.language)
                                            } else {
                                                String::new()
                                            };
                                            let default_file_name = format!(
                                                "{}{}.{}.{}",
                                                info.clean_title,
                                                lang_part,
                                                clean_audio_name.replace(' ', "_"),
                                                ext
                                            );

                                            if let Some(save_path) = rfd::FileDialog::new()
                                                .set_file_name(&default_file_name)
                                                .add_filter(&format!("Аудио (*.{})", ext), &[ext])
                                                .save_file()
                                            {
                                                self.start_audio_export(
                                                    current_input_path.clone(),
                                                    track,
                                                    chosen_fmt,
                                                    save_path,
                                                );
                                            }
                                        }

                                        // Format combo selector
                                        egui::ComboBox::from_id_source(format!("audio_fmt_select_{}", stream_idx))
                                            .selected_text(RichText::new(chosen_fmt.display_label()).size(12.0))
                                            .width(190.0)
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(
                                                    &mut chosen_fmt,
                                                    AudioExportFormat::Copy,
                                                    AudioExportFormat::Copy.display_label(),
                                                );
                                                ui.selectable_value(
                                                    &mut chosen_fmt,
                                                    AudioExportFormat::Mp3,
                                                    AudioExportFormat::Mp3.display_label(),
                                                );
                                                ui.selectable_value(
                                                    &mut chosen_fmt,
                                                    AudioExportFormat::Flac,
                                                    AudioExportFormat::Flac.display_label(),
                                                );
                                                ui.selectable_value(
                                                    &mut chosen_fmt,
                                                    AudioExportFormat::Wav,
                                                    AudioExportFormat::Wav.display_label(),
                                                );
                                            });

                                        self.audio_export_formats.insert(stream_idx, chosen_fmt);
                                    });
                                });
                            });
                        ui.add_space(4.0);
                    }
                }

                ui.add_space(20.0);
            });
    }
}
