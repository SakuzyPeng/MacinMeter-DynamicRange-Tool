// No console window behind the GUI in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

//! MacinMeter's desktop GUI on egui: a prototype beside the Tauri GUI.
//!
//! Like the Tauri GUI it only parses, renders and adapts I/O over the shared
//! `macinmeter::Application`. Installers are not part of the prototype.

mod app;
mod card_layout;
mod export;
mod fonts;
mod format;
mod i18n;
mod jobs;
mod progress;
mod report_image;
mod theme;

const ICON: &[u8] = include_bytes!("../../../tauri-app/src-tauri/icons/128x128@2x.png");

fn main() {
    let initial: Vec<std::path::PathBuf> = std::env::args_os().skip(1).map(Into::into).collect();
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_app_id("com.macinmeter.dr.desktop")
        .with_title("MacinMeter DR GUI")
        .with_inner_size([900.0, 720.0])
        .with_min_inner_size([640.0, 520.0])
        .with_drag_and_drop(true);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(ICON) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    if let Err(error) = eframe::run_native(
        "MacinMeter DR GUI",
        options,
        Box::new(|creation| Ok(Box::new(app::MeterApp::new(creation, initial)))),
    ) {
        eprintln!("failed to run MacinMeter GUI: {error}");
        std::process::exit(1);
    }
}
