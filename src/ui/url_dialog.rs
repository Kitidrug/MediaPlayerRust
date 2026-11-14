use egui::{Context, Window};
use url::Url;

pub struct UrlDialog {
    pub is_open: bool,
    pub input_url: String,
    pub error_message: Option<String>,
}

impl Default for UrlDialog {
    fn default() -> Self {
        Self {
            is_open: false,
            input_url: String::new(),
            error_message: None,
        }
    }
}

impl UrlDialog {
    pub fn open(&mut self) {
        self.is_open = true;
        self.error_message = None;
    }

    pub fn show(&mut self, ctx: &Context) -> Option<String> {
        let mut confirmed_url = None;

        if !self.is_open {
            return None;
        }

        Window::new("Open Network Stream")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("Enter network stream URL (HTTP/HTTPS, HLS .m3u8, RTSP):");
                ui.add_space(4.0);

                let text_edit = ui.add(
                    egui::TextEdit::singleline(&mut self.input_url)
                        .hint_text("rtsp://... or https://.../playlist.m3u8")
                        .desired_width(400.0),
                );

                if let Some(err) = &self.error_message {
                    ui.add_space(4.0);
                    ui.colored_label(egui::Color32::from_rgb(255, 80, 80), err);
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Open").clicked() || (text_edit.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter))) {
                        let trimmed = self.input_url.trim();
                        match Url::parse(trimmed) {
                            Ok(url) if !url.scheme().is_empty() => {
                                confirmed_url = Some(trimmed.to_string());
                                self.is_open = false;
                                self.error_message = None;
                            }
                            _ => {
                                self.error_message = Some("Please enter a valid URL with protocol (http, https, rtsp)".into());
                            }
                        }
                    }

                    if ui.button("Cancel").clicked() {
                        self.is_open = false;
                        self.error_message = None;
                    }
                });
            });

        confirmed_url
    }
}

