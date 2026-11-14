use std::time::Duration;

use egui::{
    Color32, FontId, Pos2, Rect, Rounding, Sense, Stroke, Ui, Vec2,
};

fn fade_color(c: Color32, alpha: f32) -> Color32 {
    let [r, g, b, a] = c.to_array();
    let factor = alpha.clamp(0.0, 1.0);
    Color32::from_rgba_premultiplied(
        ((r as f32) * factor) as u8,
        ((g as f32) * factor) as u8,
        ((b as f32) * factor) as u8,
        ((a as f32) * factor) as u8,
    )
}

pub struct TimelineOutput {
    pub seek_requested: Option<Duration>,
    #[allow(dead_code)]
    pub is_dragging: bool,
    #[allow(dead_code)]
    pub hover_time: Option<Duration>,
}

pub struct VideoTimeline<'a> {
    current_time: Duration,
    duration: Option<Duration>,
    is_seeking: &'a mut bool,
    seek_slider_value: &'a mut f64,
    opacity: f32,
}

impl<'a> VideoTimeline<'a> {
    pub fn new(
        current_time: Duration,
        duration: Option<Duration>,
        is_seeking: &'a mut bool,
        seek_slider_value: &'a mut f64,
    ) -> Self {
        Self {
            current_time,
            duration,
            is_seeking,
            seek_slider_value,
            opacity: 1.0,
        }
    }

    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity.clamp(0.0, 1.0);
        self
    }

    pub fn format_duration(d: Duration) -> String {
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

    pub fn show(self, ui: &mut Ui) -> TimelineOutput {
        let duration_secs = self.duration.map(|d| d.as_secs_f64()).unwrap_or(0.0);
        let current_secs = self.current_time.as_secs_f64();

        let current_ratio = if duration_secs > 0.0 {
            (current_secs / duration_secs).clamp(0.0, 1.0)
        } else {
            0.0
        };

        // Full width timeline bar
        let available_width = ui.available_width().max(100.0);
        let touch_height = 14.0_f32;

        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(available_width, touch_height), Sense::click_and_drag());

        let is_hovered = response.hovered();
        let is_active = response.dragged() || *self.is_seeking;

        // Sleek timeline height: 3.5px resting, expands to 6.5px on hover/drag
        let bar_height = if is_hovered || is_active { 6.5_f32 } else { 3.5_f32 };
        let bar_rect = Rect::from_center_size(
            rect.center(),
            Vec2::new(rect.width(), bar_height),
        );

        let mut seek_requested = None;
        let mut hover_time = None;

        // Track cursor ratio along the bar (as f64)
        let hover_pos = response.hover_pos();
        let hover_ratio: Option<f64> = hover_pos.map(|pos| {
            (((pos.x - bar_rect.left()) / bar_rect.width()) as f64).clamp(0.0, 1.0)
        });

        if let Some(hr) = hover_ratio {
            if duration_secs > 0.0 {
                hover_time = Some(Duration::from_secs_f64(hr * duration_secs));
            }
        }

        // Dragging & seeking logic
        if response.drag_started() {
            *self.is_seeking = true;
        }

        if response.dragged() {
            if let Some(hr) = hover_ratio {
                *self.seek_slider_value = hr;
            }
        }

        if response.drag_stopped() || (response.clicked() && !response.dragged()) {
            *self.is_seeking = false;
            let final_ratio = hover_ratio.unwrap_or(current_ratio);
            *self.seek_slider_value = final_ratio;
            if duration_secs > 0.0 {
                let target = Duration::from_secs_f64(final_ratio * duration_secs);
                seek_requested = Some(target);
            }
        }

        let display_ratio = if *self.is_seeking {
            *self.seek_slider_value
        } else {
            current_ratio
        };

        let painter = ui.painter();

        // 1. Background rail (sleek dark grey like Kodik player)
        let bg_color = fade_color(Color32::from_rgb(60, 64, 76), self.opacity);
        painter.rect_filled(bar_rect, Rounding::same(bar_height / 2.0_f32), bg_color);

        // 2. Hover highlight bar (preview up to cursor)
        if (is_hovered || is_active) && hover_ratio.is_some() && self.opacity > 0.3 {
            let hr = hover_ratio.unwrap();
            let hover_bar_width = bar_rect.width() * hr as f32;
            let hover_rect = Rect::from_min_size(
                bar_rect.min,
                Vec2::new(hover_bar_width, bar_height),
            );
            painter.rect_filled(
                hover_rect,
                Rounding::same(bar_height / 2.0_f32),
                fade_color(Color32::from_white_alpha(50), self.opacity),
            );
        }

        // 3. Played progress bar (crisp white like Kodik player)
        let played_width = (bar_rect.width() * display_ratio as f32).max(bar_height);
        let played_rect = Rect::from_min_size(
            bar_rect.min,
            Vec2::new(played_width, bar_height),
        );
        let progress_color = fade_color(Color32::from_rgb(240, 243, 248), self.opacity);
        painter.rect_filled(played_rect, Rounding::same(bar_height / 2.0_f32), progress_color);

        // 4. Scrubber Thumb (circle knob visible on hover/drag)
        if (is_active || is_hovered) && self.opacity > 0.3 {
            let thumb_x = bar_rect.left() + (bar_rect.width() * display_ratio as f32);
            let thumb_pos = Pos2::new(thumb_x, bar_rect.center().y);
            let thumb_radius = 6.0_f32;

            painter.circle_filled(
                thumb_pos,
                thumb_radius + 2.0_f32,
                fade_color(Color32::from_rgba_premultiplied(255, 255, 255, 40), self.opacity),
            );
            painter.circle_filled(thumb_pos, thumb_radius, fade_color(Color32::WHITE, self.opacity));
        }

        // 5. Hover Preview Tooltip Badge (floats directly above cursor on timeline)
        if let (Some(pos), Some(ht)) = (hover_pos, hover_time) {
            if (is_hovered || is_active) && self.opacity > 0.3 {
                // Vertical cursor marker line
                let line_top = Pos2::new(pos.x, bar_rect.min.y - 4.0_f32);
                let line_bottom = Pos2::new(pos.x, bar_rect.max.y + 4.0_f32);
                painter.line_segment(
                    [line_top, line_bottom],
                    Stroke::new(1.5_f32, Color32::from_white_alpha(180)),
                );

                // Floating tooltip badge
                let tooltip_text = Self::format_duration(ht);
                let text_color = Color32::WHITE;
                let badge_font = FontId::monospace(11.5_f32);

                let text_layout = painter.layout_no_wrap(
                    tooltip_text,
                    badge_font,
                    text_color,
                );

                let badge_padding = Vec2::new(8.0_f32, 4.0_f32);
                let badge_size = text_layout.size() + badge_padding * 2.0_f32;

                // Clamp badge X so it doesn't leave window borders
                let badge_x = (pos.x - badge_size.x / 2.0_f32)
                    .clamp(rect.left(), rect.right() - badge_size.x);
                let badge_y = bar_rect.top() - badge_size.y - 8.0_f32;

                let badge_rect = Rect::from_min_size(
                    Pos2::new(badge_x, badge_y),
                    badge_size,
                );

                painter.rect_filled(
                    badge_rect,
                    Rounding::same(4.0_f32),
                    fade_color(Color32::from_rgb(20, 22, 28), self.opacity),
                );
                painter.rect_stroke(
                    badge_rect,
                    Rounding::same(4.0_f32),
                    Stroke::new(1.0_f32, fade_color(Color32::from_rgb(60, 64, 76), self.opacity)),
                );

                let text_pos = badge_rect.min + badge_padding;
                painter.galley(text_pos, text_layout, fade_color(text_color, self.opacity));
            }
        }

        TimelineOutput {
            seek_requested,
            is_dragging: *self.is_seeking,
            hover_time,
        }
    }
}
