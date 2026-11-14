#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod player;
mod ui;

use eframe::egui;
use ui::MediaPlayerApp;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("GStreamer Media Player (Rust)")
            .with_inner_size([1120.0, 700.0])
            .with_min_inner_size([540.0, 360.0]),
        ..Default::default()
    };

    eframe::run_native(
        "GStreamer Media Player",
        native_options,
        Box::new(|cc| {
            // Apply customized modern dark glass styling
            let mut visuals = egui::Visuals::dark();
            visuals.window_fill = egui::Color32::from_rgb(14, 16, 23);
            visuals.panel_fill = egui::Color32::from_rgb(16, 18, 25);
            visuals.override_text_color = Some(egui::Color32::from_rgb(241, 245, 249));

            visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(20, 23, 33);
            visuals.widgets.noninteractive.rounding = egui::Rounding::same(6.0);

            visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(26, 30, 44);
            visuals.widgets.inactive.rounding = egui::Rounding::same(6.0);

            visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(42, 48, 70);
            visuals.widgets.hovered.rounding = egui::Rounding::same(6.0);

            visuals.widgets.active.bg_fill = egui::Color32::from_rgb(79, 70, 229);
            visuals.widgets.active.rounding = egui::Rounding::same(6.0);

            cc.egui_ctx.set_visuals(visuals);

            match MediaPlayerApp::new(cc) {
                Ok(app) => Box::new(app),
                Err(err) => {
                    eprintln!("Critical error during app initialization: {:#}", err);
                    std::process::exit(1);
                }
            }
        }),
    )
}
