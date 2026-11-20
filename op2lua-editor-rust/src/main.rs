//! OP2LuaEditor: a mission layout editor for OP2Lua missions, for Windows, Linux and macOS.

#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod app;
mod celltype;
mod dll;
mod editor;
mod export;
mod launch;
mod mapdoc;
mod mission;
mod props;
mod sound;
mod units;
mod view;

/// The window's icon, the VB editor's (assets/app.ico).
fn icon() -> eframe::egui::IconData {
    let img = image::load_from_memory_with_format(include_bytes!("../assets/app.ico"), image::ImageFormat::Ico)
        .expect("the embedded icon")
        .to_rgba8();
    eframe::egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("OP2LuaEditor")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 400.0])
            .with_drag_and_drop(true)
            .with_icon(icon()),
        ..Default::default()
    };
    eframe::run_native("OP2LuaEditor", options, Box::new(|cc| Ok(Box::new(app::EditorApp::new(cc)))))
}
